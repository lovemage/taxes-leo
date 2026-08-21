//! 由 UI 驅動的執行層測試。
//!
//! 這一層原本寫在 Tauri 桌面殼裡，但 Tauri 在 Linux 編不動，等於完全
//! 無法測試。搬進 workspace 後，設定轉換、暫停語意與進度計算都能驗證，
//! 桌面殼只剩「把 Tauri 的 command 與事件接到這裡」的接線。

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use poker_ipc::run::{execute, RunControl, RunProgress, RunRequest};
use poker_storage::Store;

fn request() -> RunRequest {
    RunRequest {
        players: 9,
        auto_refill_enabled: true,
        auto_refill_target: 9,
        starting_stack_bb: 200,
        small_blind: 1,
        big_blind: 2,
        ante_mode: "none".to_owned(),
        ante_amount: 0,
        straddle_mode: "none".to_owned(),
        rake_percent: 0,
        rake_cap_bb: 0,
        rake_no_flop_no_drop: false,
        hand_limit: 2_000,
        master_seed: "20260821".to_owned(),
        hero_seat: 0,
    }
}

// ── 面板 A 的設定轉換 ───────────────────────────────────────────────────

#[test]
fn 設定轉換保留桌型與籌碼深度() {
    let config = request().to_session_config().expect("轉換");
    assert_eq!(config.players, 9);
    assert_eq!(config.auto_refill, Some(9));
    // 200BB × 每 BB 2 個最小單位 = 400
    assert_eq!(config.starting_stacks[0].units(), 400);
    assert_eq!(config.master_seed, 20_260_821);
}

#[test]
fn 關閉自動補位時設定為_none() {
    let mut request = request();
    request.auto_refill_enabled = false;
    let config = request.to_session_config().expect("轉換");
    assert_eq!(config.auto_refill, None);
}

/// straddle 金額依規則細則 2.3 自動計算，前端不傳金額。
#[test]
fn straddle_金額自動計算為兩倍遞增() {
    let mut request = request();
    request.straddle_mode = "double".to_owned();
    let config = request.to_session_config().expect("轉換");

    let amounts: Vec<u64> = config
        .table
        .straddle
        .amounts
        .iter()
        .map(|c| c.units())
        .collect();
    assert_eq!(amounts, vec![4, 8], "首段 2×BB，後段為前段 2 倍");
}

#[test]
fn 抽水百分比轉為萬分比() {
    let mut request = request();
    request.rake_percent = 5;
    request.rake_cap_bb = 3;
    let config = request.to_session_config().expect("轉換");
    assert_eq!(config.table.rake.basis_points, 500, "5% = 500 萬分比");
    assert_eq!(config.table.rake.cap.units(), 6, "3BB × 每 BB 2 單位");
}

#[test]
fn 越界設定被攔下() {
    let mut request = request();
    request.players = 5;
    assert!(
        request.to_session_config().is_err(),
        "6-max 以下不得通過（核心規格 1.2）"
    );

    let mut request = self::request();
    request.master_seed = "not-a-number".to_owned();
    assert!(request.to_session_config().is_err(), "非法 seed 必須攔下");

    let mut request = self::request();
    request.ante_mode = "somethingElse".to_owned();
    assert!(request.to_session_config().is_err(), "未知 ante 模式必須攔下");
}

// ── 執行、進度與取消 ────────────────────────────────────────────────────

fn run_with(control: Arc<RunControl>, hand_limit: u64) -> (Vec<RunProgress>, u64) {
    let mut request = request();
    request.hand_limit = hand_limit;
    let config = request.to_session_config().expect("轉換");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));

    let updates = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&updates);
    execute(&config, &store, &control, 1_771_200_000, move |progress| {
        sink.lock().expect("鎖").push(progress);
    })
    .expect("執行");

    let done = control.hands_done.load(Ordering::Relaxed);
    let updates = Arc::try_unwrap(updates)
        .expect("唯一持有")
        .into_inner()
        .expect("鎖");
    (updates, done)
}

#[test]
fn 執行完成後推送最終進度() {
    let (updates, done) = run_with(Arc::new(RunControl::default()), 2_000);

    assert_eq!(done, 2_000, "應跑滿指定手數");
    let last = updates.last().expect("至少一次進度");
    assert!(last.finished, "最後一次進度必須標示完成");
    assert!(!last.cancelled);
    assert_eq!(last.hands_done, 2_000);
    assert_eq!(last.hands_total, 2_000);
}

/// 進度不得逐手推送，否則 UI 執行緒會被重繪吃滿（核心規格 3.2）。
#[test]
fn 進度更新經過節流而非逐手推送() {
    let (updates, _) = run_with(Arc::new(RunControl::default()), 2_000);
    assert!(
        updates.len() < 50,
        "2000 手不應推送 {} 次進度，必須節流",
        updates.len()
    );
    assert!(updates.len() >= 2, "仍須有中途進度，不能只在結束時推一次");
}

#[test]
fn 取消後停止並標示為已取消() {
    let control = Arc::new(RunControl::default());
    control.cancelled.store(true, Ordering::Relaxed);

    let (updates, done) = run_with(Arc::clone(&control), 100_000);
    assert!(done < 100_000, "取消後不應跑完全部手數");
    let last = updates.last().expect("至少一次進度");
    assert!(last.cancelled, "最終進度必須標示已取消");
    assert!(last.finished, "取消也是一種結束");
}

/// 核心規格 3.2：暫停／續跑不可改變最終結果。
#[test]
fn 暫停續跑不改變最終結果() {
    let plain = Arc::new(RunControl::default());
    let (_, done_plain) = run_with(Arc::clone(&plain), 1_000);

    // 中途暫停再續跑
    let paused = Arc::new(RunControl::default());
    let control_for_thread = Arc::clone(&paused);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(5));
        control_for_thread.paused.store(true, Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(80));
        control_for_thread.paused.store(false, Ordering::Relaxed);
    });
    let (_, done_paused) = run_with(Arc::clone(&paused), 1_000);

    assert_eq!(
        done_plain, done_paused,
        "暫停只讓執行緒停在手與手之間，不得改變完成手數"
    );
}

#[test]
fn 取消可解除暫停的等待() {
    // 暫停中直接取消，執行緒不得卡在等待迴圈
    let control = Arc::new(RunControl::default());
    control.paused.store(true, Ordering::Relaxed);

    let control_for_thread = Arc::clone(&control);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        control_for_thread.cancelled.store(true, Ordering::Relaxed);
        control_for_thread.paused.store(false, Ordering::Relaxed);
    });

    let (updates, _) = run_with(Arc::clone(&control), 50_000);
    assert!(
        updates.last().expect("至少一次進度").cancelled,
        "暫停中取消必須能結束執行"
    );
}

// ── 執行權的釋放 ────────────────────────────────────────────────────────

/// 桌面殼用 `is_active()` 決定要不要拒絕新的 run。
///
/// 這組測試存在的原因是實機回報：第一個 run 正常跑完後，第二次啟動被
/// 「已有 run 正在執行」永久拒絕——因為跑完的 control 其 `cancelled`
/// 仍是 false，只看那個旗標判斷不出「已結束」。
#[test]
fn 正常完成後釋放執行權() {
    let control = Arc::new(RunControl::default());
    assert!(control.is_active(), "尚未執行時視為佔用中，避免重複啟動");

    run_with(Arc::clone(&control), 1_000);

    assert!(
        !control.is_active(),
        "跑完的 run 必須釋放執行權，否則第二個 run 永遠開不起來"
    );
}

#[test]
fn 取消後釋放執行權() {
    let control = Arc::new(RunControl::default());
    control.cancelled.store(true, Ordering::Relaxed);
    run_with(Arc::clone(&control), 10_000);
    assert!(!control.is_active());
}

/// 執行中不得釋放執行權，否則兩個 run 會同時寫入同一個資料庫。
#[test]
fn 執行中維持佔用() {
    let control = Arc::new(RunControl::default());
    let observed = Arc::new(Mutex::new(Vec::new()));

    let mut request = request();
    request.hand_limit = 5_000;
    let config = request.to_session_config().expect("轉換");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));

    let control_for_probe = Arc::clone(&control);
    let sink = Arc::clone(&observed);
    execute(&config, &store, &control, 1_771_200_000, move |progress| {
        // 在進度回呼裡檢查，此時執行緒確實還在 execute 內
        if !progress.finished {
            sink.lock().expect("鎖").push(control_for_probe.is_active());
        }
    })
    .expect("執行");

    let observed = observed.lock().expect("鎖");
    assert!(!observed.is_empty(), "應有中途進度可供觀察");
    assert!(observed.iter().all(|active| *active), "執行中必須維持佔用");
}
