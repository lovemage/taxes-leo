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

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use poker_ipc::{
    CellOverrideView, HandSummaryView, HandView, HoleCardVisibility, PowerPreviewView,
    RangeMatrixView, RunView, StrategyMetaView, StrategyNodesView,
};
use poker_storage::Store;
use poker_ipc::run::{self, RunControl, RunRequest};
use tauri::{Emitter, Manager, State};

/// 應用程式狀態。
///
/// `Store` 由 UI 執行緒與背景執行緒共用，因此包在 `Arc<Mutex<_>>`。
/// 核心規格 3.2 要求批次執行期間仍可瀏覽既有報表與 log，
/// 兩邊必然會同時碰到資料庫。
struct AppState {
    store: Arc<Mutex<Store>>,
    /// 目前檢視中的 run。尚未執行任何 run 時為 `None`
    current_run: Mutex<Option<i64>>,
    control: Mutex<Option<Arc<RunControl>>>,
}

type CommandResult<T> = Result<T, String>;

// ── 執行控制（面板 E）────────────────────────────────────────────────

/// 啟動一個批次 run。立即回傳，實際執行在背景執行緒。
#[tauri::command]
fn start_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: RunRequest,
    created_at: i64,
) -> CommandResult<()> {
    let config = request.to_session_config()?;
    // 逐座 Bot 設定隨 request 一起進來；驗證在這裡就做，
    // 不合法的參數不該等到背景執行緒跑起來才失敗
    let bots = request.bots.clone();
    poker_ipc::bots::to_bot_configs(&bots, config.players)?;
    // 面板 D 的逐格覆寫同樣先驗證。不合法的覆寫若拖到背景執行緒才失敗，
    // 使用者會以為 run 已經帶著他改的策略跑起來了
    let hero_overrides = request.hero_overrides.clone();
    poker_ipc::strategy::to_cell_overrides(&hero_overrides)?;

    // 已有 run 在跑時拒絕啟動，避免兩個 run 同時寫入。
    //
    // 判斷用 `is_active()` 而不是只看 cancelled：正常跑完的 run 其
    // cancelled 仍是 false，只看那個旗標會讓第二次執行永遠被拒
    {
        let guard = state.control.lock().map_err(|_| "狀態鎖已毀損")?;
        if let Some(existing) = guard.as_ref() {
            if existing.is_active() {
                return Err("已有 run 正在執行，請先取消或等待完成".to_owned());
            }
        }
    }

    let control = Arc::new(RunControl::default());
    *state.control.lock().map_err(|_| "狀態鎖已毀損")? = Some(Arc::clone(&control));

    let store = Arc::clone(&state.store);
    let app_for_thread = app.clone();

    std::thread::spawn(move || {
        let result = run::execute(
            &config,
            &bots,
            &hero_overrides,
            &store,
            &control,
            created_at,
            |progress| {
                let _ = app_for_thread.emit("run-progress", &progress);
            },
        );
        match result {
            Ok(run_id) => {
                // run_id 寫回應用程式狀態，後續查詢才知道要看哪個 run
                if let Some(state) = app_for_thread.try_state::<AppState>() {
                    if let Ok(mut slot) = state.current_run.lock() {
                        *slot = Some(run_id);
                    }
                }
                let _ = app_for_thread.emit("run-ready", run_id);
            }
            Err(message) => {
                let _ = app_for_thread.emit("run-failed", message);
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn pause_run(state: State<'_, AppState>, paused: bool) -> CommandResult<()> {
    let guard = state.control.lock().map_err(|_| "狀態鎖已毀損")?;
    if let Some(control) = guard.as_ref() {
        control.paused.store(paused, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
fn cancel_run(state: State<'_, AppState>) -> CommandResult<()> {
    let guard = state.control.lock().map_err(|_| "狀態鎖已毀損")?;
    if let Some(control) = guard.as_ref() {
        control.cancelled.store(true, Ordering::Relaxed);
        // 取消時一併解除暫停，否則執行緒會卡在等待迴圈
        control.paused.store(false, Ordering::Relaxed);
    }
    Ok(())
}

/// 面板 A 的效力預覽。不需要 run，純粹依設定計算。
#[tauri::command]
fn preview_power(hand_limit: u64, players: usize) -> Vec<PowerPreviewView> {
    poker_ipc::views::power_previews(hand_limit, players)
}

// ── Bot 設定（面板 B／C）─────────────────────────────────────────────

/// 21 個 Bot 參數的規格。前端據此渲染欄位，不自行抄一份範圍
#[tauri::command]
fn list_bot_params() -> Vec<poker_ipc::ParamSpecView> {
    poker_ipc::bots::all_specs()
}

/// 工程用示範組合。**不是校準過的人格**，顧問內容進來前的替代品
#[tauri::command]
fn list_bot_presets() -> Vec<poker_ipc::BotSeatConfig> {
    poker_ipc::bots::demo_presets()
}

// ── 策略（面板 D）───────────────────────────────────────────────────

/// 基準內容的來源與現況。UI 據此揭露「未經顧問簽核」與翻後 fallback
#[tauri::command]
fn strategy_meta() -> StrategyMetaView {
    poker_ipc::strategy::meta()
}

/// 某（桌型 × 位置）下到得了的情境與籌碼分檔
#[tauri::command]
fn strategy_nodes(seated: u8, hero: String) -> StrategyNodesView {
    poker_ipc::strategy::nodes(seated, &hero)
}

/// 一個節點的 13×13 範圍矩陣。頻率由引擎算，UI 只負責畫
#[tauri::command]
fn strategy_matrix(
    seated: u8,
    hero: String,
    bucket: String,
    scenario: String,
    overrides: Vec<CellOverrideView>,
) -> CommandResult<RangeMatrixView> {
    poker_ipc::strategy::matrix(seated, &hero, &bucket, &scenario, &overrides)
}

// ── 資料查詢（面板 F／G）─────────────────────────────────────────────

#[tauri::command]
fn get_run(state: State<'_, AppState>) -> CommandResult<RunView> {
    let run_id = state
        .current_run
        .lock()
        .map_err(|_| "狀態鎖已毀損")?
        .ok_or("尚未執行任何 run")?;
    let store = state.store.lock().map_err(|_| "資料庫鎖已毀損")?;
    poker_ipc::views::run_view(&store, run_id).map_err(|e| format!("取得 run 失敗：{e:?}"))
}

#[tauri::command]
fn list_hands(
    state: State<'_, AppState>,
    offset: u64,
    limit: u64,
) -> CommandResult<Vec<HandSummaryView>> {
    let run_id = state
        .current_run
        .lock()
        .map_err(|_| "狀態鎖已毀損")?
        .ok_or("尚未執行任何 run")?;
    let store = state.store.lock().map_err(|_| "資料庫鎖已毀損")?;
    poker_ipc::views::hand_summaries(&store, run_id, offset, limit.min(500))
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
    let run_id = state
        .current_run
        .lock()
        .map_err(|_| "狀態鎖已毀損")?
        .ok_or("尚未執行任何 run")?;
    let store = state.store.lock().map_err(|_| "資料庫鎖已毀損")?;
    poker_ipc::views::hand_view(&store, run_id, index, visibility)
        .map_err(|e| format!("取得手牌失敗：{e:?}"))
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // 落地到 app data 目錄，不用 in-memory。
            //
            // 兩個理由：跑滿 1000K 手的 log 放在記憶體會吃掉數百 MB；
            // 而且 run 的結果必須在關掉視窗後還在，否則核心規格 3.2 的
            // 「事後瀏覽既有報表與 log」形同虛設。
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(data_dir.join("runs.sqlite"))?;
            // 接回上次的 run，否則重開視窗會看不到先前跑完的結果
            let latest = store.latest_run_id()?;
            // equity 排序要算約五秒，面板 D 與第一個 run 都會用到。在背景
            // 先算好，否則使用者點進策略面板會對著空畫面等；`OnceLock`
            // 保證只算一次，先到的執行緒付錢，後到的直接拿
            std::thread::spawn(|| {
                let _ = poker_ipc::rankings::all();
            });
            app.manage(AppState {
                store: Arc::new(Mutex::new(store)),
                current_run: Mutex::new(latest),
                control: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_run,
            pause_run,
            cancel_run,
            preview_power,
            list_bot_params,
            list_bot_presets,
            strategy_meta,
            strategy_nodes,
            strategy_matrix,
            get_run,
            list_hands,
            get_hand
        ])
        .run(tauri::generate_context!())
        .expect("啟動 Tauri 應用程式失敗");
}
