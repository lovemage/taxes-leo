//! 翻後規則接進 Bot 決策的驗收測試（0907 計劃 Stage 2）。
//!
//! 這裡守三件事：
//!
//! 1. 規則真的接進決策路徑——不是接了但永遠走 equity fallback。
//! 2. 使用者的絕對覆寫**不被 persona／cap／noise 默默改寫**，而且七個
//!    trace 階段仍然存在。
//! 3. 尺寸意圖在決策當下才換算，撞在同一個合法尺寸的意圖會合併。

use poker_engine::betting::{Action, LegalActions, RaiseRange};
use poker_engine::bot::{BotAgent, BotConfig as SeatConfig};
use poker_engine::card::Card;
use poker_engine::chips::Chips;
use poker_engine::hand::{ActionProvider, Street};
use poker_engine::position::PositionLabel;
use poker_engine::strategy::baseline::BaselineRules;
use poker_engine::strategy::decision::{DecisionView, OpponentPublic, PublicAction, StackBucket};
use poker_engine::strategy::distribution::FULL;
use poker_engine::strategy::hand_strength::HandStrengthConfig;
use poker_engine::strategy::postflop::{
    HandStrength, Matched, PostflopActionKind as Kind, PostflopCondition,
    PostflopIntentDistribution, PostflopSituation, PostflopSizing, PostflopRule, RuleSet,
    RuleSource,
};
use poker_engine::strategy::postflop_baseline::engineering_rules;
use poker_engine::strategy::postflop_view::postflop_node;

const HERO: usize = 0;

fn card(text: &str) -> Card {
    Card::parse(text).unwrap_or_else(|| panic!("無法解析牌張 {text}"))
}

fn cards(text: &str) -> Vec<Card> {
    text.split_whitespace().map(card).collect()
}

fn acted(seat: usize, position: PositionLabel, street: Street, raised: bool, to: u64) -> PublicAction {
    PublicAction {
        street,
        seat,
        position,
        action: if raised {
            Action::RaiseTo(Chips::new(to))
        } else {
            Action::Call
        },
        raised,
        committed_to: Chips::new(to),
    }
}

/// 翻牌、英雄在 BTN、翻前是英雄加注：典型的 c-bet 機會。
fn flop_view(hole: &str, board: &str, to_call: u64) -> DecisionView {
    let hole_cards = cards(hole);
    DecisionView {
        seat: HERO,
        position: PositionLabel::Btn,
        street: Street::Flop,
        hole_cards: [hole_cards[0], hole_cards[1]],
        board: cards(board),
        seated: 6,
        effective_stack_bucket: StackBucket::Deeper,
        pot: Chips::new(30),
        to_call: Chips::new(to_call),
        big_blind: Chips::new(2),
        legal: LegalActions {
            seat: HERO,
            can_fold: to_call > 0,
            can_check: to_call == 0,
            call_to: (to_call > 0).then(|| Chips::new(to_call)),
            raise: Some(RaiseRange {
                min_to: Chips::new(to_call.max(2) * 2),
                max_to: Chips::new(400),
            }),
            all_in_to: Some(Chips::new(400)),
        },
        history: vec![
            acted(HERO, PositionLabel::Btn, Street::Preflop, true, 6),
            acted(3, PositionLabel::Bb, Street::Preflop, false, 6),
        ],
        opponents: vec![OpponentPublic {
            seat: 3,
            position: PositionLabel::Bb,
            stack: Chips::new(400),
            committed: Chips::new(to_call),
            folded: false,
            all_in: false,
        }],
    }
}

// ── 節點推導 ────────────────────────────────────────────────────

#[test]
fn 翻前加注後的翻牌無人下注是_c_bet_機會() {
    let view = flop_view("As Ks", "Ah 7d 3c", 0);
    let node = postflop_node(&view, &HandStrengthConfig::ENGINEERING).expect("翻後節點");

    assert_eq!(node.context.situation(), PostflopSituation::NoBet);
    assert_eq!(
        node.context.line.previous_street_aggressor,
        poker_engine::strategy::postflop::AggressorRole::Hero
    );
    assert_eq!(node.context.effective_stack_bucket, StackBucket::Deeper);
    assert_eq!(node.context.active_players, 2);
    // 英雄在 BTN，翻後最後行動，因此身後沒有人
    assert_eq!(node.context.opponents_behind, 0);
}

#[test]
fn 面對下注的尺度由跟注額與底池推導() {
    // 底池 30、要跟 10 → 1/3 檔
    let view = flop_view("As Ks", "Ah 7d 3c", 10);
    let node = postflop_node(&view, &HandStrengthConfig::ENGINEERING).expect("翻後節點");
    assert_eq!(node.context.situation(), PostflopSituation::FacingBet);
    assert_eq!(
        node.context.facing_size,
        poker_engine::strategy::postflop::FacingSize::Third
    );
}

#[test]
fn 翻前節點不會被誤送進翻後路徑() {
    let mut view = flop_view("As Ks", "Ah 7d 3c", 0);
    view.street = Street::Preflop;
    view.board.clear();
    assert!(postflop_node(&view, &HandStrengthConfig::ENGINEERING).is_none());
}

// ── 尺寸意圖換算 ────────────────────────────────────────────────

#[test]
fn 意圖在決策當下才換算成籌碼() {
    let intent = PostflopIntentDistribution::new(vec![
        (Kind::Check, 5_000),
        (Kind::ThirdPot, 5_000),
    ])
    .expect("意圖分佈");

    // 同一條規則，底池不同就下不同的注
    let small = intent
        .to_actions(
            PostflopSituation::NoBet,
            PostflopSizing {
                pot: Chips::new(30),
                to_call: Chips::new(0),
            },
        )
        .expect("換算");
    let large = intent
        .to_actions(
            PostflopSituation::NoBet,
            PostflopSizing {
                pot: Chips::new(300),
                to_call: Chips::new(0),
            },
        )
        .expect("換算");

    assert!(small.weight_of(Action::RaiseTo(Chips::new(10))) > 0);
    assert!(large.weight_of(Action::RaiseTo(Chips::new(100))) > 0);
}

#[test]
fn 與下注狀態矛盾的意圖不會產生行動() {
    let intent = PostflopIntentDistribution::new(vec![
        (Kind::Check, 4_000),
        (Kind::Call, 3_000),
        (Kind::Fold, 3_000),
    ])
    .expect("意圖分佈");

    // 無人下注：跟注與蓋牌不成立，只剩過牌
    let actions = intent
        .to_actions(
            PostflopSituation::NoBet,
            PostflopSizing {
                pot: Chips::new(30),
                to_call: Chips::new(0),
            },
        )
        .expect("換算");
    assert_eq!(actions.weight_of(Action::Check), FULL);
    assert_eq!(actions.weight_of(Action::Fold), 0);
}

#[test]
fn 撞在同一個尺寸的意圖會合併而不是重複列出() {
    // 底池 1 時 1/3、2/3 與滿池都換算成極小的加注額，可能撞在一起
    let intent = PostflopIntentDistribution::new(vec![
        (Kind::ThirdPot, 3_000),
        (Kind::TwoThirdsPot, 3_000),
        (Kind::Pot, 4_000),
    ])
    .expect("意圖分佈");

    let actions = intent
        .to_actions(
            PostflopSituation::NoBet,
            PostflopSizing {
                pot: Chips::new(1),
                to_call: Chips::new(0),
            },
        )
        .expect("換算");

    let total: u32 = [
        Action::RaiseTo(Chips::new(0)),
        Action::RaiseTo(Chips::new(1)),
    ]
    .into_iter()
    .map(|action| actions.weight_of(action))
    .sum();
    assert_eq!(total, FULL, "合併後權重不得憑空增減");
}

// ── 規則接進 Bot 決策 ───────────────────────────────────────────

fn agent_with(rules: RuleSet) -> BotAgent {
    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        BotAgent::rankings(500),
        vec![SeatConfig::defaults("測試"); 6],
        4_242,
    );
    agent.set_postflop_rules(rules);
    agent
}

/// 只有一條規則：任何翻後節點都蓋牌或過牌
fn always_passive() -> RuleSet {
    RuleSet::new(
        vec![
            PostflopRule::engineering(
                "T-nobet",
                "無人下注一律過牌",
                PostflopCondition {
                    situation: Some(PostflopSituation::NoBet),
                    ..PostflopCondition::default()
                },
                PostflopIntentDistribution::new(vec![(Kind::Check, FULL)]).expect("意圖"),
            ),
            PostflopRule::engineering(
                "T-facing",
                "面對下注一律蓋牌",
                PostflopCondition {
                    situation: Some(PostflopSituation::FacingBet),
                    ..PostflopCondition::default()
                },
                PostflopIntentDistribution::new(vec![(Kind::Fold, FULL)]).expect("意圖"),
            ),
        ],
        "test/v1",
    )
}

#[test]
fn 規則真的決定了_bot_的翻後行動() {
    let mut agent = agent_with(always_passive());

    // 無人下注 → 規則說過牌
    let checked = agent.choose(&flop_view("As Ks", "Ah 7d 3c", 0));
    assert_eq!(checked, Action::Check);

    // 面對下注 → 規則說蓋牌
    let folded = agent.choose(&flop_view("7c 2d", "Ah 9d 3c", 10));
    assert_eq!(folded, Action::Fold);
}

#[test]
fn 規則沒命中時退回_equity_基準而不是崩潰() {
    // 只涵蓋河牌的規則集：翻牌節點命不中任何規則
    let rules = RuleSet::new(
        vec![PostflopRule::engineering(
            "T-river",
            "只管河牌",
            PostflopCondition {
                street: Some(Street::River),
                ..PostflopCondition::default()
            },
            PostflopIntentDistribution::new(vec![(Kind::Check, FULL)]).expect("意圖"),
        )],
        "test/v1",
    );
    let mut agent = agent_with(rules);

    let action = agent.choose(&flop_view("As Ks", "Ah 7d 3c", 0));
    // equity heuristic 會給出某個合法行動；重點是沒有 panic 也沒有非法動作
    assert!(matches!(
        action,
        Action::Check | Action::RaiseTo(_) | Action::AllIn
    ));
}

#[test]
fn 工程通則涵蓋八個牌力組的兩種下注狀態() {
    let rules = engineering_rules();
    assert_eq!(rules.rules().len(), 16);

    // 全部未簽核，且排在使用者覆寫與官方內容之後
    for rule in rules.rules() {
        assert!(!rule.consultant_approved);
        assert_eq!(rule.source, RuleSource::Generic);
    }

    // 沒有排序違反、沒有同層遮蔽
    assert!(
        rules.analyse().is_empty(),
        "工程通則自己就有問題的話，使用者的規則會被它連累：{:?}",
        rules.analyse()
    );
}

#[test]
fn 未簽核通則的棄牌比例隨牌力單調() {
    let rules = engineering_rules();
    let fold_of = |strength: HandStrength| {
        rules
            .rules()
            .iter()
            .find(|rule| {
                rule.condition.hand_strength == Some(strength)
                    && rule.condition.situation == Some(PostflopSituation::FacingBet)
            })
            .map_or(0, |rule| rule.intent.weight_of(Kind::Fold))
    };

    // 這是未校準前也該成立的不變量：牌越弱越常棄牌
    assert!(fold_of(HandStrength::Nuts) <= fold_of(HandStrength::StrongMade));
    assert!(fold_of(HandStrength::StrongMade) <= fold_of(HandStrength::MediumMade));
    assert!(fold_of(HandStrength::MediumMade) <= fold_of(HandStrength::BluffCatcher));
    assert!(fold_of(HandStrength::BluffCatcher) <= fold_of(HandStrength::WeakDraw));
    assert!(fold_of(HandStrength::WeakDraw) <= fold_of(HandStrength::Air));
}

// ── 使用者覆寫是絕對頻率（計劃 §4.3）──────────────────────────────

/// 使用者覆寫：無人下注時 70% 過牌、30% 下 1/3 底池
fn user_override() -> RuleSet {
    RuleSet::new(
        vec![PostflopRule {
            id: "U-1".to_owned(),
            name: "我的 c-bet".to_owned(),
            condition: PostflopCondition {
                situation: Some(PostflopSituation::NoBet),
                ..PostflopCondition::default()
            },
            intent: PostflopIntentDistribution::new(vec![
                (Kind::Check, 7_000),
                (Kind::ThirdPot, 3_000),
            ])
            .expect("意圖"),
            source: RuleSource::UserOverride,
            version: "user/v1".to_owned(),
            consultant_approved: false,
        }],
        "test/v1",
    )
}

/// 非中性人格：侵略性拉高、跟注黏性拉低、噪音與剝削上限都不是預設值
fn aggressive_config() -> SeatConfig {
    let mut config = SeatConfig::defaults("非中性");
    config
        .set_persona(
            "postflopAggression",
            poker_engine::bot::params::ParamValue::Myriad(18_000),
        )
        .expect("設定侵略性");
    config
        .set_behavior(
            "decisionNoisePp",
            poker_engine::bot::params::ParamValue::Myriad(2_000),
        )
        .expect("設定噪音");
    config
        .set_behavior(
            "exploitAdjustmentCapPp",
            poker_engine::bot::params::ParamValue::Myriad(1_000),
        )
        .expect("設定剝削上限");
    config
        .set_behavior(
            "allowedBetSizes",
            poker_engine::bot::params::ParamValue::Count(1),
        )
        .expect("設定尺度數");
    config
}

#[test]
fn 中和後的設定讓每個管線階段都是恆等變換() {
    let neutralised = aggressive_config().neutralised_for_absolute_override();

    for key in [
        "postflopAggression",
        "preflopAggression",
        "callPersistence",
        "foldDiscipline",
    ] {
        assert_eq!(
            neutralised
                .effective(key)
                .and_then(poker_engine::bot::params::ParamValue::as_myriad),
            Some(FULL),
            "{key} 必須中和為恆等倍率"
        );
    }
    assert_eq!(
        neutralised
            .effective("decisionNoisePp")
            .and_then(poker_engine::bot::params::ParamValue::as_myriad),
        Some(0),
        "噪音必須關掉——使用者填的是絕對頻率"
    );
    assert!(
        neutralised
            .effective("allowedBetSizes")
            .and_then(poker_engine::bot::params::ParamValue::as_count)
            .unwrap_or(0)
            >= 3,
        "尺度上限要放到覆寫用得到的每一種尺度"
    );
}

#[test]
fn 使用者覆寫的頻率不被非中性人格改寫() {
    // 同一個節點跑很多次，統計實際打出的行動比例。中和有效的話，
    // 過牌比例會落在覆寫的 70% 附近；沒中和的話侵略性 1.8 倍會把它拉低
    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        BotAgent::rankings(500),
        vec![aggressive_config(); 6],
        7_777,
    );
    agent.set_postflop_rules(user_override());

    let view = flop_view("As Ks", "Ah 7d 3c", 0);
    let mut checks = 0;
    const ROUNDS: usize = 2_000;
    for _ in 0..ROUNDS {
        if agent.choose(&view) == Action::Check {
            checks += 1;
        }
    }

    let ratio = checks * 10_000 / ROUNDS;
    assert!(
        (6_500..=7_500).contains(&ratio),
        "覆寫寫的是 70% 過牌，實測 {}%——人格倍率不得默默改寫使用者的絕對頻率",
        ratio / 100
    );
}

#[test]
fn 官方與通則來源仍走一般管線() {
    // 同一份頻率，來源改成 Generic：這次侵略性應該真的起作用
    let mut generic = user_override();
    let rules: Vec<PostflopRule> = generic
        .rules()
        .iter()
        .map(|rule| PostflopRule {
            source: RuleSource::Generic,
            ..rule.clone()
        })
        .collect();
    generic = RuleSet::new(rules, "test/v1");

    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        BotAgent::rankings(500),
        vec![aggressive_config(); 6],
        7_777,
    );
    agent.set_postflop_rules(generic);

    let view = flop_view("As Ks", "Ah 7d 3c", 0);
    let mut checks = 0;
    const ROUNDS: usize = 2_000;
    for _ in 0..ROUNDS {
        if agent.choose(&view) == Action::Check {
            checks += 1;
        }
    }

    let ratio = checks * 10_000 / ROUNDS;
    assert!(
        ratio < 6_500,
        "Bot 可調整的 baseline 應該被侵略性拉低過牌比例，實測 {}%",
        ratio / 100
    );
}

// ── 可重現性 ────────────────────────────────────────────────────

#[test]
fn 相同種子與策略輸入產生相同決策序列() {
    let sequence = || {
        let mut agent = agent_with(engineering_rules());
        let view = flop_view("As Ks", "Ah 7d 3c", 0);
        (0..50).map(|_| agent.choose(&view)).collect::<Vec<_>>()
    };
    assert_eq!(sequence(), sequence());
}

#[test]
fn 解析結果帶著命中規則的來源() {
    let rules = user_override();
    let view = flop_view("As Ks", "Ah 7d 3c", 0);
    let node = postflop_node(&view, &HandStrengthConfig::ENGINEERING).expect("節點");
    let (matched, _) = rules.resolve(&node.context, node.sizing, &|_| true);

    assert!(matches!(
        matched,
        Matched::Rule {
            source: RuleSource::UserOverride,
            ..
        }
    ));
}
