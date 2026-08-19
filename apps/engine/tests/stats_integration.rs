//! 統計層與引擎的端到端測試。
//!
//! 驗證的不只是「算得出數字」，而是**數字帶著它的可信度**：
//! estimator 名稱、有效樣本、可判定狀態一路傳到底。

use poker_engine::betting::Action;
use poker_engine::chips::Chips;
use poker_engine::hand::ActionProvider;
use poker_engine::rng::{Rng, RngDomain};
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::stats::{
    benjamini_hochberg, estimate_block_length, moving_block_bootstrap, preview_all, wilson,
    AnalysisLevel, Observation, Test, Verdict, DEFAULT_RESAMPLES, PLANNING_SIGMA_BB100,
};
use poker_engine::strategy::DecisionView;
use poker_engine::table::TableConfig;

fn c(n: u64) -> Chips {
    Chips::new(n)
}

struct CallingStation;

impl ActionProvider for CallingStation {
    fn choose(&mut self, view: &DecisionView) -> Action {
        let legal = &view.legal;
        if legal.can_check {
            Action::Check
        } else if legal.call_to.is_some() {
            Action::Call
        } else {
            Action::AllIn
        }
    }
}

/// 跑一個 run，收集使用者的逐手觀測。
fn collect(hands: u64) -> Vec<Observation> {
    let config = SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![c(400); 9],
        auto_refill: Some(9),
        hero_seat: 0,
        hand_limit: hands,
        master_seed: 31337,
    };

    let big_blind = f64::from(u32::try_from(config.table.big_blind.units()).unwrap_or(2));
    let hero = config.hero_seat;
    let mut out = Vec::new();
    run_session(&config, &mut CallingStation, |played| {
        let contributed = played.result.total_contributions[hero];
        let gained = played.result.distribution.payouts[hero] + played.result.distribution.refunds[hero];
        let delta = f64::from(u32::try_from(gained.units()).unwrap_or(0))
            - f64::from(u32::try_from(contributed.units()).unwrap_or(0));
        out.push(Observation {
            delta_bb: delta / big_blind,
            instance: played.instance_index,
        });
    });
    out
}

#[test]
fn 由真實_run_算出帶可信度的估計() {
    let observations = collect(20_000);
    assert!(!observations.is_empty());

    let series: Vec<f64> = observations.iter().map(|o| o.delta_bb).collect();
    let block_length = estimate_block_length(&series);
    assert!(block_length >= 1);

    let mut rng = Rng::derive(1, 1, RngDomain::Stats);
    let estimate = moving_block_bootstrap(&observations, block_length, DEFAULT_RESAMPLES, &mut rng);

    assert_eq!(estimate.hands, observations.len());
    assert!(estimate.ci_low <= estimate.point, "點估計必須落在 CI 內");
    assert!(estimate.point <= estimate.ci_high);
    // estimator 名稱必須可供報表顯示（核心規格 5.3）
    assert!(!estimate.estimator.as_str().is_empty());
}

/// 全桌同策略是對稱賽局，EV 應與 0 無法區分。
///
/// 這則驗證的是**統計層不會製造假信心**：資料裡沒有效應時，
/// 它必須說「無法判定」，而不是把雜訊當成結論。
#[test]
fn 全桌同策略時判為無法判定而非硬給結論() {
    let observations = collect(50_000);
    let series: Vec<f64> = observations.iter().map(|o| o.delta_bb).collect();
    let block_length = estimate_block_length(&series);

    let mut rng = Rng::derive(2, 2, RngDomain::Stats);
    let estimate = moving_block_bootstrap(&observations, block_length, DEFAULT_RESAMPLES, &mut rng);

    assert_eq!(
        estimate.verdict,
        Verdict::Indeterminate,
        "9 個座位都是同一策略，EV 應與 0 無法區分（實得 {:.1} bb/100，CI [{:.1}, {:.1}]）",
        estimate.point,
        estimate.ci_low,
        estimate.ci_high
    );
    assert!(
        estimate.ci_low <= 0.0 && estimate.ci_high >= 0.0,
        "對稱賽局的 CI 必須涵蓋 0"
    );
}

/// 使用者採明顯較差的策略時，統計層必須能判定出來。
///
/// 與上一則互補：上一則驗證「沒有效應時不製造結論」，
/// 這一則驗證「有真實效應時抓得到」。兩者都通過才代表 estimator 可用。
#[test]
fn 使用者策略明顯較差時可判定為負() {
    let config = SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![c(400); 9],
        auto_refill: Some(9),
        hero_seat: 0,
        hand_limit: 30_000,
        master_seed: 90210,
    };

    let big_blind = 2.0;
    let hero = config.hero_seat;
    let mut observations = Vec::new();
    run_session(&config, &mut AlwaysFoldHero { hero }, |played| {
        let contributed = played.result.total_contributions[hero];
        let gained =
            played.result.distribution.payouts[hero] + played.result.distribution.refunds[hero];
        let delta = f64::from(u32::try_from(gained.units()).unwrap_or(0))
            - f64::from(u32::try_from(contributed.units()).unwrap_or(0));
        observations.push(Observation {
            delta_bb: delta / big_blind,
            instance: played.instance_index,
        });
    });

    let series: Vec<f64> = observations.iter().map(|o| o.delta_bb).collect();
    let block_length = estimate_block_length(&series);
    let mut rng = Rng::derive(6, 6, RngDomain::Stats);
    let estimate = moving_block_bootstrap(&observations, block_length, DEFAULT_RESAMPLES, &mut rng);

    assert_eq!(
        estimate.verdict,
        Verdict::Determinate,
        "使用者一律棄牌，只輸盲注、永遠贏不到底池，5 萬手必然可判定為負\
         （實得 {:.1} bb/100，CI [{:.1}, {:.1}]）",
        estimate.point,
        estimate.ci_low,
        estimate.ci_high
    );
    assert!(estimate.ci_high < 0.0, "CI 上界應為負");
}

/// 使用者一律棄牌，其餘座位正常跟注。
///
/// 選這個組合而非「使用者跟注、對手加注」，是因為前者的方向無爭議：
/// 使用者只會輸掉盲注、永遠贏不到任何底池，EV 必為負。
/// 後者實測反而讓使用者小贏（對手互相加注造成的結構效應），
/// 不適合當「統計層抓不抓得到真實效應」的測試基準。
struct AlwaysFoldHero {
    hero: usize,
}

impl ActionProvider for AlwaysFoldHero {
    fn choose(&mut self, view: &DecisionView) -> Action {
        if view.seat == self.hero {
            return Action::Fold;
        }
        let legal = &view.legal;
        if legal.can_check {
            Action::Check
        } else if legal.call_to.is_some() {
            Action::Call
        } else {
            Action::AllIn
        }
    }
}

#[test]
fn 手數太少時標為樣本不足而非硬給結論() {
    let observations = collect(10_000);
    let mut rng = Rng::derive(3, 3, RngDomain::Stats);
    // 刻意用很長的 block，讓有效樣本降到門檻以下
    let estimate = moving_block_bootstrap(&observations, 500, DEFAULT_RESAMPLES, &mut rng);
    assert_eq!(
        estimate.verdict,
        Verdict::InsufficientSample,
        "有效樣本 {} 個 block 低於門檻時必須標樣本不足",
        estimate.effective_blocks
    );
}

#[test]
fn 逐位置切片依在桌人數分開統計() {
    let config_hands = 30_000;
    let observations = collect(config_hands);

    // 以桌次切片模擬「依當手在桌人數切片」的資料結構要求
    let instances: std::collections::BTreeSet<u64> =
        observations.iter().map(|o| o.instance).collect();
    assert!(!instances.is_empty(), "應至少有一個桌次");

    // 每個切片各自估計，且各自帶自己的有效樣本
    let mut rng = Rng::derive(4, 4, RngDomain::Stats);
    for instance in instances.iter().take(3) {
        let slice: Vec<Observation> = observations
            .iter()
            .copied()
            .filter(|o| o.instance == *instance)
            .collect();
        let estimate = moving_block_bootstrap(&slice, 20, 200, &mut rng);
        assert_eq!(estimate.hands, slice.len());
        // 切片的有效樣本必然小於整體，因此更容易被判樣本不足——這是預期行為
        assert!(estimate.effective_blocks <= observations.len() / 20);
    }
}

#[test]
fn 行為頻率以分子分母加_wilson_呈現() {
    let observations = collect(20_000);
    // 以「該手是否為正收益」當作示範比例
    let wins = observations.iter().filter(|o| o.delta_bb > 0.0).count();
    let total = observations.len();

    let proportion = wilson(
        u64::try_from(wins).expect("計數必在 u64 範圍"),
        u64::try_from(total).expect("計數必在 u64 範圍"),
    );
    assert_eq!(
        proportion.numerator,
        u64::try_from(wins).expect("計數"),
        "分子必須保留供 UI 顯示"
    );
    assert_eq!(
        proportion.denominator,
        u64::try_from(total).expect("計數"),
        "分母必須保留"
    );
    let point = proportion.point().expect("分母非 0");
    assert!(proportion.ci_low <= point && point <= proportion.ci_high);
}

#[test]
fn 執行前的效力預覽反映規格結論() {
    // 核心規格 5.3.1：169 格在 1000K 上限下仍無法判定
    let previews = preview_all(1_000_000, 9, PLANNING_SIGMA_BB100);
    let grid = previews
        .iter()
        .find(|p| matches!(p.level, AnalysisLevel::HandClassGrid))
        .expect("含 169 格層級");
    assert!(!grid.usable, "169 格在上限手數下仍不可判定");

    let overall = previews
        .iter()
        .find(|p| matches!(p.level, AnalysisLevel::Overall))
        .expect("含整體層級");
    assert!(overall.usable, "整體 bb/100 在 100 萬手下應可用");
}

#[test]
fn 熱力圖的漏洞旗標必須經過_fdr_控制() {
    // 模擬 169 格各自做一次檢定，全部為虛無
    let observations = collect(20_000);
    let mut rng = Rng::derive(5, 5, RngDomain::Stats);

    // 把資料隨機切成 169 份，每份各自估計；全部來自同一分佈，
    // 因此任何「顯著」都是假陽性
    let chunk = observations.len() / 169;
    let mut tests = Vec::new();
    for id in 0..169 {
        let slice = &observations[id * chunk..(id + 1) * chunk];
        let estimate = moving_block_bootstrap(slice, 10, 200, &mut rng);
        // 以 CI 是否跨 0 粗略換算 p 值的替代指標
        let p = if estimate.verdict == Verdict::Determinate {
            0.01
        } else {
            0.5
        };
        tests.push(Test { id, p_value: p });
    }

    let results = benjamini_hochberg(&tests, 0.05);
    assert_eq!(results.len(), 169);
    // 重點不是旗標數為 0，而是 FDR 控制確實作用在整組檢定上
    let flagged = results.iter().filter(|r| r.significant).count();
    let naive = tests.iter().filter(|t| t.p_value <= 0.05).count();
    assert!(
        flagged <= naive,
        "FDR 控制後的旗標數不得多於未校正時（{flagged} vs {naive}）"
    );
}
