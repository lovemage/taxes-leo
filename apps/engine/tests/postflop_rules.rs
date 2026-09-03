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
use poker_engine::strategy::postflop::{
    classify_board, BoardTexture, CoverageStats, FacingSize, HandStrength, Matched,
    PostflopActionKind, PostflopCondition, PostflopContext, PostflopRule, PostflopSituation,
    PotType, RuleIssue, RuleSet,
};

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
    PostflopRule {
        name: name.to_owned(),
        condition,
        actions: dist(vec![(Action::Check, 6_000), (bet(10), 4_000)]),
    }
}

fn context() -> PostflopContext {
    PostflopContext {
        street: Street::Flop,
        board_textures: classify_board(&cards(&["As", "7d", "2c"])).expect("翻牌面"),
        hand_strength: HandStrength::Value,
        active_players: 3,
        hero_position: PositionLabel::Btn,
        opponents_behind: 0,
        pot_type: PotType::SingleRaised,
        facing_size: FacingSize::None,
        spr_centi: 400,
    }
}

#[test]
fn 八種牌面分類涵蓋六種外觀與乾濕結構() {
    let cases = [
        (["As", "7s", "2s"], BoardTexture::Flush),
        (["As", "7s", "2d"], BoardTexture::FlushDraw),
        (["As", "7d", "2c"], BoardTexture::Rainbow),
        (["As", "Ad", "7c"], BoardTexture::RainbowPaired),
        (["As", "Ad", "7s"], BoardTexture::FlushDrawPaired),
        (["As", "Ad", "Ac"], BoardTexture::Trips),
    ];
    for (board, expected) in cases {
        let textures = classify_board(&cards(&board)).expect("翻牌面");
        assert_eq!(textures.surface(), expected, "牌面 {board:?}");
    }

    let dry = classify_board(&cards(&["As", "7d", "2c"])).expect("乾面");
    assert!(dry.contains(BoardTexture::Dry));
    let wet = classify_board(&cards(&["9s", "8d", "7c"])).expect("濕面");
    assert!(wet.contains(BoardTexture::Wet));
}

#[test]
fn 彩虹與乾濕是可同時命中的兩個維度() {
    let dry = context();
    assert!(PostflopCondition {
        board_texture: Some(BoardTexture::Rainbow),
        ..Default::default()
    }
    .matches(&dry));
    assert!(PostflopCondition {
        board_texture: Some(BoardTexture::Dry),
        ..Default::default()
    }
    .matches(&dry));

    let rainbow = PostflopCondition {
        board_texture: Some(BoardTexture::Rainbow),
        ..Default::default()
    };
    let wet = PostflopCondition {
        board_texture: Some(BoardTexture::Wet),
        ..Default::default()
    };
    assert!(rainbow.intersects(&wet));
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
                    board_texture: Some(BoardTexture::Dry),
                    ..Default::default()
                },
            ),
            rule("通則", PostflopCondition::default()),
        ],
        "baseline-v1",
    );

    let (matched, distribution) = set.resolve(&context(), &all_legal);
    assert_eq!(matched, Matched::Rule(1), "應命中第二條而非通則");
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
        vec![PostflopRule {
            name: "只會下注".to_owned(),
            condition: PostflopCondition::default(),
            actions: dist(vec![(bet(10), FULL)]),
        }],
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

    assert_eq!(matched, Matched::Rule(0));
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
                    board_texture: Some(BoardTexture::Dry),
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
        issues.iter().any(|i: &RuleIssue| i.is_error()),
        "遮蔽是必須修正的 error"
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
                    board_texture: Some(BoardTexture::Dry),
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
                    board_texture: Some(BoardTexture::Dry),
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
fn 策略完整度為命中規則的比例() {
    let mut stats = CoverageStats::default();
    for _ in 0..7 {
        stats.record(&Matched::Rule(0));
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
        "7/10 命中規則 → 完整度 70%"
    );
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
