//! Tauri 桌面殼。
//!
//! **這一層是薄殼。** 每個 command 只做三件事：解析參數、呼叫
//! [`poker_ipc::IpcHandler`]、把結果交還前端。牌局邏輯一律留在引擎
//! （實做計劃鐵則 2：Rust 引擎是牌局的唯一權威，UI 只是 view）。
//!
//! command 名稱與參數形狀刻意與開發用 HTTP 外殼（`apps/devserver`）一致，
//! 因此前端從 HTTP 切到 Tauri 時只需替換 `apps/ui/src/api.ts` 的傳輸層。
//!
//! # 為什麼不在 workspace 裡
//!
//! Tauri 在 Linux 需要 webkit2gtk，開發機沒有。若把本 crate 併入根
//! workspace，`cargo test` 會因為它編不過而全部停擺。Windows 端用的是
//! 系統內建的 WebView2，不需要 webkit2gtk，因此在本目錄單獨建置即可。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use poker_engine::betting::Action;
use poker_engine::chips::Chips;
use poker_engine::hand::ActionProvider;
use poker_engine::rng::RNG_VERSION;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::strategy::DecisionView;
use poker_engine::table::TableConfig;
use poker_ipc::{HandSummaryView, HandView, HoleCardVisibility, IpcHandler, RunView};
use poker_storage::codec::{HandRecord, LOG_FORMAT_VERSION};
use poker_storage::manifest::{
    ContentSnapshot, ExecutionMode, InstanceRecord, RuleVariants, RunManifest, SCHEMA_VERSION,
};
use poker_storage::Store;
use tauri::State;

const HERO_SEAT: usize = 0;

/// 應用程式狀態。
struct AppState {
    handler: Mutex<IpcHandler>,
    run_id: i64,
}

/// 示範用的行動來源。M2 的策略內容就緒後由使用者策略與 Bot 決策取代。
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

/// Tauri command 的錯誤型別。前端只會拿到訊息字串。
type CommandResult<T> = Result<T, String>;

#[tauri::command]
fn get_run(state: State<'_, AppState>) -> CommandResult<RunView> {
    let handler = state.handler.lock().map_err(|_| "狀態鎖已毀損")?;
    handler
        .get_run(state.run_id)
        .map_err(|e| format!("取得 run 失敗：{e:?}"))
}

#[tauri::command]
fn list_hands(
    state: State<'_, AppState>,
    offset: u64,
    limit: u64,
) -> CommandResult<Vec<HandSummaryView>> {
    let handler = state.handler.lock().map_err(|_| "狀態鎖已毀損")?;
    handler
        .list_hands(state.run_id, offset, limit.min(500))
        .map_err(|e| format!("列表失敗：{e:?}"))
}

#[tauri::command]
fn get_hand(state: State<'_, AppState>, index: u64, reveal_all: bool) -> CommandResult<HandView> {
    // 核心規格 2.4：重播是否顯示未攤牌底牌採明確設定，預設不顯示。
    // 遮蔽在 IPC 邊界完成，未亮出的底牌不會進入回傳值
    let visibility = if reveal_all {
        HoleCardVisibility::All
    } else {
        HoleCardVisibility::RevealedOnly
    };
    let handler = state.handler.lock().map_err(|_| "狀態鎖已毀損")?;
    handler
        .get_hand(state.run_id, index, visibility)
        .map_err(|e| format!("取得手牌失敗：{e:?}"))
}

fn build_manifest(config: &SessionConfig) -> RunManifest {
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
        rake_basis_points: 0,
        rake_cap: 0,
        rake_no_flop_no_drop: false,
        stack_policy: "bustOut".to_owned(),
        auto_refill_target: config.auto_refill,
        rule_variants: RuleVariants::default(),
        hero_strategy: ContentSnapshot::new(
            "示範策略",
            "v0",
            serde_json::json!({ "note": "顧問內容就緒前的佔位" }),
        ),
        bot_personas: Vec::new(),
        baseline_version: "none".to_owned(),
        instances: Vec::new(),
        created_at: 1_771_200_000,
        completed: false,
        checkpoint_version: 1,
    }
}

/// 產生示範資料。正式版由面板 A／E 驅動，此處只為讓殼跑得起來。
fn seed_run() -> (IpcHandler, i64) {
    let config = SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![Chips::new(400); 9],
        auto_refill: Some(9),
        hero_seat: HERO_SEAT,
        hand_limit: 500,
        master_seed: 20_260_816,
    };

    let mut store = Store::open_in_memory().expect("建立記憶體資料庫");
    let manifest = build_manifest(&config);
    let run_id = store.create_run(&manifest).expect("建立 run");

    let mut rows = Vec::new();
    let summary = run_session(&config, &mut CallingStation, |played| {
        let record = HandRecord::from_played(played);
        let contributed = played.result.total_contributions[HERO_SEAT];
        let delta = record.hero_delta(HERO_SEAT, contributed);
        rows.push((record, played.seated, delta));
    });
    for chunk in rows.chunks(500) {
        store.write_hands(run_id, chunk).expect("寫入 log");
    }

    let mut final_manifest = build_manifest(&config);
    final_manifest.instances = summary
        .instances
        .iter()
        .map(|i| InstanceRecord {
            index: i.index,
            first_hand: 0,
            last_hand: i.hands.saturating_sub(1),
            hands: i.hands,
            end: format!("{:?}", i.end),
            refills: Vec::new(),
        })
        .collect();
    final_manifest.completed = true;
    store
        .finish_run(run_id, &final_manifest, summary.hands_played)
        .expect("結束 run");

    (IpcHandler::new(store), run_id)
}

fn main() {
    let (handler, run_id) = seed_run();

    tauri::Builder::default()
        .manage(AppState {
            handler: Mutex::new(handler),
            run_id,
        })
        .invoke_handler(tauri::generate_handler![get_run, list_hands, get_hand])
        .run(tauri::generate_context!())
        .expect("啟動 Tauri 應用程式失敗");
}
