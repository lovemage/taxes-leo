//! 儲存層驗收測試。
//!
//! 對應核心規格 3.3（`RunManifest` 必要欄位）、7.2（載入延遲）、
//! 以及實做計劃 M0 的 log 容量規格。

use std::time::Instant;

use poker_engine::betting::{Action, LegalActions};
use poker_engine::chips::Chips;
use poker_engine::hand::{ActionProvider, Street};
use poker_engine::rng::RNG_VERSION;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::table::TableConfig;

use poker_storage::codec::{decode, encode, HandRecord, LOG_FORMAT_VERSION};
use poker_storage::manifest::{
    ContentSnapshot, ExecutionMode, RuleVariants, RunManifest, SCHEMA_VERSION,
};
use poker_storage::{Store, StorageError};

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

fn session_config(hands: u64) -> SessionConfig {
    SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![c(400); 9],
        auto_refill: Some(9),
        hero_seat: 0,
        hand_limit: hands,
        master_seed: 20260816,
        }
}

fn manifest_for(config: &SessionConfig) -> RunManifest {
    RunManifest {
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: SCHEMA_VERSION,
        log_format_version: LOG_FORMAT_VERSION,
        rng_algorithm: RNG_VERSION.to_owned(),
        stream_derivation: "splitmix64(master_seed, hand_index, domain) → xoshiro256**".to_owned(),
        master_seed: config.master_seed,
        execution_mode: ExecutionMode::Batch,
        hand_limit: config.hand_limit,
        players: config.players,
        hero_seat: config.hero_seat,
        starting_stacks: config.starting_stacks.iter().map(|c| c.units()).collect(),
        small_blind: config.table.small_blind.units(),
        big_blind: config.table.big_blind.units(),
        ante_mode: "none".to_owned(),
        ante_amount: 0,
        straddle_amounts: Vec::new(),
        rake_basis_points: config.table.rake.basis_points,
        rake_cap: config.table.rake.cap.units(),
        rake_no_flop_no_drop: config.table.rake.no_flop_no_drop,
        stack_policy: "bustOut".to_owned(),
        auto_refill_target: config.auto_refill,
        rule_variants: RuleVariants::default(),
        hero_strategy: ContentSnapshot::new(
            "測試策略",
            "v0",
            serde_json::json!({ "note": "M2 前以佔位內容代替" }),
        ),
        bot_personas: Vec::new(),
        baseline_version: "none".to_owned(),
        instances: Vec::new(),
        created_at: 1_771_200_000,
        completed: false,
        checkpoint_version: 1,
    }
}

/// 跑一個 run 並全部落 log，回傳 (store, run_id, 手數)。
fn run_and_store(hands: u64) -> (Store, i64, u64) {
    let config = session_config(hands);
    let mut store = Store::open_in_memory().expect("建立記憶體資料庫");
    let manifest = manifest_for(&config);
    let run_id = store.create_run(&manifest).expect("建立 run");

    let mut batch: Vec<(HandRecord, usize, i64)> = Vec::new();
    let summary = run_session(&config, &mut CallingStation, |played| {
        let record = HandRecord::from_played(played);
        let contributed = played.result.total_contributions[config.hero_seat];
        let delta = record.hero_delta(config.hero_seat, contributed);
        batch.push((record, played.seated, delta));
    });

    // 批次交易寫入（核心規格 3.2）
    for chunk in batch.chunks(1000) {
        store.write_hands(run_id, chunk).expect("批次寫入");
    }
    store
        .finish_run(run_id, &manifest, summary.hands_played)
        .expect("結束 run");

    (store, run_id, summary.hands_played)
}

// ── 編解碼 ──────────────────────────────────────────────────────────────

#[test]
fn 逐手紀錄編解碼可完整還原() {
    let config = session_config(200);
    let mut records = Vec::new();
    run_session(&config, &mut CallingStation, |played| {
        records.push(HandRecord::from_played(played));
    });

    assert!(!records.is_empty());
    for record in &records {
        let bytes = encode(record);
        let restored = decode(&bytes).expect("解碼");
        assert_eq!(*record, restored, "編解碼必須完整還原");
    }
}

#[test]
fn 格式版本不符時拒絕解碼() {
    let config = session_config(5);
    let mut first = None;
    run_session(&config, &mut CallingStation, |played| {
        if first.is_none() {
            first = Some(HandRecord::from_played(played));
        }
    });
    let mut bytes = encode(&first.expect("至少一手"));

    // 竄改版本欄位
    bytes[2] = 0xFF;
    assert!(
        decode(&bytes).is_err(),
        "版本不符必須拒絕，不得盡力而為地解讀"
    );

    // 竄改 magic
    let mut bytes = encode(&HandRecord {
        hand_index: 0,
        instance_index: 0,
        occupied: vec![true; 6],
        big_blind_seat: 2,
        hole_cards: vec![None; 6],
        board: Vec::new(),
        actions: Vec::new(),
        payouts: vec![Chips::ZERO; 6],
        refunds: vec![Chips::ZERO; 6],
        rake: Chips::ZERO,
    });
    bytes[0] = 0;
    assert!(decode(&bytes).is_err(), "magic 不符必須拒絕");
}

// ── RunManifest（核心規格 3.3）──────────────────────────────────────────

#[test]
fn manifest_可存讀且內容快照與_hash_相符() {
    let config = session_config(10);
    let manifest = manifest_for(&config);
    assert!(manifest.validate().is_ok(), "manifest 應通過驗證");
    assert!(
        manifest.hero_strategy.verify(),
        "內容快照的 hash 必須與內容相符"
    );

    let mut store = Store::open_in_memory().expect("建立資料庫");
    let run_id = store.create_run(&manifest).expect("建立 run");
    let loaded = store.load_manifest(run_id).expect("讀取 manifest");

    assert_eq!(loaded.rng_algorithm, RNG_VERSION);
    assert_eq!(loaded.master_seed, config.master_seed);
    assert_eq!(loaded.starting_stacks.len(), config.players);
    assert_eq!(loaded.stack_policy, "bustOut");
    // 規則細則第七章：不實作的現實規則必須有明確紀錄
    assert!(!loaded.rule_variants.burn_card);
    assert!(!loaded.rule_variants.run_it_twice);
    assert_eq!(loaded.rule_variants.muck_policy, "realistic");
}

#[test]
fn 內容被竄改時_hash_校驗失敗() {
    let config = session_config(10);
    let mut manifest = manifest_for(&config);
    manifest.hero_strategy.content = serde_json::json!({ "note": "被改過" });
    assert!(
        !manifest.hero_strategy.verify(),
        "內容與 hash 不符必須被偵測"
    );
    assert!(manifest.validate().is_err());

    let mut store = Store::open_in_memory().expect("建立資料庫");
    assert!(
        matches!(
            store.create_run(&manifest),
            Err(StorageError::InvalidManifest(_))
        ),
        "不合法的 manifest 不得寫入"
    );
}

#[test]
fn 桌次邊界寫入_manifest() {
    let config = session_config(400);
    let mut store = Store::open_in_memory().expect("建立資料庫");
    let mut manifest = manifest_for(&config);
    let run_id = store.create_run(&manifest).expect("建立 run");

    let summary = run_session(&config, &mut CallingStation, |_| {});
    manifest.instances = summary
        .instances
        .iter()
        .map(|i| poker_storage::InstanceRecord {
            index: i.index,
            first_hand: 0,
            last_hand: i.hands.saturating_sub(1),
            hands: i.hands,
            end: format!("{:?}", i.end),
            refills: i
                .refills
                .iter()
                .map(|r| poker_storage::RefillRecord {
                    hand_index: r.hand_index,
                    seat: r.seat,
                    buy_in: r.buy_in.units(),
                })
                .collect(),
        })
        .collect();
    manifest.completed = true;

    store
        .finish_run(run_id, &manifest, summary.hands_played)
        .expect("結束 run");
    let loaded = store.load_manifest(run_id).expect("讀取");

    assert!(loaded.completed);
    assert!(!loaded.instances.is_empty(), "桌次邊界必須寫入 manifest");
    assert_eq!(
        loaded.instances.iter().map(|i| i.hands).sum::<u64>(),
        summary.hands_played
    );
}

// ── 查詢與延遲（核心規格 7.2）──────────────────────────────────────────

#[test]
fn 可依手序載入指定手牌() {
    let (store, run_id, hands) = run_and_store(500);
    assert!(hands > 0);

    for index in [0, hands / 2, hands - 1] {
        let record = store.load_hand(run_id, index).expect("載入指定手牌");
        assert_eq!(record.hand_index, index);
    }
    assert!(
        matches!(store.load_hand(run_id, hands + 100), Err(StorageError::NotFound)),
        "不存在的手序應回報 NotFound"
    );
}

#[test]
fn 分頁瀏覽依手序遞增() {
    let (store, run_id, hands) = run_and_store(300);
    let page = store.page_hands(run_id, 50, 25).expect("分頁");
    assert_eq!(page.len(), 25.min(usize::try_from(hands).unwrap_or(0)));
    for pair in page.windows(2) {
        assert!(pair[0].hand_index < pair[1].hand_index, "分頁須依手序遞增");
    }
}

#[test]
fn 使用者損益序列免解碼可取得() {
    let (store, run_id, hands) = run_and_store(300);
    let deltas = store.hero_deltas(run_id).expect("讀取損益");
    assert_eq!(deltas.len(), usize::try_from(hands).expect("手數"));
}

#[test]
fn 載入指定手牌的延遲遠低於門檻() {
    // 核心規格 7.2：100 萬手 DB 載入指定手牌 p95 ≤ 200 ms。
    // 這裡用較小的資料量做趨勢確認，正式量測依 M0 的 258V 基準機進行。
    let (store, run_id, hands) = run_and_store(2000);

    let mut worst = std::time::Duration::ZERO;
    for index in (0..hands).step_by(97) {
        let start = Instant::now();
        store.load_hand(run_id, index).expect("載入");
        worst = worst.max(start.elapsed());
    }
    assert!(
        worst.as_millis() < 200,
        "單手載入最壞值 {worst:?} 已逼近 200ms 門檻"
    );
}

// ── log 容量（實做計劃 M0）─────────────────────────────────────────────

#[test]
fn 每手_log_大小可外推至百萬手兩_gb_門檻() {
    let hands = 2000u64;
    let (store, _run_id, played) = run_and_store(hands);
    let bytes = store.size_bytes().expect("查詢大小");
    let per_hand = bytes / played.max(1);

    // 100 萬手 ≤ 2GB → 每手平均需 ≤ 約 2048 位元組
    assert!(
        per_hand <= 2048,
        "每手平均 {per_hand} 位元組，外推 100 萬手為 {} MB，超過 2GB 門檻",
        per_hand * 1_000_000 / 1_048_576
    );

    // 印出供 M0 容量規格定案參考
    println!(
        "每手平均 {per_hand} 位元組；外推 100 萬手約 {} MB",
        per_hand * 1_000_000 / 1_048_576
    );
}

// ── 清理（M0 log 規格）─────────────────────────────────────────────────

#[test]
fn 可依日期範圍刪除_run() {
    let config = session_config(20);
    let mut store = Store::open_in_memory().expect("建立資料庫");

    let mut old = manifest_for(&config);
    old.created_at = 1_000_000_000;
    let old_id = store.create_run(&old).expect("建立舊 run");

    let mut recent = manifest_for(&config);
    recent.created_at = 2_000_000_000;
    let recent_id = store.create_run(&recent).expect("建立新 run");

    let removed = store.delete_runs_before(1_500_000_000).expect("刪除");
    assert_eq!(removed, 1, "只應刪掉早於門檻的 run");
    assert!(store.load_manifest(old_id).is_err(), "舊 run 已刪除");
    assert!(store.load_manifest(recent_id).is_ok(), "新 run 應保留");
}

// ── 重播一致性 ──────────────────────────────────────────────────────────

#[test]
fn 由_log_還原的行動序列與原執行一致() {
    let config = session_config(200);

    let mut original: Vec<HandRecord> = Vec::new();
    run_session(&config, &mut CallingStation, |played| {
        original.push(HandRecord::from_played(played));
    });

    let mut store = Store::open_in_memory().expect("建立資料庫");
    let manifest = manifest_for(&config);
    let run_id = store.create_run(&manifest).expect("建立 run");
    let rows: Vec<(HandRecord, usize, i64)> = original
        .iter()
        .map(|r| (r.clone(), 9usize, 0i64))
        .collect();
    store.write_hands(run_id, &rows).expect("寫入");

    for record in &original {
        let loaded = store.load_hand(run_id, record.hand_index).expect("載入");
        assert_eq!(loaded.board, record.board, "公共牌必須一致");
        assert_eq!(loaded.hole_cards, record.hole_cards, "底牌必須一致");
        assert_eq!(loaded.actions, record.actions, "行動序列必須逐筆一致");
        assert_eq!(loaded.payouts, record.payouts, "分配結果必須一致");
        assert_eq!(loaded.rake, record.rake);
    }
}
