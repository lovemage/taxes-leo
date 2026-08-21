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

use poker_ipc::{HandSummaryView, HandView, HoleCardVisibility, PowerPreviewView, RunView};
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
        let result = run::execute(&config, &store, &control, created_at, |progress| {
            let _ = app_for_thread.emit("run-progress", &progress);
        });
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
            get_run,
            list_hands,
            get_hand
        ])
        .run(tauri::generate_context!())
        .expect("啟動 Tauri 應用程式失敗");
}
