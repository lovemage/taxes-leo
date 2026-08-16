//! IPC 契約的驗收測試。
//!
//! 最重要的一組是隱藏資訊遮蔽：核心規格 2.4 要求未亮出的底牌不得外流，
//! 而遮蔽必須發生在**邊界**，不能靠前端自律。

use poker_engine::betting::{Action, LegalActions};
use poker_engine::chips::Chips;
use poker_engine::hand::{ActionProvider, Street};
use poker_engine::rng::RNG_VERSION;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::table::TableConfig;
use poker_storage::codec::{HandRecord, LOG_FORMAT_VERSION};
use poker_storage::manifest::{
    ContentSnapshot, ExecutionMode, RuleVariants, RunManifest, SCHEMA_VERSION,
};
use poker_storage::Store;

use poker_ipc::{HoleCardVisibility, IpcHandler};

fn c(n: u64) -> Chips {
    Chips::new(n)
}

struct CallingStation;

impl ActionProvider for CallingStation {
    fn choose(&mut self, _street: Street, legal: &LegalActions) -> Action {
        if legal.can_check {
            Action::Check
        } else if legal.call_to.is_some() {
            Action::Call
        } else {
            Action::AllIn
        }
    }
}

fn config() -> SessionConfig {
    SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![c(400); 9],
        auto_refill: Some(9),
        hero_seat: 0,
        hand_limit: 300,
        master_seed: 4242,
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
        big_blind: 2,
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

/// 跑一個 run、落 log，回傳 handler 與 run_id。
fn prepared() -> (IpcHandler, i64) {
    let config = config();
    let mut store = Store::open_in_memory().expect("建立資料庫");
    let m = manifest(&config);
    let run_id = store.create_run(&m).expect("建立 run");

    let mut rows = Vec::new();
    let summary = run_session(&config, &mut CallingStation, |played| {
        let record = HandRecord::from_played(played);
        let contributed = played.result.total_contributions[config.hero_seat];
        let delta = record.hero_delta(config.hero_seat, contributed);
        rows.push((record, played.seated, delta));
    });
    store.write_hands(run_id, &rows).expect("寫入");

    let mut m = manifest(&config);
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

// ── 隱藏資訊遮蔽（核心規格 2.4）─────────────────────────────────────────

#[test]
fn 未亮出的底牌不得出現在_ipc_回傳中() {
    let (handler, run_id) = prepared();

    let mut checked_any_muck = false;
    for index in 0..50u64 {
        let view = handler
            .get_hand(run_id, index, HoleCardVisibility::RevealedOnly)
            .expect("取得手牌");

        for seat in &view.seats {
            if seat.occupied && !seat.revealed {
                assert!(
                    seat.hole_cards.is_none(),
                    "第 {index} 手座位 {} 未亮牌，底牌不得外流",
                    seat.seat
                );
                checked_any_muck = true;
            }
            if seat.revealed {
                assert!(
                    seat.hole_cards.is_some(),
                    "已亮牌的座位應提供底牌供 UI 顯示"
                );
            }
        }
    }
    assert!(
        checked_any_muck,
        "測試資料中應存在未亮牌的座位，否則此測試沒有實際驗到遮蔽"
    );
}

#[test]
fn 明確開啟全揭露時才送出全部底牌() {
    let (handler, run_id) = prepared();

    let redacted = handler
        .get_hand(run_id, 0, HoleCardVisibility::RevealedOnly)
        .expect("取得手牌");
    let full = handler
        .get_hand(run_id, 0, HoleCardVisibility::All)
        .expect("取得手牌");

    let redacted_count = redacted
        .seats
        .iter()
        .filter(|s| s.hole_cards.is_some())
        .count();
    let full_count = full.seats.iter().filter(|s| s.hole_cards.is_some()).count();

    assert!(
        full_count > redacted_count,
        "全揭露模式應比預設模式送出更多底牌（{full_count} vs {redacted_count}）"
    );
    assert_eq!(
        full_count,
        full.seats.iter().filter(|s| s.occupied).count(),
        "全揭露時所有在座者都應有底牌"
    );
    // 兩種模式的 revealed 旗標必須一致，遮蔽不得竄改事實
    for (a, b) in redacted.seats.iter().zip(&full.seats) {
        assert_eq!(a.revealed, b.revealed, "亮牌事實不因可見範圍而改變");
    }
}

#[test]
fn 逐手摘要不含任何底牌() {
    let (handler, run_id) = prepared();
    let summaries = handler.list_hands(run_id, 0, 20).expect("列表");
    assert_eq!(summaries.len(), 20);

    // 摘要型別本身就沒有底牌欄位，序列化後也不應出現任何牌張欄位
    let json = serde_json::to_string(&summaries).expect("序列化");
    assert!(
        !json.contains("holeCards"),
        "逐手摘要不得帶出底牌欄位"
    );
}

// ── 位置標籤（規則細則 8.4.1）──────────────────────────────────────────

#[test]
fn 位置標籤使用規格的唯一命名() {
    let (handler, run_id) = prepared();
    let view = handler
        .get_hand(run_id, 0, HoleCardVisibility::RevealedOnly)
        .expect("取得手牌");

    let labels: Vec<&str> = view
        .seats
        .iter()
        .filter_map(|s| s.position.as_deref())
        .collect();

    let allowed = [
        "UTG", "UTG+1", "UTG+2", "UTG+3", "UTG+4", "LJ", "HJ", "CO", "BTN", "SB", "BB",
    ];
    for label in &labels {
        assert!(allowed.contains(label), "出現規格外的位置標籤 {label}");
    }
    assert!(labels.contains(&"BB"), "BB 標籤恆存在");
    assert_eq!(
        labels.len(),
        view.seats.iter().filter(|s| s.occupied).count(),
        "每位在桌玩家都應有標籤"
    );
}

// ── run 摘要 ────────────────────────────────────────────────────────────

#[test]
fn run_摘要帶出_rng_版本與桌次數() {
    let (handler, run_id) = prepared();
    let run = handler.get_run(run_id).expect("取得 run");

    assert_eq!(run.run_id, run_id);
    assert_eq!(run.rng_algorithm, RNG_VERSION, "RNG 版本必須可供 UI 顯示");
    assert_eq!(run.players, 9);
    assert!(run.completed);
    assert!(run.instance_count >= 1, "桌次數供統計層判斷 block 是否足夠");
}

// ── 序列化格式 ──────────────────────────────────────────────────────────

#[test]
fn dto_序列化為_camel_case_供前端直接使用() {
    let (handler, run_id) = prepared();
    let view = handler
        .get_hand(run_id, 0, HoleCardVisibility::RevealedOnly)
        .expect("取得手牌");
    let json = serde_json::to_string(&view).expect("序列化");

    assert!(json.contains("handIndex"), "欄位須為 camelCase");
    assert!(json.contains("deadButton"));
    assert!(!json.contains("hand_index"), "不得混入 snake_case");
}
