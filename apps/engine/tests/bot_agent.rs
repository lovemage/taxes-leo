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
        big_blind: Chips::new(2),
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
    let raised = matches!(action, Action::RaiseTo(_));
    let committed_to = match action {
        Action::RaiseTo(to) => to,
        _ => Chips::new(2),
    };
    PublicAction {
        street: Street::Preflop,
        seat,
        position,
        action,
        raised,
        committed_to,
    }
}

/// 籌碼不足的全下——沒有推高注額，實質是部分跟注。
fn short_all_in(seat: usize, position: PositionLabel, committed_to: Chips) -> PublicAction {
    PublicAction {
        street: Street::Preflop,
        seat,
        position,
        action: Action::AllIn,
        raised: false,
        committed_to,
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
            let distribution =
                distribution_for(&node, class, &rules, ranking, Chips::new(2)).ok()?;
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

// ── review 指出的缺口 ───────────────────────────────────────────────────

/// 加注金額必須是**真的籌碼**，不是 centi-BB。
///
/// 內容表寫 250 意思是 2.5BB。直接當成 250 個籌碼單位的話，1/2 桌的
/// 「開牌」會變成 125BB——每一手都等同全下，統計毫無意義。
#[test]
fn 開牌尺度為兩點五倍大盲而非兩百五十個籌碼() {
    use poker_engine::strategy::baseline::distribution_for;
    use poker_engine::strategy::{HandClass, PreflopNode, PreflopScenario};

    let rules = BaselineRules::engineering_placeholder();
    let rankings = BotAgent::rankings(FAST_SAMPLES);
    let node = PreflopNode {
        seated: 9,
        hero: PositionLabel::Btn,
        bucket: StackBucket::Deep,
        scenario: PreflopScenario::Unopened,
    };

    for (big_blind, expected) in [(2u64, 5u64), (4, 10), (10, 25)] {
        let distribution = distribution_for(
            &node,
            HandClass::from_cards(
                Card::new(Rank::Ace, Suit::Spades),
                Card::new(Rank::Ace, Suit::Hearts),
            ),
            &rules,
            &rankings[&2],
            Chips::new(big_blind),
        )
        .expect("可產生");

        let raise = distribution
            .entries()
            .iter()
            .find_map(|&(action, _)| match action {
                Action::RaiseTo(to) => Some(to.units()),
                _ => None,
            })
            .expect("AA 在 BTN 開牌必有加注");

        assert_eq!(
            raise, expected,
            "大盲 {big_blind} 時 2.5BB 應為 {expected} 個籌碼單位，實得 {raise}"
        );
    }
}

/// `expected_opponents` 要得到的每一種對手數都必須有排序可用。
///
/// 少算的話 `preflop_baseline` 會取不到排序而整個節點退回 fallback，
/// 多人跛入的範圍就完全不見了。
#[test]
fn 排序涵蓋全部可能的預期對手數() {
    use poker_engine::bot::agent::MAX_EXPECTED_OPPONENTS;
    use poker_engine::strategy::baseline::expected_opponents;
    use poker_engine::strategy::{PreflopNode, PreflopScenario};

    let rankings = BotAgent::rankings(500);

    let scenarios = [
        PreflopScenario::Unopened,
        PreflopScenario::VsLimp { limpers: 1 },
        PreflopScenario::VsLimp { limpers: 2 },
        PreflopScenario::VsLimp { limpers: 3 },
        PreflopScenario::VsLimp { limpers: 7 },
        PreflopScenario::VsOpen {
            opener: PositionLabel::Utg,
        },
        PreflopScenario::VsThreeBet {
            by: PositionLabel::Bb,
        },
        PreflopScenario::VsFourBet {
            by: PositionLabel::Utg,
        },
        PreflopScenario::VsSqueeze {
            by: PositionLabel::Bb,
        },
    ];

    for scenario in scenarios {
        let node = PreflopNode {
            seated: 9,
            hero: PositionLabel::Btn,
            bucket: StackBucket::Deep,
            scenario,
        };
        let needed = expected_opponents(&node);
        assert!(
            needed <= MAX_EXPECTED_OPPONENTS,
            "{scenario:?} 要 {needed} 名對手，超過常數宣告的上界"
        );
        assert!(
            rankings.contains_key(&needed),
            "{scenario:?} 要 {needed} 名對手的排序，但沒有建立"
        );
    }
}

/// 籌碼不足的全下只是部分跟注，不得算成一次加注。
///
/// 算成加注的話，「有人開牌 ＋ 短碼全下跟」會被後手看成 3-bet，
/// 套用完全不同（而且錯誤）的範圍。
#[test]
fn 短碼全下跟注不算加注() {
    use poker_engine::strategy::PreflopScenario;

    let hero = 8;
    let history = vec![
        acted(2, PositionLabel::Utg1, Action::RaiseTo(Chips::new(6))),
        // 這名玩家只剩 4 個籌碼，全下也追不上 6，注額沒被推高
        short_all_in(3, PositionLabel::Utg2, Chips::new(4)),
    ];

    assert_eq!(
        scenario_of(&view_with(history, hero)),
        PreflopScenario::VsOpen {
            opener: PositionLabel::Utg1
        },
        "只有一次真正的加注，後手面對的仍是開牌而不是 3-bet"
    );
}

/// 推高注額的全下才是加注。
#[test]
fn 推高注額的全下算加注() {
    use poker_engine::strategy::PreflopScenario;

    let hero = 8;
    let mut shove = short_all_in(3, PositionLabel::Utg2, Chips::new(200));
    shove.raised = true;
    let history = vec![
        acted(2, PositionLabel::Utg1, Action::RaiseTo(Chips::new(6))),
        shove,
    ];

    assert_eq!(
        scenario_of(&view_with(history, hero)),
        PreflopScenario::VsThreeBet {
            by: PositionLabel::Utg2
        }
    );
}

/// 標成未實作的參數必須**真的沒有作用**。
///
/// 這條守的是旗標與實作之間的漂移。若哪天某個欄位接上了決策路徑卻忘了
/// 把旗標翻成 true，UI 會繼續把它畫成停用，使用者調不到一個其實有用的
/// 參數；反過來旗標亂標 true，使用者會拉一個沒作用的滑桿。
#[test]
fn 未實作的參數不影響決策() {
    use poker_engine::bot::params::{BEHAVIOR_SPECS, PERSONA_SPECS};

    let rankings = BotAgent::rankings(FAST_SAMPLES);
    let reference = play(vec![BotConfig::defaults("基準"); 9], &rankings);

    for spec in PERSONA_SPECS.iter().chain(BEHAVIOR_SPECS.iter()) {
        if spec.implemented {
            continue;
        }
        for extreme in [spec.min, spec.max] {
            let mut config = BotConfig::defaults("極端");
            let value = match spec.default {
                poker_engine::bot::ParamValue::Myriad(_) => {
                    poker_engine::bot::ParamValue::Myriad(extreme)
                }
                poker_engine::bot::ParamValue::Count(_) => {
                    poker_engine::bot::ParamValue::Count(extreme)
                }
                poker_engine::bot::ParamValue::Enum(_) => poker_engine::bot::ParamValue::Enum(
                    u8::try_from(extreme).expect("列舉索引小於 256"),
                ),
                poker_engine::bot::ParamValue::Flag(_) => {
                    poker_engine::bot::ParamValue::Flag(extreme != 0)
                }
            };
            let applied = config
                .set_persona(spec.key, value)
                .or_else(|_| config.set_behavior(spec.key, value));
            applied.expect("端點值必在合法範圍內");

            let result = play(vec![config; 9], &rankings);
            assert_eq!(
                (result.folds, result.raises, result.total),
                (reference.folds, reference.raises, reference.total),
                "{} 標成未實作，但把它設成 {extreme} 改變了決策",
                spec.key
            );
        }
    }
}

/// 標成已實作的參數必須至少在某個情境改變決策。
///
/// 與上一條相反的方向：擋掉「宣告接了但其實沒接」的情況——
/// `rangeWidth` 在修正之前正是這樣。
#[test]
fn 已實作的參數確實會改變決策() {
    use poker_engine::bot::params::{ParamValue, BEHAVIOR_SPECS, PERSONA_SPECS};

    let rankings = BotAgent::rankings(FAST_SAMPLES);

    for spec in PERSONA_SPECS.iter().chain(BEHAVIOR_SPECS.iter()) {
        if !spec.implemented {
            continue;
        }
        let outcome = |raw: u32| {
            // 底子先偏離基準：`exploitAdjustmentCapPp` 夾的是「偏移後與
            // 基準的差距」，全預設時沒有差距可夾，測不出它的作用
            let mut config = with(&[("preflopAggression", 14_000)]);
            let value = match spec.default {
                ParamValue::Myriad(_) => ParamValue::Myriad(raw),
                ParamValue::Count(_) => ParamValue::Count(raw),
                ParamValue::Enum(_) => ParamValue::Enum(u8::try_from(raw).unwrap_or(0)),
                ParamValue::Flag(_) => ParamValue::Flag(raw != 0),
            };
            config
                .set_persona(spec.key, value)
                .or_else(|_| config.set_behavior(spec.key, value))
                .expect("端點值合法");
            let result = play(vec![config; 9], &rankings);
            (result.folds, result.raises, result.total)
        };

        assert_ne!(
            outcome(spec.min),
            outcome(spec.max),
            "{} 標成已實作，但端點之間的決策完全相同",
            spec.key
        );
    }
}

// ── 逐格覆寫（面板 D 的自身策略）─────────────────────────────────────────

/// 覆寫必須**只**裝在指定座位上。
///
/// 面板 D 編的是「自身策略」。若覆寫滲到其他座位，使用者以為自己在測
/// 一組策略，實際上是把全桌九個人一起改掉了——那個 run 的統計不回答
/// 任何他想問的問題，而且沒有任何徵兆。
#[test]
fn 逐格覆寫只影響指定座位() {
    use poker_engine::strategy::cell_override::{CellOverrides, OverrideCell};
    use poker_engine::strategy::hand_class::HandClass;
    use poker_engine::strategy::preflop::{PreflopNode, PreflopScenario};

    let hero = 3;
    let other = 4;
    // `view_with` 給的是 9 人桌 BTN、Deep bucket、無人進池，手牌 AKs
    let node = PreflopNode {
        seated: 9,
        hero: PositionLabel::Btn,
        bucket: StackBucket::Deep,
        scenario: PreflopScenario::Unopened,
    };
    let ak_suited = HandClass::all()
        .into_iter()
        .find(|c| c.label() == "AKs")
        .expect("牌類存在");

    let mut overrides = CellOverrides::new();
    // 主動 0、跟注 0 ⇒ 棄牌 100%（棄牌是餘數）
    overrides.set(node, ak_suited, OverrideCell::new(0, 0).expect("合法"));

    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        BotAgent::rankings(FAST_SAMPLES),
        vec![BotConfig::defaults("測試"); 9],
        20_260_823,
    );
    agent.set_seat_overrides(hero, overrides);

    // 覆寫過的座位：AKs 在這個節點一律棄牌
    for _ in 0..30 {
        assert_eq!(
            agent.choose(&view_with(Vec::new(), hero)),
            Action::Fold,
            "覆寫說 100% 棄牌，決策就不得出現別的行動"
        );
    }

    // 沒覆寫的座位：AKs 穩穩落在 BTN 的開牌範圍內
    let mut aggressive = 0;
    for _ in 0..30 {
        if matches!(
            agent.choose(&view_with(Vec::new(), other)),
            Action::RaiseTo(_) | Action::AllIn
        ) {
            aggressive += 1;
        }
    }
    assert_eq!(
        aggressive, 30,
        "其他座位不得被英雄的覆寫連帶改掉，AKs 必須照樣開牌"
    );
}
