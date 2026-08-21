//! Bot 決策接線的行為驗證。
//!
//! 在這之前執行層對全部座位用的是佔位策略，所以「調參數」是個假動作。
//! 這組測試守的就是那件事不再是假的。

use std::collections::BTreeMap;

use poker_engine::betting::{Action, LegalActions, RaiseRange};
use poker_engine::bot::{scenario_of, BotAgent, BotConfig};
use poker_engine::card::{Card, Rank, Suit};
use poker_engine::chips::Chips;
use poker_engine::hand::{ActionProvider, Street};
use poker_engine::position::PositionLabel;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::strategy::baseline::BaselineRules;
use poker_engine::strategy::decision::PublicAction;
use poker_engine::strategy::{DecisionView, StackBucket};
use poker_engine::table::TableConfig;

/// 取樣數刻意低於內容級門檻：這裡驗的是接線與參數效果，
/// 不是範圍內容本身的精度（那由 calibration 那組測試負責）。
const FAST_SAMPLES: u64 = 2_000;

fn view_with(history: Vec<PublicAction>, seat: usize) -> DecisionView {
    DecisionView {
        seat,
        position: PositionLabel::Btn,
        street: Street::Preflop,
        hole_cards: [
            Card::new(Rank::Ace, Suit::Spades),
            Card::new(Rank::King, Suit::Spades),
        ],
        board: Vec::new(),
        seated: 9,
        effective_stack_bucket: StackBucket::Deep,
        pot: Chips::new(3),
        to_call: Chips::new(2),
        legal: LegalActions {
            seat,
            can_fold: true,
            can_check: false,
            call_to: Some(Chips::new(2)),
            raise: Some(RaiseRange {
                min_to: Chips::new(4),
                max_to: Chips::new(400),
            }),
            all_in_to: Some(Chips::new(400)),
        },
        history,
        opponents: Vec::new(),
    }
}

fn acted(seat: usize, position: PositionLabel, action: Action) -> PublicAction {
    PublicAction {
        street: Street::Preflop,
        seat,
        position,
        action,
    }
}

// ── 翻前情境識別（核心規格 4.1 的 node 要素）────────────────────────────

#[test]
fn 無人進池為_unopened() {
    use poker_engine::strategy::PreflopScenario;
    let history = vec![
        acted(1, PositionLabel::Utg, Action::Fold),
        acted(2, PositionLabel::Utg1, Action::Fold),
    ];
    assert_eq!(scenario_of(&view_with(history, 8)), PreflopScenario::Unopened);
}

#[test]
fn 只有跟注者為_vs_limp() {
    use poker_engine::strategy::PreflopScenario;
    let history = vec![
        acted(1, PositionLabel::Utg, Action::Call),
        acted(2, PositionLabel::Utg1, Action::Fold),
        acted(3, PositionLabel::Utg2, Action::Call),
    ];
    assert_eq!(
        scenario_of(&view_with(history, 8)),
        PreflopScenario::VsLimp { limpers: 2 }
    );
}

/// 面對開牌的節點必須記住**開牌者是誰**——同樣是面對開牌，
/// 對 UTG 與對 BTN 該打的範圍差很多。
#[test]
fn 單一加注為_vs_open_且記得開牌者() {
    use poker_engine::strategy::PreflopScenario;
    let history = vec![
        acted(1, PositionLabel::Utg, Action::Fold),
        acted(2, PositionLabel::Utg1, Action::RaiseTo(Chips::new(6))),
        acted(3, PositionLabel::Utg2, Action::Fold),
    ];
    assert_eq!(
        scenario_of(&view_with(history, 8)),
        PreflopScenario::VsOpen {
            opener: PositionLabel::Utg1
        }
    );
}

/// squeeze 與單純 3-bet 的差別是中間那個跟注者，兩者範圍不同。
#[test]
fn 英雄開牌後有人跟注再被加注為_squeeze() {
    use poker_engine::strategy::PreflopScenario;
    let hero = 8;
    let history = vec![
        acted(hero, PositionLabel::Btn, Action::RaiseTo(Chips::new(6))),
        acted(0, PositionLabel::Sb, Action::Call),
        acted(1, PositionLabel::Bb, Action::RaiseTo(Chips::new(24))),
    ];
    assert_eq!(
        scenario_of(&view_with(history, hero)),
        PreflopScenario::VsSqueeze {
            by: PositionLabel::Bb
        }
    );
}

#[test]
fn 英雄開牌後直接被加注為_3bet() {
    use poker_engine::strategy::PreflopScenario;
    let hero = 8;
    let history = vec![
        acted(hero, PositionLabel::Btn, Action::RaiseTo(Chips::new(6))),
        acted(1, PositionLabel::Bb, Action::RaiseTo(Chips::new(24))),
    ];
    assert_eq!(
        scenario_of(&view_with(history, hero)),
        PreflopScenario::VsThreeBet {
            by: PositionLabel::Bb
        }
    );
}

#[test]
fn 三次加注為_vs_4bet() {
    use poker_engine::strategy::PreflopScenario;
    let hero = 8;
    let history = vec![
        acted(2, PositionLabel::Utg1, Action::RaiseTo(Chips::new(6))),
        acted(hero, PositionLabel::Btn, Action::RaiseTo(Chips::new(18))),
        acted(2, PositionLabel::Utg1, Action::RaiseTo(Chips::new(48))),
    ];
    assert_eq!(
        scenario_of(&view_with(history, hero)),
        PreflopScenario::VsFourBet {
            by: PositionLabel::Utg1
        }
    );
}

// ── 參數確實會改變行為 ──────────────────────────────────────────────────

/// 統計一個 run 的行動組成。
struct Tally {
    agent: BotAgent,
    folds: u32,
    raises: u32,
    total: u32,
}

impl Tally {
    fn raise_rate(&self) -> f64 {
        f64::from(self.raises) / f64::from(self.total.max(1))
    }
}

impl ActionProvider for Tally {
    fn choose(&mut self, view: &DecisionView) -> Action {
        let action = self.agent.choose(view);
        self.total += 1;
        match action {
            Action::Fold => self.folds += 1,
            Action::RaiseTo(_) | Action::AllIn => self.raises += 1,
            _ => {}
        }
        action
    }
}

fn play(seats: Vec<BotConfig>, rankings: &BTreeMap<usize, poker_engine::strategy::ranking::EquityRanking>) -> Tally {
    let config = SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![Chips::new(400); 9],
        auto_refill: Some(9),
        hero_seat: 0,
        hand_limit: 400,
        master_seed: 20_260_821,
    };
    let mut tally = Tally {
        agent: BotAgent::new(
            BaselineRules::engineering_placeholder(),
            rankings.clone(),
            seats,
            config.master_seed,
        ),
        folds: 0,
        raises: 0,
        total: 0,
    };
    run_session(&config, &mut tally, |_| {});
    tally
}

fn with(params: &[(&str, u32)]) -> BotConfig {
    let mut config = BotConfig::defaults("測試");
    for &(key, value) in params {
        config
            .set_persona(key, poker_engine::bot::ParamValue::Myriad(value))
            .expect("參數應合法");
    }
    config
}

/// 棄牌紀律必須改變混合格的棄牌權重。
///
/// 這條刻意在**分佈層**驗而不是統計整場的棄牌率。原因是目前的基準內容
/// 絕大多數格子是純策略（fold 權重就是 10000），倍率乘上去再正規化還是
/// 100%——參數在那些格子上本來就不該有作用。整場棄牌率因此被純策略的
/// 格子稀釋到看不出差異，用它當斷言只會驗到雜訊。
#[test]
fn 棄牌紀律改變混合格的棄牌權重() {
    use poker_engine::bot::pipeline;
    use poker_engine::strategy::baseline::distribution_for;
    use poker_engine::strategy::{HandClass, PreflopNode, PreflopScenario};

    let rules = BaselineRules::engineering_placeholder();
    let rankings = BotAgent::rankings(FAST_SAMPLES);
    let ranking = &rankings[&2];
    let node = PreflopNode {
        seated: 9,
        hero: PositionLabel::Btn,
        bucket: StackBucket::Deep,
        scenario: PreflopScenario::Unopened,
    };

    // 找一格基準本身就是混合的——參數只在這種格子上有作用
    let mixed = HandClass::all()
        .into_iter()
        .filter_map(|class| {
            let distribution = distribution_for(&node, class, &rules, ranking).ok()?;
            (distribution.entries().len() > 1).then_some((class, distribution))
        })
        .next()
        .expect("基準內容應至少有一個混合格");

    let (class, baseline) = mixed;
    let is_legal = |_: Action| true;

    let weight_with = |discipline: u32| {
        let mut config = BotConfig::defaults("測試");
        config
            .set_persona(
                "foldDiscipline",
                poker_engine::bot::ParamValue::Myriad(discipline),
            )
            .expect("參數應合法");
        let trace = pipeline::run(&baseline, &config, is_legal, 0).expect("管線應成功");
        trace
            .stages
            .iter()
            .last()
            .expect("至少一階段")
            .1
            .weight_of(Action::Fold)
    };

    let loose = weight_with(6_000);
    let tight = weight_with(15_000);

    assert!(
        tight > loose,
        "{class:?}：棄牌紀律 150% 的棄牌權重 {tight} 應高於 60% 的 {loose}"
    );
}

/// 提高翻前侵略性必須讓加注變多。
#[test]
fn 提高翻前侵略性會提高加注率() {
    let rankings = BotAgent::rankings(FAST_SAMPLES);

    let passive = play(vec![with(&[("preflopAggression", 6_000)]); 9], &rankings);
    let aggressive = play(vec![with(&[("preflopAggression", 15_000)]); 9], &rankings);

    let passive_rate = passive.raise_rate();
    let aggressive_rate = aggressive.raise_rate();

    assert!(
        aggressive_rate > passive_rate,
        "侵略性 150% 的加注率 {aggressive_rate:.3} 應高於 60% 的 {passive_rate:.3}"
    );
}

/// 逐座設定必須逐座生效——全桌同一組人格的話，面板 B 的座位指派沒有意義。
#[test]
fn 不同座位可以用不同設定() {
    let rankings = BotAgent::rankings(FAST_SAMPLES);

    let mut mixed = vec![with(&[("foldDiscipline", 15_000)]); 9];
    mixed[0] = with(&[("foldDiscipline", 6_000)]);

    let uniform = play(vec![with(&[("foldDiscipline", 15_000)]); 9], &rankings);
    let varied = play(mixed, &rankings);

    assert_ne!(
        uniform.folds, varied.folds,
        "只改一個座位的設定就應該改變整體行動組成"
    );
}

/// 翻後沒有內容表，一律走 fallback：能過牌就過牌，不自行編造策略。
#[test]
fn 翻後一律走_fallback_不自行編造策略() {
    let rankings = BotAgent::rankings(FAST_SAMPLES);
    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        rankings,
        vec![BotConfig::defaults("測試"); 9],
        1,
    );

    let mut view = view_with(Vec::new(), 3);
    view.street = Street::Flop;
    view.board = vec![
        Card::new(Rank::Two, Suit::Clubs),
        Card::new(Rank::Seven, Suit::Hearts),
        Card::new(Rank::Nine, Suit::Diamonds),
    ];
    view.legal.can_check = true;
    assert_eq!(agent.choose(&view), Action::Check, "能過牌就過牌");

    view.legal.can_check = false;
    assert_eq!(agent.choose(&view), Action::Fold, "面對下注就棄牌");
}

/// 範圍寬度必須真的改變進池範圍。
///
/// 這是人格層最重要的一個參數。做成「行動權重縮放」是行不通的——
/// 基準內容絕大多數格子是純策略，乘上倍率再正規化仍是 100%。
#[test]
fn 範圍寬度改變棄牌率() {
    let rankings = BotAgent::rankings(FAST_SAMPLES);

    let narrow = play(vec![with(&[("rangeWidth", 7_000)]); 9], &rankings);
    let wide = play(vec![with(&[("rangeWidth", 14_000)]); 9], &rankings);

    let narrow_rate = f64::from(narrow.folds) / f64::from(narrow.total.max(1));
    let wide_rate = f64::from(wide.folds) / f64::from(wide.total.max(1));

    assert!(
        narrow_rate > wide_rate + 0.01,
        "範圍 70% 的棄牌率 {narrow_rate:.3} 應明顯高於 140% 的 {wide_rate:.3}"
    );
}

/// 可以免費過牌時不得棄牌。
///
/// 棄牌永遠合法，legal mask 擋不住它；但放棄一個免費的翻牌換不到任何
/// 東西，是嚴格劣勢。內容表寫的是「這手不值得投錢」，
/// 不是「這手連免費的牌都不要看」。
#[test]
fn 可以免費過牌時不棄牌() {
    let rankings = BotAgent::rankings(FAST_SAMPLES);
    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        rankings,
        // 棄牌紀律拉到上限，逼出「傾向棄牌」的極端設定
        vec![with(&[("foldDiscipline", 15_000), ("rangeWidth", 5_000)]); 9],
        7,
    );

    // 最爛的起手牌之一，且沒有人加注：內容表會說棄牌，但這裡棄牌是白送
    let mut view = view_with(Vec::new(), 4);
    view.position = PositionLabel::Bb;
    view.hole_cards = [
        Card::new(Rank::Seven, Suit::Clubs),
        Card::new(Rank::Two, Suit::Diamonds),
    ];
    view.legal.can_check = true;
    view.legal.call_to = None;
    view.to_call = Chips::ZERO;

    for _ in 0..50 {
        assert_ne!(
            agent.choose(&view),
            Action::Fold,
            "免費過牌的情境下不得棄牌"
        );
    }
}
