//! Postflop 規則清單的驗收測試。
//!
//! 對應核心規格 4.2 與 UI 規格 D.5。重點在三種規則病狀的偵測——
//! 被遮蔽的規則不會報錯、只會靜默失效，因此必須靠工具找出來。

use poker_engine::betting::Action;
use poker_engine::card::Card;
use poker_engine::chips::Chips;
use poker_engine::hand::Street;
use poker_engine::position::PositionLabel;
use poker_engine::strategy::distribution::{ActionDistribution, FULL};
use poker_engine::strategy::decision::StackBucket;
use poker_engine::strategy::postflop::{
    classify_board, classify_line, AggressorRole, BoardConnectivity, BoardSurface, CoverageStats,
    FacingSize, HandStrength, Matched, PostflopActionKind, PostflopCondition, PostflopContext,
    PostflopLine, PostflopLineCondition, PostflopLineName, PostflopNode, PostflopRule,
    PostflopSituation, PotType, RelativeAggressorOrder, RuleIssue, RuleSet, RuleSource,
};
use poker_engine::strategy::postflop::{enumerate_postflop_nodes, node_count_report};

fn cards(values: &[&str]) -> Vec<Card> {
    values
        .iter()
        .map(|value| Card::parse(value).expect("合法測試牌"))
        .collect()
}

fn bet(units: u64) -> Action {
    Action::RaiseTo(Chips::new(units))
}

fn dist(entries: Vec<(Action, u32)>) -> ActionDistribution {
    ActionDistribution::new(entries).expect("建立分佈")
}

fn rule(name: &str, condition: PostflopCondition) -> PostflopRule {
    PostflopRule::engineering(
        format!("R-{name}"),
        name,
        condition,
        dist(vec![(Action::Check, 6_000), (bet(10), 4_000)]),
    )
}

/// 指定來源層的規則，供來源排序與同層診斷測試使用
fn rule_from(source: RuleSource, name: &str, condition: PostflopCondition) -> PostflopRule {
    PostflopRule {
        source,
        ..rule(name, condition)
    }
}

fn context() -> PostflopContext {
    PostflopContext {
        street: Street::Flop,
        board_textures: classify_board(&cards(&["As", "7d", "2c"])).expect("翻牌面"),
        hand_strength: HandStrength::StrongMade,
        active_players: 3,
        hero_position: PositionLabel::Btn,
        opponents_behind: 0,
        pot_type: PotType::SingleRaised,
        facing_size: FacingSize::None,
        spr_centi: 400,
        effective_stack_bucket: StackBucket::Deeper,
        line: hero_cbet_line(),
    }
}

/// 英雄翻前加注、翻牌無人下注：最典型的 c-bet 機會。
fn hero_cbet_line() -> PostflopLine {
    PostflopLine::new(
        AggressorRole::Hero,
        AggressorRole::Hero,
        RelativeAggressorOrder::NotApplicable,
        false,
        0,
        0,
    )
}

#[test]
fn 六種外觀與乾濕結構是各自獨立的兩軸() {
    let cases = [
        (["As", "7s", "2s"], BoardSurface::Flush),
        (["As", "7s", "2d"], BoardSurface::FlushDraw),
        (["As", "7d", "2c"], BoardSurface::Rainbow),
        (["As", "Ad", "7c"], BoardSurface::RainbowPaired),
        (["As", "Ad", "7s"], BoardSurface::FlushDrawPaired),
        (["As", "Ad", "Ac"], BoardSurface::Trips),
    ];
    for (board, expected) in cases {
        let textures = classify_board(&cards(&board)).expect("翻牌面");
        assert_eq!(textures.surface(), expected, "牌面 {board:?}");
    }

    let dry = classify_board(&cards(&["As", "7d", "2c"])).expect("乾面");
    assert_eq!(dry.connectivity(), BoardConnectivity::Dry);
    let wet = classify_board(&cards(&["9s", "8d", "7c"])).expect("濕面");
    assert_eq!(wet.connectivity(), BoardConnectivity::Wet);
    // 兩軸互不干涉：同樣是彩虹面，乾濕可以不同
    assert_eq!(wet.surface(), BoardSurface::Rainbow);
}

#[test]
fn 彩虹乾燥與彩虹濕潤可設定不同策略() {
    let dry = context();
    // 只指定一軸是廣域條件
    assert!(PostflopCondition {
        board_surface: Some(BoardSurface::Rainbow),
        ..Default::default()
    }
    .matches(&dry));
    assert!(PostflopCondition {
        board_connectivity: Some(BoardConnectivity::Dry),
        ..Default::default()
    }
    .matches(&dry));

    // 同時指定兩軸是精確匹配
    let rainbow_dry = PostflopCondition {
        board_surface: Some(BoardSurface::Rainbow),
        board_connectivity: Some(BoardConnectivity::Dry),
        ..Default::default()
    };
    let rainbow_wet = PostflopCondition {
        board_surface: Some(BoardSurface::Rainbow),
        board_connectivity: Some(BoardConnectivity::Wet),
        ..Default::default()
    };
    assert!(rainbow_dry.matches(&dry));
    assert!(!rainbow_wet.matches(&dry), "驗收條件 6：兩者必須是不同節點");
    assert!(
        !rainbow_dry.intersects(&rainbow_wet),
        "彩虹乾燥與彩虹濕潤沒有交集，不該互相產生重疊警告"
    );

    // 單軸條件涵蓋同軸的精確條件
    let rainbow_any = PostflopCondition {
        board_surface: Some(BoardSurface::Rainbow),
        ..Default::default()
    };
    assert!(rainbow_any.contains(&rainbow_dry));
    assert!(!rainbow_dry.contains(&rainbow_any));
    assert!(rainbow_any.intersects(&rainbow_wet));
}

#[test]
fn 無人下注與面對下注的動作合法性符合牌局規則() {
    assert!(PostflopActionKind::Check.is_available(PostflopSituation::NoBet));
    assert!(!PostflopActionKind::Call.is_available(PostflopSituation::NoBet));
    assert!(!PostflopActionKind::Fold.is_available(PostflopSituation::NoBet));
    assert!(!PostflopActionKind::Check.is_available(PostflopSituation::FacingBet));
    assert!(PostflopActionKind::Call.is_available(PostflopSituation::FacingBet));
    assert!(PostflopActionKind::Fold.is_available(PostflopSituation::FacingBet));
    for action in [
        PostflopActionKind::ThirdPot,
        PostflopActionKind::TwoThirdsPot,
        PostflopActionKind::Pot,
    ] {
        assert!(action.is_available(PostflopSituation::NoBet));
        assert!(action.is_available(PostflopSituation::FacingBet));
    }
}

fn all_legal(_: Action) -> bool {
    true
}

// ── 條件比對 ────────────────────────────────────────────────────────────

#[test]
fn 萬用條件命中任何節點() {
    let condition = PostflopCondition::default();
    assert!(condition.matches(&context()));
}

#[test]
fn 指定欄位不符即不命中() {
    let condition = PostflopCondition {
        street: Some(Street::River),
        ..Default::default()
    };
    assert!(!condition.matches(&context()), "街別不符不應命中");
}

#[test]
fn 範圍條件依區間判定() {
    let condition = PostflopCondition {
        active_players: Some(2..=3),
        ..Default::default()
    };
    assert!(condition.matches(&context()));

    let condition = PostflopCondition {
        active_players: Some(4..=9),
        ..Default::default()
    };
    assert!(!condition.matches(&context()));
}

/// 核心規格 4.2：「多人底池不得只用單一 IP／OOP 表達位置。」
#[test]
fn 多人位置以身後對手數表達而非_ip_oop() {
    let heads_up_in_position = PostflopContext {
        active_players: 2,
        opponents_behind: 0,
        ..context()
    };
    let multiway_middle = PostflopContext {
        active_players: 4,
        opponents_behind: 2,
        ..context()
    };

    // 兩者在 IP/OOP 的二分法下都可能被歸為「有位置」，但實際差很多
    let condition = PostflopCondition {
        opponents_behind: Some(0..=0),
        ..Default::default()
    };
    assert!(condition.matches(&heads_up_in_position));
    assert!(
        !condition.matches(&multiway_middle),
        "身後還有兩個對手的節點不得與真正的最後行動位混為一談"
    );
}

// ── 第一條命中 ──────────────────────────────────────────────────────────

#[test]
fn 依優先序取第一條命中的規則() {
    let set = RuleSet::new(
        vec![
            rule(
                "河牌專用",
                PostflopCondition {
                    street: Some(Street::River),
                    ..Default::default()
                },
            ),
            rule(
                "翻牌乾面",
                PostflopCondition {
                    street: Some(Street::Flop),
                    board_connectivity: Some(BoardConnectivity::Dry),
                    ..Default::default()
                },
            ),
            rule("通則", PostflopCondition::default()),
        ],
        "baseline-v1",
    );

    let (matched, distribution) = set.resolve(&context(), &all_legal);
    assert_eq!(
        matched,
        Matched::Rule {
            index: 1,
            source: RuleSource::EngineeringFallback
        },
        "應命中第二條而非通則"
    );
    assert!(distribution.is_some());
}

#[test]
fn 無規則命中時走_fallback() {
    let set = RuleSet::new(
        vec![rule(
            "只管河牌",
            PostflopCondition {
                street: Some(Street::River),
                ..Default::default()
            },
        )],
        "baseline-v1",
    );

    let (matched, distribution) = set.resolve(&context(), &all_legal);
    assert!(
        matches!(matched, Matched::Fallback(_)),
        "沒有規則命中必須走 fallback"
    );
    assert!(distribution.is_none());
}

/// 核心規格 4.2：合法行動遮蔽後權重為 0 時必須進入 fallback，
/// 不得除以 0 或任選行動。
#[test]
fn 遮蔽後權重歸零時走_fallback_而非任選() {
    let set = RuleSet::new(
        vec![PostflopRule::engineering(
            "R-only-bet",
            "只會下注",
            PostflopCondition::default(),
            dist(vec![(bet(10), FULL)]),
        )],
        "baseline-v1",
    );

    // 下注不合法時，該規則的全部權重都被遮蔽
    let (matched, distribution) = set.resolve(&context(), &|a| !matches!(a, Action::RaiseTo(_)));
    assert!(
        matches!(matched, Matched::Fallback(_)),
        "剩餘權重為 0 必須走 fallback"
    );
    assert!(distribution.is_none(), "不得回傳任選的分佈");
}

#[test]
fn 遮蔽後仍有合法行動時重新正規化() {
    let set = RuleSet::new(
        vec![rule("通則", PostflopCondition::default())],
        "baseline-v1",
    );
    let (matched, distribution) = set.resolve(&context(), &|a| !matches!(a, Action::RaiseTo(_)));

    assert_eq!(
        matched,
        Matched::Rule {
            index: 0,
            source: RuleSource::EngineeringFallback
        }
    );
    let distribution = distribution.expect("仍有合法行動");
    assert_eq!(
        distribution.weight_of(Action::Check),
        FULL,
        "移除下注後過牌應吸收全部權重"
    );
}

// ── 規則衝突偵測（核心規格 4.2 明訂）──────────────────────────────────

#[test]
fn 被更早規則完全涵蓋者判為遮蔽() {
    let set = RuleSet::new(
        vec![
            // 通則排在前面
            rule("通則", PostflopCondition::default()),
            // 特例排在後面，永遠輪不到
            rule(
                "翻牌乾面",
                PostflopCondition {
                    street: Some(Street::Flop),
                    board_connectivity: Some(BoardConnectivity::Dry),
                    ..Default::default()
                },
            ),
        ],
        "baseline-v1",
    );

    let issues = set.analyse();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, RuleIssue::Shadowed { rule: 1, by: 0 })),
        "排在通則之後的特例必須被標為遮蔽：{issues:?}"
    );
    assert!(
        !issues.iter().any(|issue| issue.is_error()),
        "計劃 §5.3：遮蔽是 warning。幾乎一定是寫錯，但擋住保存會讓使用者連暫存都做不到"
    );
}

#[test]
fn 特例在前通則在後不算遮蔽() {
    let set = RuleSet::new(
        vec![
            rule(
                "翻牌乾面",
                PostflopCondition {
                    street: Some(Street::Flop),
                    board_connectivity: Some(BoardConnectivity::Dry),
                    ..Default::default()
                },
            ),
            rule("通則", PostflopCondition::default()),
        ],
        "baseline-v1",
    );

    let issues = set.analyse();
    assert!(
        !issues.iter().any(|i: &RuleIssue| i.is_error()),
        "先特例後通則是正常寫法，不應報 error：{issues:?}"
    );
}

/// 這裡刻意寫出顛倒的範圍，因為本測試驗的就是引擎會不會抓到它。
#[allow(clippy::reversed_empty_ranges)]
#[test]
fn 範圍顛倒的條件判為不可能成立() {
    let set = RuleSet::new(
        vec![rule(
            "人數顛倒",
            PostflopCondition {
                active_players: Some(5..=3),
                ..Default::default()
            },
        )],
        "baseline-v1",
    );

    let issues = set.analyse();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, RuleIssue::Impossible { rule: 0 })),
        "5..=3 不可能成立：{issues:?}"
    );
}

#[test]
fn 部分重疊只列為_warning() {
    let set = RuleSet::new(
        vec![
            rule(
                "翻牌任意面",
                PostflopCondition {
                    street: Some(Street::Flop),
                    ..Default::default()
                },
            ),
            rule(
                "任意街乾面",
                PostflopCondition {
                    board_connectivity: Some(BoardConnectivity::Dry),
                    ..Default::default()
                },
            ),
        ],
        "baseline-v1",
    );

    let issues = set.analyse();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, RuleIssue::Overlap { rule: 1, with: 0 })),
        "兩者在『翻牌且乾面』相交，應列為重疊：{issues:?}"
    );
    assert!(
        !issues.iter().any(|i: &RuleIssue| i.is_error()),
        "部分重疊互不涵蓋，是 warning 不是 error"
    );
}

#[test]
fn 範圍條件的涵蓋關係正確判定() {
    let wide = PostflopCondition {
        active_players: Some(2..=9),
        ..Default::default()
    };
    let narrow = PostflopCondition {
        active_players: Some(3..=4),
        ..Default::default()
    };
    assert!(wide.contains(&narrow), "2..=9 涵蓋 3..=4");
    assert!(!narrow.contains(&wide));
    assert!(wide.intersects(&narrow));

    let disjoint = PostflopCondition {
        active_players: Some(5..=6),
        ..Default::default()
    };
    assert!(!narrow.intersects(&disjoint), "3..=4 與 5..=6 不相交");
}

#[test]
fn 不相交的規則不產生任何問題() {
    let set = RuleSet::new(
        vec![
            rule(
                "翻牌",
                PostflopCondition {
                    street: Some(Street::Flop),
                    ..Default::default()
                },
            ),
            rule(
                "河牌",
                PostflopCondition {
                    street: Some(Street::River),
                    ..Default::default()
                },
            ),
        ],
        "baseline-v1",
    );
    assert!(set.analyse().is_empty(), "互不相交的規則不應有任何問題");
}

// ── 覆蓋率統計 ──────────────────────────────────────────────────────────

#[test]
fn 策略完整度只算使用者自己寫的規則() {
    let mut stats = CoverageStats::default();
    for _ in 0..7 {
        stats.record(&Matched::Rule {
            index: 0,
            source: RuleSource::UserOverride,
        });
    }
    for _ in 0..3 {
        stats.record(&Matched::Fallback(
            poker_engine::strategy::postflop::FallbackReason::NoRuleMatched,
        ));
    }

    assert_eq!(stats.total(), 10);
    assert_eq!(
        stats.completeness_myriad(),
        Some(7_000),
        "7/10 命中使用者規則 → 完整度 70%"
    );
}

#[test]
fn 工程_fallback_規則的命中不計入玩家完整度() {
    let mut stats = CoverageStats::default();
    for _ in 0..9 {
        stats.record(&Matched::Rule {
            index: 0,
            source: RuleSource::EngineeringFallback,
        });
    }
    stats.record(&Matched::Rule {
        index: 1,
        source: RuleSource::UserOverride,
    });

    assert_eq!(stats.rule_hits(), 10, "十次都命中了規則");
    assert_eq!(
        stats.completeness_myriad(),
        Some(1_000),
        "但只有一次是使用者寫的——不分層的話這個數字會是 100%，\
         而它存在的用途正是告訴使用者還有多少節點沒寫"
    );
    assert_eq!(stats.engineering_hits, 9);
    assert_eq!(stats.user_hits, 1);
}

#[test]
fn 兩種_fallback_原因分開計數() {
    use poker_engine::strategy::postflop::FallbackReason;
    let mut stats = CoverageStats::default();
    stats.record(&Matched::Fallback(FallbackReason::NoRuleMatched));
    stats.record(&Matched::Fallback(FallbackReason::AllWeightsMasked {
        rule: 2,
    }));

    assert_eq!(stats.fallback_no_rule, 1);
    assert_eq!(
        stats.fallback_masked, 1,
        "「沒規則」與「規則被遮蔽光」是不同問題，報表必須分開呈現"
    );
}

#[test]
fn 無決策節點時完整度為_na() {
    assert_eq!(CoverageStats::default().completeness_myriad(), None);
}

// ── 牌局線路（計劃 §4.1）────────────────────────────────────────

/// 對手是最近主動方時必須指定英雄的相對行動順序
fn opponent_line(
    previous: AggressorRole,
    last: AggressorRole,
    order: RelativeAggressorOrder,
    hero_checked: bool,
) -> PostflopLine {
    PostflopLine::new(previous, last, order, hero_checked, 0, 0)
}

#[test]
fn 無人下注的六條線路各自命名不共用其他桶() {
    use AggressorRole as Role;
    use RelativeAggressorOrder as Order;
    let no_bet = PostflopSituation::NoBet;

    let cases = [
        (
            opponent_line(Role::Hero, Role::Hero, Order::NotApplicable, false),
            PostflopLineName::CbetChance,
        ),
        (
            opponent_line(Role::None, Role::Hero, Order::NotApplicable, false),
            PostflopLineName::DelayedCbetChance,
        ),
        (
            opponent_line(Role::Opponent, Role::Opponent, Order::HeroBefore, false),
            PostflopLineName::DonkChance,
        ),
        (
            opponent_line(Role::Opponent, Role::Opponent, Order::HeroAfter, false),
            PostflopLineName::StabChance,
        ),
        (
            opponent_line(Role::None, Role::Opponent, Order::HeroBefore, false),
            PostflopLineName::ProbeChance,
        ),
        (
            opponent_line(Role::None, Role::Opponent, Order::HeroAfter, false),
            PostflopLineName::DelayedStabChance,
        ),
        (
            opponent_line(Role::None, Role::None, Order::NotApplicable, false),
            PostflopLineName::NoAggressorBetChance,
        ),
    ];

    for (line, expected) in cases {
        assert_eq!(
            classify_line(no_bet, line),
            Some(expected),
            "線路 {line:?} 應歸入 {}",
            expected.label()
        );
    }
}

#[test]
fn probe_與_delayed_c_bet_不會撞在同一節點() {
    use AggressorRole as Role;
    use RelativeAggressorOrder as Order;
    // 兩者的前一街都沒有主動方，差別在更早的那一街是誰主動。
    // 只看前一街的話這兩條線路會撞在一起（驗收條件 8）
    let delayed_cbet = opponent_line(Role::None, Role::Hero, Order::NotApplicable, false);
    let probe = opponent_line(Role::None, Role::Opponent, Order::HeroBefore, false);

    assert_eq!(
        classify_line(PostflopSituation::NoBet, delayed_cbet),
        Some(PostflopLineName::DelayedCbetChance)
    );
    assert_eq!(
        classify_line(PostflopSituation::NoBet, probe),
        Some(PostflopLineName::ProbeChance)
    );
}

#[test]
fn 面對下注的六條線路各自命名() {
    use AggressorRole as Role;
    use RelativeAggressorOrder as Order;
    let facing = PostflopSituation::FacingBet;

    let cases = [
        (
            opponent_line(Role::Opponent, Role::Opponent, Order::HeroAfter, false),
            PostflopLineName::FacingCbet,
        ),
        (
            opponent_line(Role::None, Role::Opponent, Order::HeroAfter, false),
            PostflopLineName::FacingDelayedCbet,
        ),
        (
            opponent_line(Role::Hero, Role::Hero, Order::NotApplicable, false),
            PostflopLineName::FacingDonk,
        ),
        (
            opponent_line(Role::Hero, Role::Hero, Order::NotApplicable, true),
            PostflopLineName::FacingStabAfterCheck,
        ),
        (
            opponent_line(Role::None, Role::Hero, Order::NotApplicable, false),
            PostflopLineName::FacingProbe,
        ),
        (
            opponent_line(Role::None, Role::None, Order::NotApplicable, false),
            PostflopLineName::FacingNoAggressorBet,
        ),
    ];

    for (line, expected) in cases {
        assert_eq!(
            classify_line(facing, line),
            Some(expected),
            "線路 {line:?} 應歸入 {}",
            expected.label()
        );
    }
}

#[test]
fn 面對_donk_與過牌後面對_stab_只差在英雄是否已過牌() {
    use AggressorRole as Role;
    use RelativeAggressorOrder as Order;
    let donk = opponent_line(Role::Hero, Role::Hero, Order::NotApplicable, false);
    let stab = opponent_line(Role::Hero, Role::Hero, Order::NotApplicable, true);

    assert_ne!(
        classify_line(PostflopSituation::FacingBet, donk),
        classify_line(PostflopSituation::FacingBet, stab),
        "英雄是否已過牌是可見的次級條件，不能併成同一個節點"
    );
}

#[test]
fn 不適用的行動順序會被正規化而不是產生兩個分支() {
    use AggressorRole as Role;
    use RelativeAggressorOrder as Order;

    // 最近主動方是英雄自己，「英雄在他之前還是之後」沒有意義
    let before = PostflopLine::new(Role::Hero, Role::Hero, Order::HeroBefore, false, 0, 0);
    let after = PostflopLine::new(Role::Hero, Role::Hero, Order::HeroAfter, false, 0, 0);
    assert_eq!(before, after, "同一個牌局狀態必須只有一個 canonical 表示");
    assert_eq!(
        before.relative_aggressor_order,
        Order::NotApplicable
    );

    // 反過來：對手是最近主動方卻沒有順序，是不自洽的值
    let degenerate = PostflopLine {
        previous_street_aggressor: Role::Opponent,
        last_aggressor_before_current_street: Role::Opponent,
        relative_aggressor_order: Order::NotApplicable,
        hero_checked_this_street: false,
        current_street_bet_count: 0,
        current_street_raise_count: 0,
    };
    assert!(!degenerate.is_coherent());
    assert_eq!(classify_line(PostflopSituation::NoBet, degenerate), None);
}

#[test]
fn 前一街主動方與跨街最近主動方必須一致() {
    use AggressorRole as Role;
    use RelativeAggressorOrder as Order;
    // 前一街是英雄主動，跨街最近主動方卻是對手：本街之前的最近主動方
    // 不可能比前一街更舊
    let contradictory = PostflopLine {
        previous_street_aggressor: Role::Hero,
        last_aggressor_before_current_street: Role::Opponent,
        relative_aggressor_order: Order::HeroAfter,
        hero_checked_this_street: false,
        current_street_bet_count: 0,
        current_street_raise_count: 0,
    };
    assert!(!contradictory.is_coherent());
    assert_eq!(classify_line(PostflopSituation::NoBet, contradictory), None);
}

#[test]
fn 線路條件用範圍表達至少加注一次() {
    let mut context = context();
    context.line = PostflopLine::new(
        AggressorRole::Opponent,
        AggressorRole::Opponent,
        RelativeAggressorOrder::HeroAfter,
        false,
        1,
        2,
    );

    let at_least_once = PostflopCondition {
        line: PostflopLineCondition {
            current_street_raise_count: Some(1..=u8::MAX),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(at_least_once.matches(&context));

    let none_yet = PostflopCondition {
        line: PostflopLineCondition {
            current_street_raise_count: Some(0..=0),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(!none_yet.matches(&context));

    // 集合關係：0..=255 涵蓋 1..=255，反之不然
    assert!(PostflopLineCondition {
        current_street_raise_count: Some(0..=u8::MAX),
        ..Default::default()
    }
    .contains(&PostflopLineCondition {
        current_street_raise_count: Some(1..=u8::MAX),
        ..Default::default()
    }));
}

#[test]
fn 有效籌碼分檔參與規則匹配() {
    let context = context();
    assert_eq!(context.effective_stack_bucket, StackBucket::Deeper);

    assert!(PostflopCondition {
        effective_stack_bucket: Some(StackBucket::Deeper),
        ..Default::default()
    }
    .matches(&context));
    assert!(!PostflopCondition {
        effective_stack_bucket: Some(StackBucket::Short),
        ..Default::default()
    }
    .matches(&context));

    // 未指定即萬用，且涵蓋任何具體分檔
    let any = PostflopCondition::default();
    assert!(any.matches(&context));
    assert!(any.contains(&PostflopCondition {
        effective_stack_bucket: Some(StackBucket::Short),
        ..Default::default()
    }));
}

// ── 來源層級與診斷（計劃 §5.3）──────────────────────────────────

#[test]
fn 使用者覆寫壓過官方通則不算重疊也不算遮蔽() {
    // 同一個條件，一條使用者覆寫、一條官方通則。這是設計本意，
    // 跨層相交報成警告的話，每寫一條覆寫就多一個假警報
    let set = RuleSet::new(
        vec![
            rule_from(
                RuleSource::UserOverride,
                "我的翻牌乾面",
                PostflopCondition {
                    street: Some(Street::Flop),
                    ..Default::default()
                },
            ),
            rule_from(
                RuleSource::Official,
                "官方翻牌",
                PostflopCondition {
                    street: Some(Street::Flop),
                    ..Default::default()
                },
            ),
        ],
        "baseline-v1",
    );

    assert!(
        set.analyse().is_empty(),
        "跨來源層的相交是設計本意：{:?}",
        set.analyse()
    );
}

#[test]
fn 同一層內的完全涵蓋仍要標為遮蔽() {
    let set = RuleSet::new(
        vec![
            rule_from(
                RuleSource::UserOverride,
                "通則",
                PostflopCondition::default(),
            ),
            rule_from(
                RuleSource::UserOverride,
                "特例",
                PostflopCondition {
                    street: Some(Street::Flop),
                    ..Default::default()
                },
            ),
        ],
        "baseline-v1",
    );

    assert!(set
        .analyse()
        .iter()
        .any(|i| matches!(i, RuleIssue::Shadowed { rule: 1, by: 0 })));
}

#[test]
fn 官方規則排到使用者覆寫之前是必須修正的錯誤() {
    let set = RuleSet::new(
        vec![
            rule_from(RuleSource::Official, "官方", PostflopCondition::default()),
            rule_from(
                RuleSource::UserOverride,
                "我的覆寫",
                PostflopCondition::default(),
            ),
        ],
        "baseline-v1",
    );

    let issues = set.analyse();
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, RuleIssue::LayerOrderViolation { rule: 1, after: 0 })),
        "順序錯了的話使用者的覆寫會被官方通則蓋掉，而畫面上完全看不出來：{issues:?}"
    );
    assert!(issues.iter().any(|issue| issue.is_error()));
}

#[test]
fn 正確的來源排序不產生排序違反() {
    let set = RuleSet::new(
        vec![
            rule_from(
                RuleSource::UserOverride,
                "我的覆寫",
                PostflopCondition {
                    street: Some(Street::Flop),
                    ..Default::default()
                },
            ),
            rule_from(
                RuleSource::Official,
                "官方",
                PostflopCondition {
                    street: Some(Street::Turn),
                    ..Default::default()
                },
            ),
            rule_from(
                RuleSource::Generic,
                "通則",
                PostflopCondition {
                    street: Some(Street::River),
                    ..Default::default()
                },
            ),
            rule_from(
                RuleSource::EngineeringFallback,
                "工程基準",
                PostflopCondition::default(),
            ),
        ],
        "baseline-v1",
    );

    assert!(!set
        .analyse()
        .iter()
        .any(|i| matches!(i, RuleIssue::LayerOrderViolation { .. })));
}

// ── 節點列舉（計劃 §4.2）────────────────────────────────────────

#[test]
fn 列舉器不輸出河牌的聽牌組別() {
    for node in enumerate_postflop_nodes() {
        if node.street == Street::River {
            assert!(
                node.hand_strength.reachable_on(Street::River),
                "河牌沒有未來補牌，{:?} 不可能成立",
                node.hand_strength
            );
        }
    }

    // 反過來：翻牌與轉牌八組齊全
    for street in [Street::Flop, Street::Turn] {
        let strengths: std::collections::BTreeSet<_> = enumerate_postflop_nodes()
            .into_iter()
            .filter(|node| node.street == street)
            .map(|node| node.hand_strength)
            .collect();
        assert_eq!(strengths.len(), 8, "{street:?} 應有八個牌力組");
    }
}

#[test]
fn 列舉器不輸出翻牌與轉牌的三條濕潤面() {
    for node in enumerate_postflop_nodes() {
        if node.surface == BoardSurface::Trips
            && node.connectivity == BoardConnectivity::Wet
        {
            assert_eq!(
                node.street,
                Street::River,
                "三張同點數的公共牌湊不出三種點數的視窗，只有河牌補到兩張才可能"
            );
        }
    }
}

#[test]
fn 列舉器的下注狀態與面對尺度一致() {
    for node in enumerate_postflop_nodes() {
        match node.situation {
            PostflopSituation::NoBet => assert_eq!(
                node.facing_size,
                FacingSize::None,
                "無人下注沒有面對尺度可談"
            ),
            PostflopSituation::FacingBet => assert_ne!(
                node.facing_size,
                FacingSize::None,
                "面對下注必然有尺度"
            ),
        }
        assert_eq!(
            node.line.situation(),
            node.situation,
            "線路名稱與下注狀態必須一致"
        );
    }
}

#[test]
fn 節點鍵唯一且不含共用的其他桶() {
    let nodes = enumerate_postflop_nodes();
    let keys: std::collections::BTreeSet<String> = nodes.iter().map(PostflopNode::key).collect();
    assert_eq!(keys.len(), nodes.len(), "canonical 節點鍵必須唯一");
    assert!(
        !keys.iter().any(|key| key.contains("other")),
        "不得建立可編輯的共用「其他」桶——一條規則吞掉多個殘餘情境就沒人說得出它在講什麼"
    );
}

#[test]
fn 線路命名與最小條件互為反向映射() {
    for situation in PostflopSituation::ALL {
        let names: &[PostflopLineName] = match situation {
            PostflopSituation::NoBet => &PostflopLineName::NO_BET,
            PostflopSituation::FacingBet => &PostflopLineName::FACING_BET,
        };
        for &name in names {
            let line = name.representative_line();
            assert_eq!(
                classify_line(situation, line),
                Some(name),
                "{} 的代表值必須歸回自己",
                name.label()
            );
            assert!(
                name.minimal_condition().matches(line),
                "{} 的最小條件必須命中自己的代表值",
                name.label()
            );
        }
    }
}

#[test]
fn 節點規模報表逐項顯示維度成長() {
    let report = node_count_report();

    assert_eq!(report.node_set_version, "postflop-nodes/v1");
    assert!(report.core > 0);
    assert!(
        report.with_facing_size > report.core,
        "面對下注的三檔尺度必須讓節點數變多"
    );
    assert_eq!(report.with_pot_type, report.with_facing_size * 3);
    assert_eq!(report.with_stack_bucket, report.with_pot_type * 9);
    assert!(
        report.with_facing_size < report.cartesian,
        "排除規則必須真的擋掉不可達組合（{} vs 笛卡兒積 {}）",
        report.with_facing_size,
        report.cartesian
    );
}

#[test]
fn 命不中任何可達節點的規則標為_unreachable() {
    let nodes = enumerate_postflop_nodes();

    // 河牌 × 強聽牌：河牌沒有未來補牌，這個組合永遠不會出現
    let unreachable = rule(
        "河牌強聽牌",
        PostflopCondition {
            street: Some(Street::River),
            hand_strength: Some(HandStrength::StrongDraw),
            ..Default::default()
        },
    );
    let reachable = rule(
        "河牌強成牌",
        PostflopCondition {
            street: Some(Street::River),
            hand_strength: Some(HandStrength::StrongMade),
            ..Default::default()
        },
    );

    let set = RuleSet::new(vec![unreachable, reachable], "baseline-v1");
    let issues = set.analyse_reachability(&nodes);
    assert_eq!(
        issues,
        vec![RuleIssue::Unreachable { rule: 0 }],
        "只有第一條不可達"
    );
    assert!(
        !issues.iter().any(|issue| issue.is_error()),
        "計劃 §5.3：unreachable 是 warning，節點集合改版時不該擋住保存"
    );
}
