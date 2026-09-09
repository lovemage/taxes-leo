//! 面板 F 報表的驗收測試。
//!
//! 這裡守的是三件在 UI 上看不出來、但錯了會讓整份報表失去意義的事：
//!
//! 1. **統計主體只有使用者座位**（核心規格 5.0），而且總盈虧必須與逐手
//!    log 對得起來——報表與 log 各算各的話，使用者只會知道其中一個是錯的。
//! 2. **誠實揭露不是選配**（UI 規格 F.5）：比例一定帶分子／分母與區間，
//!    分母為 0 時是 N/A 而不是 0%；EV 一定帶 estimator、有效樣本與判定。
//! 3. **`include_dead` 只作用於逐位置切片**（F.4）。整體卡照樣含 dead 手。

use poker_engine::betting::Action;
use poker_engine::chips::Chips;
use poker_engine::hand::ActionProvider;
use poker_engine::rng::RNG_VERSION;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::strategy::DecisionView;
use poker_engine::table::TableConfig;
use poker_storage::codec::{HandRecord, LOG_FORMAT_VERSION};
use poker_storage::manifest::{
    ContentSnapshot, ExecutionMode, RuleVariants, RunManifest, SCHEMA_VERSION,
};
use poker_storage::Store;

use poker_engine::stats::MIN_CLUSTERS_FOR_BOOTSTRAP;
use poker_ipc::report::{ProportionView, ReportView};
use poker_ipc::IpcHandler;

const HERO: usize = 0;
const BIG_BLIND: u64 = 2;

fn c(n: u64) -> Chips {
    Chips::new(n)
}

/// 全程跟注。決策內容不是本檔的重點，重點是它會產生攤牌、棄牌與全下
/// 三種結局，讓每一個比例指標都有非零分母
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

fn manifest(config: &SessionConfig) -> RunManifest {
    RunManifest {
        engine_version: "0.1.0".to_owned(),
        schema_version: SCHEMA_VERSION,
        log_format_version: LOG_FORMAT_VERSION,
        rng_algorithm: RNG_VERSION.to_owned(),
        stream_derivation: "splitmix64 → xoshiro256**".to_owned(),
        master_seed: config.master_seed,
        execution_mode: ExecutionMode::Batch,
        hand_limit: config.hand_limit,
        players: config.players,
        hero_seat: config.hero_seat,
        starting_stacks: config.starting_stacks.iter().map(|c| c.units()).collect(),
        small_blind: 1,
        big_blind: BIG_BLIND,
        ante_mode: "none".to_owned(),
        ante_amount: 0,
        straddle_amounts: Vec::new(),
        rake_basis_points: 0,
        rake_cap: 0,
        rake_no_flop_no_drop: false,
        stack_policy: "bustOut".to_owned(),
        auto_refill_target: config.auto_refill,
        rule_variants: RuleVariants::default(),
        hero_strategy: ContentSnapshot::new("測試", "v0", serde_json::json!({})),
        bot_personas: Vec::new(),
        baseline_version: "none".to_owned(),
        instances: Vec::new(),
        created_at: 1_771_200_000,
        completed: false,
        checkpoint_version: 1,
    }
}

/// 補碼到 9 人。沒有人出局，因此不會有 dead button／dead small blind
fn full_table() -> SessionConfig {
    SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![c(400); 9],
        auto_refill: Some(9),
        hero_seat: HERO,
        hand_limit: 300,
        master_seed: 4242,
    }
}

/// 不補碼，且使用者以外的座位只有 20 籌碼。跟注站互相全下，座位會一個個
/// 空出來——`include_dead` 要有東西可測，就必須真的打出 dead button 的手。
///
/// 手數開到 900 是為了讓桌次數跨過 [`MIN_CLUSTERS_FOR_BOOTSTRAP`]：桌次
/// 不夠的話比例會全部退回 Wilson，cluster bootstrap 這條路在整合層等於
/// 沒有被走到（實測每個桌次約 33 手）
fn busting_table() -> SessionConfig {
    let mut stacks = vec![c(20); 9];
    // 使用者不出局，run 才不會在第一次全下就結束
    stacks[HERO] = c(4_000);
    SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: stacks,
        auto_refill: None,
        hero_seat: HERO,
        hand_limit: 900,
        master_seed: 777,
    }
}

/// 跑一個 run、落 log，回傳 handler 與 run_id。
fn prepared(config: &SessionConfig) -> (IpcHandler, i64) {
    let mut store = Store::open_in_memory().expect("建立資料庫");
    let run_id = store.create_run(&manifest(config)).expect("建立 run");

    let mut rows = Vec::new();
    let summary = run_session(config, &mut CallingStation, |played| {
        let record = HandRecord::from_played(played);
        let contributed = played.result.total_contributions[config.hero_seat];
        let delta = record.hero_delta(config.hero_seat, contributed);
        rows.push((record, played.seated, delta));
    });
    store.write_hands(run_id, &rows).expect("寫入");

    let mut m = manifest(config);
    m.instances = summary
        .instances
        .iter()
        .map(|i| poker_storage::InstanceRecord {
            index: i.index,
            first_hand: 0,
            last_hand: i.hands.saturating_sub(1),
            hands: i.hands,
            end: format!("{:?}", i.end),
            refills: Vec::new(),
        })
        .collect();
    m.completed = true;
    store
        .finish_run(run_id, &m, summary.hands_played)
        .expect("結束 run");

    (IpcHandler::new(store), run_id)
}

/// 報表裡出現的每一個比例指標：頻率、整體卡與逐位置表
fn all_proportions(report: &ReportView) -> Vec<&ProportionView> {
    let mut out: Vec<&ProportionView> = report.frequencies.iter().collect();
    out.push(&report.overall.hands_won);
    out.push(&report.overall.showdown_won);
    for row in &report.positions {
        out.push(&row.hands_won);
        out.push(&row.vpip);
        out.push(&row.pfr);
    }
    out
}

// ── 統計主體與對帳（核心規格 5.0）────────────────────────────────────

#[test]
fn 報表淨盈虧與逐手_log_對得起來() {
    let config = full_table();
    let (handler, run_id) = prepared(&config);
    let report = handler.report(run_id, false).expect("報表");

    // log 端自己加總一次。兩條算路對不起來的話，使用者看到的總盈虧
    // 就跟他逐手翻出來的不是同一件事
    let mut total_units = 0i64;
    let mut offset = 0u64;
    loop {
        let page = handler.list_hands(run_id, offset, 500).expect("逐手摘要");
        if page.is_empty() {
            break;
        }
        offset += page.len() as u64;
        total_units += page.iter().map(|h| h.hero_delta).sum::<i64>();
    }

    #[allow(clippy::cast_precision_loss)]
    let expected = total_units as f64 / BIG_BLIND as f64;
    assert!(
        (report.overall.net_bb - expected).abs() < 1e-9,
        "報表 {} bb 與 log {} bb 不一致",
        report.overall.net_bb,
        expected
    );
    assert_eq!(report.scope.hands, offset, "整體卡的手數應為全部手數");
    assert_eq!(report.scope.hero_seat as usize, HERO);
}

#[test]
fn 整體卡不排除_dead_手而逐位置切片依設定排除() {
    let config = busting_table();
    let (handler, run_id) = prepared(&config);
    let excluded = handler.report(run_id, false).expect("排除 dead");
    let included = handler.report(run_id, true).expect("納入 dead");

    assert!(
        excluded.dead_hands > 0,
        "測試資料必須真的打出 dead button／dead small blind 手，否則這條測試沒有守到東西"
    );

    // 整體卡是同一份數字：dead 手照樣是使用者打過的手
    assert_eq!(excluded.overall, included.overall);
    assert_eq!(excluded.scope.hands, included.scope.hands);
    assert_eq!(excluded.frequencies, included.frequencies);
    assert_eq!(excluded.dead_hands, included.dead_hands);
    assert!(!excluded.include_dead && included.include_dead);

    // 逐位置切片才受影響
    let excluded_hands: u64 = excluded.positions.iter().map(|p| p.ev.hands).sum();
    let included_hands: u64 = included.positions.iter().map(|p| p.ev.hands).sum();
    assert!(
        included_hands > excluded_hands,
        "納入 dead 手後逐位置切片的手數必須增加（{included_hands} vs {excluded_hands}）"
    );
}

#[test]
fn 逐位置列依在桌人數與位置切片不合併() {
    let config = busting_table();
    let (handler, run_id) = prepared(&config);
    let report = handler.report(run_id, false).expect("報表");

    // 出局會讓在桌人數往下掉，因此這份資料必然橫跨多種桌型
    let seat_counts: std::collections::BTreeSet<u8> =
        report.positions.iter().map(|p| p.seated).collect();
    assert!(
        seat_counts.len() > 1,
        "9 人桌的 BTN 與 7 人桌的 BTN 是不同母體，測試資料必須涵蓋多種桌型"
    );

    // （在桌人數 × 位置）是主鍵，不得出現重複列
    let mut keys: Vec<(u8, &str)> = report
        .positions
        .iter()
        .map(|p| (p.seated, p.position.as_str()))
        .collect();
    let total = keys.len();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), total, "逐位置表出現重複的（在桌人數, 位置）列");

    for row in &report.positions {
        assert!(
            usize::from(row.seated) <= config.players,
            "在桌人數不得超過桌位數"
        );
        assert!(row.ev.hands > 0, "空切片不該出現在表上");
    }
}

// ── 誠實揭露（UI 規格 F.5／核心規格 5.3）────────────────────────────

#[test]
fn 比例指標一律附分子分母與區間() {
    let (handler, run_id) = prepared(&full_table());
    let report = handler.report(run_id, false).expect("報表");

    for p in all_proportions(&report) {
        assert!(!p.label.is_empty(), "指標必須有名稱");
        assert!(!p.definition.is_empty(), "{} 缺少定義說明", p.label);
        assert!(
            !p.estimator.is_empty(),
            "{} 缺少 estimator 名稱（核心規格 5.3）",
            p.label
        );
        assert!(
            p.numerator <= p.denominator,
            "{}：分子 {} 大於分母 {}",
            p.label,
            p.numerator,
            p.denominator
        );

        match p.point {
            // 分母為 0 一律是 N/A，不得以 0% 冒充「從來不做這件事」
            None => {
                assert_eq!(p.denominator, 0, "{} 有樣本卻沒有點估計", p.label);
                assert!(p.ci_low.is_none() && p.ci_high.is_none());
            }
            Some(point) => {
                assert!(p.denominator > 0);
                let (low, high) = (p.ci_low.expect("下界"), p.ci_high.expect("上界"));
                assert!(
                    (0.0..=1.0).contains(&point)
                        && (0.0..=1.0).contains(&low)
                        && (0.0..=1.0).contains(&high),
                    "{} 的比例與區間必須落在 0～1",
                    p.label
                );
                // 端點容差：p̂ 落在 0 或 1 時（跟注站看到翻牌就必然攤牌，
                // WTSD 正是這種情形），Wilson 端點在數學上恰等於 p̂，
                // 但由浮點運算逼近，最後一位可能差一個 ulp
                const EPS: f64 = 1e-9;
                assert!(
                    low - EPS <= point && point <= high + EPS,
                    "{} 的區間未包含點估計",
                    p.label
                );
            }
        }
    }
}

#[test]
fn 頻率清單涵蓋規格列出的七個行為指標() {
    let (handler, run_id) = prepared(&full_table());
    let report = handler.report(run_id, false).expect("報表");

    let labels: Vec<&str> = report.frequencies.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "VPIP",
            "PFR",
            "3-bet",
            "C-bet",
            "Fold to C-bet",
            "WTSD",
            "W$SD"
        ]
    );
    // VPIP／PFR 的分母是總手數，與整體卡同一份樣本
    assert_eq!(report.frequencies[0].denominator, report.scope.hands);
    assert_eq!(report.frequencies[1].denominator, report.scope.hands);
}

#[test]
fn 比例的_estimator_與有效桌次數如實揭露() {
    // 同一桌次內的手牌不是獨立樣本，因此比例的區間走 cluster bootstrap；
    // 桌次不足時退回 Wilson，但必須說出來（核心規格 5.3、UI 規格 F.3.1）
    let (handler, run_id) = prepared(&busting_table());
    let report = handler.report(run_id, false).expect("報表");

    for p in all_proportions(&report) {
        assert!(
            p.effective_clusters <= report.scope.instance_count,
            "{}：有效桌次 {} 不得超過總桌次 {}",
            p.label,
            p.effective_clusters,
            report.scope.instance_count
        );

        let clusters =
            usize::try_from(p.effective_clusters).expect("桌次數必在 usize 範圍");
        if clusters >= MIN_CLUSTERS_FOR_BOOTSTRAP {
            assert!(
                p.estimator.contains("cluster bootstrap"),
                "{}：{} 個桌次足以做 cluster bootstrap，卻標成「{}」",
                p.label,
                clusters,
                p.estimator
            );
        } else {
            assert!(
                p.estimator.contains("Wilson"),
                "{}：只有 {} 個桌次時不得冒充 cluster bootstrap（標成「{}」）",
                p.label,
                clusters,
                p.estimator
            );
        }
    }

    assert!(
        all_proportions(&report)
            .iter()
            .any(|p| p.estimator.contains("cluster bootstrap")),
        "測試資料的桌次數（實際 {}）必須跨過 {MIN_CLUSTERS_FOR_BOOTSTRAP} 的門檻，否則 cluster bootstrap 這條路在整合層根本沒被走到",
        report.scope.instance_count
    );
}

#[test]
fn ev_估計恆帶區間_estimator_與有效樣本() {
    let (handler, run_id) = prepared(&full_table());
    let report = handler.report(run_id, false).expect("報表");

    let ev = &report.overall.ev;
    assert!(ev.ci_low <= ev.point && ev.point <= ev.ci_high, "區間未包含點估計");
    assert!(ev.half_width >= 0.0);
    assert!(!ev.estimator.is_empty(), "estimator 名稱必須顯示（核心規格 5.3）");
    assert!(!ev.verdict.is_empty(), "判定狀態必須顯示（F.5）");
    assert_eq!(ev.hands, report.scope.hands);
    assert!(
        ev.effective_blocks > 0 && ev.effective_blocks <= ev.hands,
        "有效樣本以 block 數計，且不得超過手數"
    );

    assert!(report.overall.sigma_bb >= 0.0);
    assert!(report.overall.max_drawdown_bb >= 0.0);
    // 每百手標準差是每手的 10 倍（sqrt(100)）
    assert!((report.overall.sigma100_bb - report.overall.sigma_bb * 10.0).abs() < 1e-9);
}

#[test]
fn all_in_ev_在有分段_equity_之前恆為_null() {
    let (handler, run_id) = prepared(&full_table());
    let report = handler.report(run_id, false).expect("報表");
    // 核心規格 5.2 要的是 all-in 節點之後分段的 equity；用實際結算冒充
    // 會讓它退化成另一個 EV，失去排除 runout 運氣的唯一用途
    assert!(report.overall.all_in_ev_bb100.is_none());
}

// ── 可重現性與契約形狀 ───────────────────────────────────────────────

#[test]
fn 同一個_run_的報表每次都算出同一份數字() {
    let (handler, run_id) = prepared(&full_table());
    // bootstrap 會抽樣。沒有固定種子的話，重開面板就看到不一樣的 CI，
    // 使用者會合理地認為其中一次是錯的
    assert_eq!(
        handler.report(run_id, false).expect("第一次"),
        handler.report(run_id, false).expect("第二次")
    );
}

#[test]
fn 報表_dto_序列化為_camel_case_供前端直接使用() {
    let (handler, run_id) = prepared(&full_table());
    let report = handler.report(run_id, false).expect("報表");
    let json = serde_json::to_value(&report).expect("序列化");

    assert!(json.get("includeDead").is_some());
    assert!(json.get("deadHands").is_some());
    assert!(json.get("include_dead").is_none());

    let scope = json.get("scope").expect("scope");
    for key in ["runId", "heroSeat", "bigBlind", "engineVersion", "createdAt"] {
        assert!(scope.get(key).is_some(), "scope 缺少 {key}");
    }

    let overall = json.get("overall").expect("overall");
    for key in ["netBb", "handsWon", "showdownWon", "sigma100Bb", "maxDrawdownBb"] {
        assert!(overall.get(key).is_some(), "overall 缺少 {key}");
    }
    // null 必須實際出現在 JSON 裡，前端才知道是「沒有這個數字」
    assert!(overall.get("allInEvBb100").expect("allInEvBb100").is_null());

    let ev = overall.get("ev").expect("ev");
    for key in ["ciLow", "ciHigh", "halfWidth", "effectiveBlocks"] {
        assert!(ev.get(key).is_some(), "ev 缺少 {key}");
    }

    let vpip = json
        .get("frequencies")
        .and_then(|f| f.get(0))
        .expect("第一個頻率");
    for key in ["ciLow", "ciHigh", "estimator", "effectiveClusters"] {
        assert!(vpip.get(key).is_some(), "比例缺少 {key}");
    }

    let tables = json.get("tables").expect("tables");
    for key in ["instanceCount", "avgHandsAlive", "endReasons", "refillCount"] {
        assert!(tables.get(key).is_some(), "tables 缺少 {key}");
    }
}

#[test]
fn 桌次卡的結束原因與桌次數與_manifest_一致() {
    let (handler, run_id) = prepared(&busting_table());
    let report = handler.report(run_id, false).expect("報表");
    let run = handler.get_run(run_id).expect("run 摘要");

    let tables = &report.tables;
    assert_eq!(tables.instance_count, report.scope.instance_count);
    assert_eq!(
        tables.end_reasons.iter().map(|r| r.count).sum::<u64>(),
        tables.instance_count,
        "每個桌次恰有一個結束原因"
    );
    assert!(tables.avg_hands_alive > 0.0);
    assert_eq!(run.hands_played, report.scope.hands);
}

#[test]
fn 不存在的_run_回傳錯誤而不是空報表() {
    let (handler, run_id) = prepared(&full_table());
    assert!(handler.report(run_id + 999, false).is_err());
}
