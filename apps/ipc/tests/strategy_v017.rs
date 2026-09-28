//! 0.1.7：OPEN 尺度區間、3-bet／Squeeze 分離、翻後底池類型與 Hero OPEN 快照。

use std::sync::{Arc, Mutex};

use poker_engine::bot::{scenario_with_bounds, BotAgent, BotConfig};
use poker_engine::strategy::baseline::BaselineRules;
use poker_engine::strategy::decision::{OpponentPublic, PublicAction};
use poker_engine::strategy::postflop_baseline::engineering_rules;
use poker_engine::strategy::preflop::{OpenSizeTier, OpenTierBounds, PreflopScenario};
use poker_engine::strategy::{DecisionView, StackBucket};
use poker_engine::{
    betting::{Action, LegalActions, RaiseRange},
    card::Card,
    chips::Chips,
    hand::{ActionProvider, Street},
    position::PositionLabel as P,
};
use poker_ipc::postflop::{
    postflop_rule, PostflopNodeOverrideView, PostflopOverridesView, PostflopRuleQuery,
    PostflopWeightInput,
};
use poker_ipc::run::{execute, HeroStrategy, RunControl, RunRequest};
use poker_ipc::strategy::{matrix, to_cell_overrides, CellOverrideView, OpenTierBoundsView};
use poker_storage::Store;

const BB: u64 = 2;

fn raise(seat: usize, position: P, to: u64) -> PublicAction {
    PublicAction {
        street: Street::Preflop,
        seat,
        position,
        action: Action::RaiseTo(Chips::new(to)),
        raised: true,
        committed_to: Chips::new(to),
    }
}

fn call(seat: usize, position: P, to: u64) -> PublicAction {
    PublicAction {
        street: Street::Preflop,
        seat,
        position,
        action: Action::Call,
        raised: false,
        committed_to: Chips::new(to),
    }
}

/// 9 人桌，英雄在 BTN（座位 7），UTG（座位 2）open 到 `open_to`。
fn facing_open(open_to: u64) -> DecisionView {
    DecisionView {
        seat: 7,
        position: P::Btn,
        street: Street::Preflop,
        hole_cards: [Card::parse("7h").unwrap(), Card::parse("2c").unwrap()],
        board: Vec::new(),
        seated: 9,
        effective_stack_bucket: StackBucket::Deeper,
        pot: Chips::new(open_to + 3),
        to_call: Chips::new(open_to),
        big_blind: Chips::new(BB),
        legal: LegalActions {
            seat: 7,
            can_fold: true,
            can_check: false,
            call_to: Some(Chips::new(open_to)),
            raise: Some(RaiseRange {
                min_to: Chips::new(open_to * 2 - BB),
                max_to: Chips::new(200),
            }),
            all_in_to: Some(Chips::new(200)),
        },
        history: vec![raise(2, P::Utg, open_to)],
        opponents: vec![OpponentPublic {
            seat: 2,
            position: P::Utg,
            stack: Chips::new(200 - open_to),
            committed: Chips::new(open_to),
            folded: false,
            all_in: false,
        }],
    }
}

fn cell(scenario: &str, class: &str, aggressive: u32, call: u32) -> CellOverrideView {
    CellOverrideView {
        seated: 9,
        hero: "BTN".to_owned(),
        bucket: "70-110".to_owned(),
        scenario: scenario.to_owned(),
        class: class.to_owned(),
        aggressive,
        call,
    }
}

// ── 三、依對手 OPEN 尺度區分 ─────────────────────────────────────────────

#[test]
fn 區間依實際_open_總額判定而非名目尺寸() {
    let bounds = OpenTierBounds::DEFAULT;
    for (open_to, want) in [
        (5, OpenSizeTier::Standard), // 2.5BB
        (6, OpenSizeTier::Standard), // 3BB：邊界屬標準
        (7, OpenSizeTier::Medium),   // 3.5BB
        (12, OpenSizeTier::Medium),  // 6BB：邊界屬中型
        (13, OpenSizeTier::Large),   // 6.5BB
        (20, OpenSizeTier::Large),   // 10BB
    ] {
        assert_eq!(
            scenario_with_bounds(&facing_open(open_to), bounds),
            PreflopScenario::VsOpen {
                opener: P::Utg,
                size: want
            },
            "open 到 {open_to} 單位"
        );
    }
    // 邊界可調：把標準上限提高到 4BB，3.5BB 就回到標準區間
    let wide = OpenTierBounds {
        medium_above_centi_bb: 400,
        large_above_centi_bb: 800,
    };
    assert_eq!(
        scenario_with_bounds(&facing_open(7), wide).open_tier(),
        Some(OpenSizeTier::Standard)
    );
}

#[test]
fn 大型區間未設定時沿用標準區間_另設後只作用於該區間() {
    let standard = "vs-open-UTG";
    let large = "vs-open-UTG@open-large";
    let only_standard = vec![cell(standard, "72o", 10_000, 0)];

    let view = matrix(9, "BTN", "70-110", large, &only_standard).expect("大型區間矩陣");
    let seven_two = view.cells.iter().find(|c| c.class == "72o").unwrap();
    assert!(seven_two.inherited_override && !seven_two.overridden);
    assert_eq!(seven_two.aggressive, 10_000, "未另設時沿用標準區間的覆寫");
    assert_eq!(view.inherited_override_count, 1);
    assert_eq!(view.open_tier.as_deref(), Some("large"));
    assert!(
        view.content_note.is_some(),
        "大型區間必須說明內容沿用標準區間"
    );

    let mut both = only_standard.clone();
    both.push(cell(large, "72o", 0, 0));
    let view = matrix(9, "BTN", "70-110", large, &both).expect("大型區間矩陣");
    let seven_two = view.cells.iter().find(|c| c.class == "72o").unwrap();
    assert!(seven_two.overridden && !seven_two.inherited_override);
    assert_eq!(seven_two.fold, 10_000, "大型區間自己的覆寫優先");

    let view = matrix(9, "BTN", "70-110", standard, &both).expect("標準區間矩陣");
    let seven_two = view.cells.iter().find(|c| c.class == "72o").unwrap();
    assert_eq!(
        seven_two.aggressive, 10_000,
        "大型區間的覆寫不得外溢到標準區間"
    );
    assert_eq!(view.open_tier.as_deref(), Some("standard"));
}

#[test]
fn 區間覆寫鍵可解析_未知區間拒絕() {
    assert!(to_cell_overrides(&[cell("vs-open-UTG@open-medium", "AA", 10_000, 0)]).is_ok());
    assert!(to_cell_overrides(&[cell("vs-open-UTG@open-huge", "AA", 10_000, 0)]).is_err());
    assert!(to_cell_overrides(&[cell("vs-open-UTG@", "AA", 10_000, 0)]).is_err());
    // 不可達的開牌者仍然擋下，不因為帶了區間就放行
    assert!(to_cell_overrides(&[cell("vs-open-SB@open-large", "AA", 10_000, 0)]).is_err());
}

#[test]
fn 區間邊界不合法時拒絕() {
    for (medium, large) in [(600, 600), (100, 600), (300, 20_000)] {
        assert!(OpenTierBoundsView {
            medium_above_centi_bb: medium,
            large_above_centi_bb: large,
        }
        .to_bounds()
        .is_err());
    }
    assert!(OpenTierBoundsView::default().to_bounds().is_ok());
}

fn agent_with(overrides: &[CellOverrideView]) -> BotAgent {
    let configs = (0..9)
        .map(|seat| BotConfig::defaults(format!("座位 {seat}")))
        .collect();
    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        BotAgent::rankings(40),
        configs,
        7,
    );
    agent.set_seat_overrides(7, to_cell_overrides(overrides).expect("覆寫"));
    agent.set_hero_postflop_rules(7, engineering_rules());
    agent
}

#[test]
fn 實際決策依_open_區間讀不同覆寫並記錄命中() {
    let overrides = vec![
        cell("vs-open-UTG", "72o", 0, 10_000),
        cell("vs-open-UTG@open-large", "72o", 0, 0),
    ];
    let mut agent = agent_with(&overrides);

    assert_eq!(
        agent.choose(&facing_open(5)),
        Action::Call,
        "2.5BB 走標準區間：跟注"
    );
    assert_eq!(
        agent.choose(&facing_open(20)),
        Action::Fold,
        "10BB 走大型區間：棄牌"
    );
    assert_eq!(
        agent.choose(&facing_open(8)),
        Action::Call,
        "4BB 屬中型區間，未另設時沿用標準區間"
    );

    let hits = agent.preflop_hits();
    assert_eq!(hits.total(), 3);
    assert_eq!(hits.scenarios.get("vs-open-UTG"), Some(&1));
    assert_eq!(hits.scenarios.get("vs-open-UTG@open-medium"), Some(&1));
    assert_eq!(hits.scenarios.get("vs-open-UTG@open-large"), Some(&1));
    assert_eq!(hits.open_sizes.get(&(OpenSizeTier::Large, 1_000)), Some(&1));
    assert_eq!(
        hits.open_sizes.get(&(OpenSizeTier::Standard, 250)),
        Some(&1)
    );
}

#[test]
fn 對手座位不記命中也不讀英雄的區間覆寫() {
    let mut agent = agent_with(&[cell("vs-open-UTG@open-large", "72o", 10_000, 0)]);
    let mut view = facing_open(20);
    view.seat = 6;
    view.position = P::Co;
    view.legal.seat = 6;
    agent.choose(&view);
    assert_eq!(agent.preflop_hits().total(), 0);
}

// ── 二、3-bet 與 Squeeze 分離 ───────────────────────────────────────────

/// 英雄 UTG（座位 2）open 到 5；`caller` 為 true 時 CO 先跟注，再由 BTN 加注。
fn facing_reraise(caller: bool) -> DecisionView {
    let mut view = facing_open(5);
    view.seat = 2;
    view.position = P::Utg;
    view.legal.seat = 2;
    view.hole_cards = [Card::parse("7h").unwrap(), Card::parse("2c").unwrap()];
    view.history = vec![raise(2, P::Utg, 5)];
    if caller {
        view.history.push(call(6, P::Co, 5));
    }
    view.history.push(raise(7, P::Btn, 18));
    view.to_call = Chips::new(13);
    view.legal.call_to = Some(Chips::new(18));
    view.legal.raise = Some(RaiseRange {
        min_to: Chips::new(31),
        max_to: Chips::new(200),
    });
    view
}

#[test]
fn 擠壓與_3bet_走各自的節點與覆寫() {
    assert_eq!(
        scenario_with_bounds(&facing_reraise(false), OpenTierBounds::DEFAULT),
        PreflopScenario::VsThreeBet { by: P::Btn }
    );
    assert_eq!(
        scenario_with_bounds(&facing_reraise(true), OpenTierBounds::DEFAULT),
        PreflopScenario::VsSqueeze { by: P::Btn }
    );

    let overrides = vec![
        CellOverrideView {
            hero: "UTG".to_owned(),
            ..cell("vs-3bet-BTN", "72o", 0, 10_000)
        },
        CellOverrideView {
            hero: "UTG".to_owned(),
            ..cell("vs-squeeze-BTN", "72o", 0, 0)
        },
    ];
    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        BotAgent::rankings(40),
        (0..9)
            .map(|s| BotConfig::defaults(format!("{s}")))
            .collect(),
        7,
    );
    agent.set_seat_overrides(2, to_cell_overrides(&overrides).expect("覆寫"));
    agent.set_hero_postflop_rules(2, engineering_rules());

    assert_eq!(
        agent.choose(&facing_reraise(false)),
        Action::Call,
        "3-bet 節點：跟注"
    );
    assert_eq!(
        agent.choose(&facing_reraise(true)),
        Action::Fold,
        "Squeeze 節點：棄牌"
    );
    let hits = agent.preflop_hits();
    assert_eq!(hits.scenarios.get("vs-3bet-BTN"), Some(&1));
    assert_eq!(hits.scenarios.get("vs-squeeze-BTN"), Some(&1));
}

#[test]
fn 擠壓矩陣標明借用_3b_欄且覆寫不影響_3bet() {
    let overrides = vec![CellOverrideView {
        hero: "UTG".to_owned(),
        ..cell("vs-squeeze-BTN", "AA", 0, 10_000)
    }];
    let squeeze = matrix(9, "UTG", "70-110", "vs-squeeze-BTN", &overrides).expect("擠壓矩陣");
    assert_eq!(squeeze.scenario_kind, "vs-squeeze");
    assert!(squeeze
        .content_note
        .as_deref()
        .is_some_and(|note| note.contains("Squeeze")));
    assert_eq!(squeeze.override_count, 1);

    let three_bet = matrix(9, "UTG", "70-110", "vs-3bet-BTN", &overrides).expect("3-bet 矩陣");
    assert_eq!(three_bet.scenario_kind, "vs-3bet");
    assert_eq!(
        three_bet.override_count, 0,
        "擠壓的覆寫不得出現在 3-bet 節點"
    );
    assert!(three_bet.content_note.is_none());
}

// ── 四、翻後底池類型 ─────────────────────────────────────────────────────

fn postflop_query(scope: &str) -> PostflopRuleQuery {
    PostflopRuleQuery {
        scope: scope.to_owned(),
        street: "flop".to_owned(),
        situation: "no-bet".to_owned(),
        line: "cbet-chance".to_owned(),
        surface: "rainbow".to_owned(),
        connectivity: "dry".to_owned(),
        hand_strength: "strong-made".to_owned(),
        facing_size: "none".to_owned(),
    }
}

fn check_only(node_key: String) -> PostflopOverridesView {
    PostflopOverridesView {
        nodes: vec![PostflopNodeOverrideView {
            node_key,
            weights: vec![PostflopWeightInput {
                kind: "check".to_owned(),
                myriad: 10_000,
            }],
        }],
        rules: Vec::new(),
    }
}

#[test]
fn 底池類型限定的覆寫只作用於該底池() {
    let three_bet_scope = "*/*/*/*/three-bet/*";
    let key = postflop_rule(
        &postflop_query(three_bet_scope),
        &PostflopOverridesView::default(),
    )
    .expect("查詢")
    .node_key;
    assert!(key.ends_with("|*/*/*/*/three-bet/*"), "六段 scope：{key}");

    let overrides = check_only(key.clone());
    let hit = postflop_rule(&postflop_query(three_bet_scope), &overrides).expect("3-bet 池");
    assert_eq!(hit.source, "user-node-override");
    assert!(hit.restore.enabled);

    for scope in [
        "*/*/*/*/single-raised/*",
        "*/*/*/*/four-bet/*",
        "*/*/*/*/*/*",
    ] {
        let other = postflop_rule(&postflop_query(scope), &overrides).expect("其他底池");
        assert_ne!(
            other.source, "user-node-override",
            "{scope} 不得讀到 3-bet 池的覆寫"
        );
    }
}

#[test]
fn 未指定底池與_spr_時沿用_016_的鍵() {
    let base = postflop_rule(
        &postflop_query("*/*/*/*/*/*"),
        &PostflopOverridesView::default(),
    )
    .expect("通用")
    .node_key;
    assert_eq!(base.split('|').count(), 7, "全部不限就是通用節點鍵：{base}");

    let scoped = postflop_rule(
        &postflop_query("BTN/*/*/*/*/*"),
        &PostflopOverridesView::default(),
    )
    .expect("位置限定")
    .node_key;
    assert!(
        scoped.ends_with("|BTN/*/*/*"),
        "後兩段不限時用四段舊格式：{scoped}"
    );

    // 0.1.6 存下的四段鍵照樣生效
    let overrides = check_only(scoped.clone());
    let hit = postflop_rule(&postflop_query("BTN/*/*/*"), &overrides).expect("舊格式查詢");
    assert_eq!(hit.source, "user-node-override");
    let hit = postflop_rule(&postflop_query("BTN/*/*/*/*/*"), &overrides).expect("新格式查詢");
    assert_eq!(hit.source, "user-node-override");
}

#[test]
fn 未知的底池或_spr_區間被拒絕() {
    assert!(postflop_rule(
        &postflop_query("*/*/*/*/five-bet/*"),
        &PostflopOverridesView::default()
    )
    .is_err());
    assert!(postflop_rule(
        &postflop_query("*/*/*/*/*/spr-99"),
        &PostflopOverridesView::default()
    )
    .is_err());
    assert!(postflop_rule(
        &postflop_query("*/*/*/*/*/spr-3-6"),
        &PostflopOverridesView::default()
    )
    .is_ok());
}

// ── 執行快照 ─────────────────────────────────────────────────────────────

fn request() -> RunRequest {
    RunRequest {
        players: 9,
        auto_refill_enabled: true,
        auto_refill_target: 9,
        starting_stack_bb: 100,
        small_blind: 1,
        big_blind: 2,
        ante_mode: "none".to_owned(),
        ante_amount: 0,
        straddle_mode: "none".to_owned(),
        rake_basis_points: 0,
        rake_cap_bb: 0,
        rake_no_flop_no_drop: false,
        hand_limit: 60,
        master_seed: "20260928".to_owned(),
        hero_seat: 0,
        bots: Vec::new(),
        hero_overrides: Vec::new(),
        hero_postflop_overrides: PostflopOverridesView::default(),
        hero_open_tiers: OpenTierBoundsView {
            medium_above_centi_bb: 350,
            large_above_centi_bb: 700,
        },
    }
}

#[test]
fn run_快照記錄區間邊界與翻前命中() {
    let request = request();
    let config = request.to_session_config().expect("轉換");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let run_id = execute(
        &config,
        &request.bots,
        HeroStrategy {
            preflop_overrides: &request.hero_overrides,
            postflop_overrides: &request.hero_postflop_overrides,
            open_tiers: request.hero_open_tiers,
        },
        &store,
        &Arc::new(RunControl::default()),
        1_790_000_000,
        |_| {},
    )
    .expect("run");

    let manifest = store
        .lock()
        .unwrap()
        .load_manifest(run_id)
        .expect("manifest");
    let hits = manifest.preflop_hits.expect("翻前命中必須寫進快照");
    assert!(hits.total > 0);
    assert_eq!(hits.total, hits.scenarios.values().sum::<u64>());
    assert_eq!(
        (hits.medium_above_centi_bb, hits.large_above_centi_bb),
        (350, 700)
    );
    let open_hits: u64 = hits.open_sizes.iter().map(|hit| hit.count).sum();
    let vs_open: u64 = hits
        .scenarios
        .iter()
        .filter(|(key, _)| key.starts_with("vs-open-") && !key.starts_with("vs-open-raise-"))
        .map(|(_, count)| count)
        .sum();
    assert_eq!(open_hits, vs_open, "每次面對開牌都要留下實際尺寸");

    let bounds = &manifest.hero_strategy.content["preflop"]["openTierBoundsCentiBb"];
    assert_eq!(bounds["mediumAbove"], 350);
    assert_eq!(bounds["largeAbove"], 700);

    // 計算頁顯示用的檢視：每一列都要解得出中文標籤與種類，次數與快照一致
    let view = poker_ipc::views::run_view(&store.lock().unwrap(), run_id).expect("run 檢視");
    let shown = view.preflop_hits.expect("run 檢視必須帶翻前命中");
    assert_eq!(shown.total, hits.total);
    assert_eq!(
        shown.scenarios.iter().map(|row| row.count).sum::<u64>(),
        hits.total
    );
    assert!(shown.scenarios.iter().all(|row| row.kind != "unknown"));
    assert!(shown
        .scenarios
        .windows(2)
        .all(|pair| pair[0].kind == pair[1].kind || pair[0].kind_label != pair[1].kind_label));
    assert!(shown
        .open_sizes
        .windows(2)
        .all(|pair| pair[0].centi_bb <= pair[1].centi_bb));
}

#[test]
fn 不合法的區間邊界讓_run_拒絕啟動() {
    let mut request = request();
    request.hero_open_tiers = OpenTierBoundsView {
        medium_above_centi_bb: 700,
        large_above_centi_bb: 350,
    };
    let config = request.to_session_config().expect("轉換");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let outcome = execute(
        &config,
        &request.bots,
        HeroStrategy {
            preflop_overrides: &request.hero_overrides,
            postflop_overrides: &request.hero_postflop_overrides,
            open_tiers: request.hero_open_tiers,
        },
        &store,
        &Arc::new(RunControl::default()),
        1_790_000_000,
        |_| {},
    );
    assert!(outcome.is_err());
}
