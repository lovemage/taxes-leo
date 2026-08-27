//! 由 UI 驅動的執行層測試。
//!
//! 這一層原本寫在 Tauri 桌面殼裡，但 Tauri 在 Linux 編不動，等於完全
//! 無法測試。搬進 workspace 後，設定轉換、暫停語意與進度計算都能驗證，
//! 桌面殼只剩「把 Tauri 的 command 與事件接到這裡」的接線。

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use poker_ipc::run::{execute, RunControl, RunPhase, RunProgress, RunRequest};
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
        rake_basis_points: 0,
        rake_cap_bb: 0,
        rake_no_flop_no_drop: false,
        hand_limit: 2_000,
        master_seed: "20260821".to_owned(),
        hero_seat: 0,
        bots: Vec::new(),
        hero_overrides: Vec::new(),
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

/// 抽水率跨 IPC 用萬分比，因此 4.5% 這類實務常見值不會被壓成整數百分比。
#[test]
fn 抽水率以萬分比原樣傳遞() {
    let mut request = request();
    request.rake_basis_points = 450;
    request.rake_cap_bb = 3;
    let config = request.to_session_config().expect("轉換");
    assert_eq!(config.table.rake.basis_points, 450, "4.5% = 450 萬分比");
    assert_eq!(config.table.rake.cap.units(), 6, "3BB × 每 BB 2 單位");
}

#[test]
fn 抽水率超過百分之百被攔下() {
    let mut request = request();
    request.rake_basis_points = 10_001;
    assert!(request.to_session_config().is_err());
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
    execute(&config, &[], &[], &store, &control, 1_771_200_000, move |progress| {
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

// ── 準備階段（面板 E 的第一秒）────────────────────────────────────────
//
// 這一組守的是實機回報的「按下開始就假死」：舊版在桌面殼啟動時現算
// 內容級 equity 排序（debug 建置 80 秒），而 `execute` 又要等同一份
// `OnceLock`。中間那段時間 UI 收不到任何事件，畫面停在 0%，
// 使用者與 Windows 都判定為當掉。

/// 第一個進度事件必須在**發出任何一手之前**就送出。
#[test]
fn 第一個進度事件是準備階段且尚未發牌() {
    let (updates, _) = run_with(Arc::new(RunControl::default()), 1_000);

    let first = updates.first().expect("至少一次進度");
    assert_eq!(
        first.phase,
        RunPhase::PreparingStrategy,
        "第一個事件必須是準備階段，否則畫面在載入內容那段時間毫無回饋"
    );
    assert_eq!(first.hands_done, 0, "準備階段不得已經發過牌");
    assert!(!first.finished);
    assert!(!first.cancelled);
    assert_eq!(
        first.hands_total, 1_000,
        "準備階段就要帶上總手數，進度條才畫得出分母"
    );
}

/// 準備階段只出現一次，而且在所有跑牌進度之前。
#[test]
fn 準備階段只出現在最前面() {
    let (updates, _) = run_with(Arc::new(RunControl::default()), 2_000);

    let preparing: Vec<usize> = updates
        .iter()
        .enumerate()
        .filter(|(_, p)| p.phase == RunPhase::PreparingStrategy)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(preparing, vec![0], "準備階段只該推一次，而且必須是第一次");

    assert!(
        updates[1..]
            .iter()
            .all(|p| matches!(p.phase, RunPhase::Running | RunPhase::Finished)),
        "準備完成之後不得再回到準備階段"
    );
}

/// 準備階段必須是**毫秒等級**。
///
/// 這正是舊版壞掉的地方：內容級排序現算要 5–80 秒，而那段時間全部落在
/// 「按下開始」與「第一個事件」之間。排序改由離線資產載入之後，這段只剩
/// 解析六千多位元組的純文字。
///
/// 門檻放寬到一秒是給 CI 的慢機器留餘裕；真正要擋下的是**數十秒**那個量級。
#[test]
fn 準備階段在一秒內完成() {
    let mut request = request();
    request.hand_limit = 30_000;
    let config = request.to_session_config().expect("轉換");
    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let control = Arc::new(RunControl::default());
    // 準備階段一結束就取消，避免這個測試真的跑滿三萬手
    let control_for_thread = Arc::clone(&control);

    let started = std::time::Instant::now();
    let first_at = Arc::new(Mutex::new(None::<std::time::Duration>));
    let sink = Arc::clone(&first_at);
    execute(&config, &[], &[], &store, &control, 1_771_200_000, move |progress| {
        if progress.phase == RunPhase::PreparingStrategy {
            *sink.lock().expect("鎖") = Some(started.elapsed());
            control_for_thread.cancelled.store(true, Ordering::Relaxed);
        }
    })
    .expect("執行");

    let elapsed = first_at.lock().expect("鎖").expect("必須推過準備階段");
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "準備階段花了 {elapsed:?}——這條路徑不得有任何現算的 Monte Carlo"
    );
}

/// 30K 手是驗收用的批次規模，必須真的跑得完。
#[test]
fn 三萬手可完整跑完並如實標示階段() {
    let (updates, done) = run_with(Arc::new(RunControl::default()), 30_000);

    assert_eq!(done, 30_000, "30K run 必須跑滿");
    let last = updates.last().expect("至少一次進度");
    assert_eq!(last.phase, RunPhase::Finished);
    assert!(last.finished);
    assert!(!last.cancelled);
    assert_eq!(last.hands_done, 30_000);
}

/// 取消時最終階段是 `Cancelled` 而不是 `Finished`。
///
/// 兩者的 `finished` 都是 true（取消也是一種結束），只有 `phase`
/// 分得出來——前端據此決定要說「已完成」還是「已取消」。
#[test]
fn 取消時最終階段標示為已取消() {
    let control = Arc::new(RunControl::default());
    control.cancelled.store(true, Ordering::Relaxed);
    let (updates, _) = run_with(Arc::clone(&control), 50_000);
    assert_eq!(updates.last().expect("至少一次進度").phase, RunPhase::Cancelled);
}

// ── 執行、進度與取消 ────────────────────────────────────────────────────

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
    // 準備階段與最終進度各算一次，因此中途進度另外數
    let running = updates
        .iter()
        .filter(|p| p.phase == RunPhase::Running)
        .count();
    assert!(running >= 1, "仍須有中途進度，不能只在頭尾各推一次");
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

/// E.6 的總時長要扣掉暫停。
///
/// 這個數字在畫面上會被當成效能訊號讀（「10,000 手 · 0.3 秒」）。把使用者
/// 暫停去做別的事那段算進去，講的就不是引擎的速度而是他離開了多久。
#[test]
fn 總時長扣掉暫停時間() {
    const PAUSE_MS: u64 = 300;

    let control = Arc::new(RunControl::default());
    let control_for_thread = Arc::clone(&control);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(5));
        control_for_thread.paused.store(true, Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(PAUSE_MS));
        control_for_thread.paused.store(false, Ordering::Relaxed);
    });

    let (updates, _) = run_with(Arc::clone(&control), 1_000);
    let last = updates.last().expect("至少一次進度");

    assert!(
        control.paused_millis.load(Ordering::Relaxed) >= PAUSE_MS - 50,
        "暫停時間必須被累計，實際 {} ms",
        control.paused_millis.load(Ordering::Relaxed)
    );
    // 一千手本身遠短於暫停的 300 ms；沒扣掉的話這裡一定超過
    assert!(
        last.elapsed_ms < PAUSE_MS,
        "總時長應扣掉暫停，實際 {} ms",
        last.elapsed_ms
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
    execute(&config, &[], &[], &store, &control, 1_771_200_000, move |progress| {
        // 在進度回呼裡檢查，此時執行緒確實還在 execute 內。
        // 準備階段同樣算執行中——執行權在那時就已經被佔住了
        if !progress.finished {
            sink.lock().expect("鎖").push(control_for_probe.is_active());
        }
    })
    .expect("執行");

    let observed = observed.lock().expect("鎖");
    assert!(!observed.is_empty(), "應有中途進度可供觀察");
    assert!(observed.iter().all(|active| *active), "執行中必須維持佔用");
}

// ── 內容快照（核心規格 3.3）─────────────────────────────────────────────

/// 使用者沒開過 Bot 面板時，manifest 仍必須記下實際在桌上的九個 Bot。
///
/// request 的 `bots` 是空陣列，但引擎會補九份預設設定——manifest 只遍歷
/// 原始請求的話會記成「沒有 Bot」，那份紀錄就無法重現這個 run。
#[test]
fn 未指定_bot_時_manifest_仍記下實際使用的九組設定() {
    let mut request = request();
    request.hand_limit = 200;
    request.bots = Vec::new();
    let config = request.to_session_config().expect("轉換");

    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let control = Arc::new(RunControl::default());
    let run_id = execute(&config, &request.bots, &[], &store, &control, 1_771_200_000, |_| {})
        .expect("執行");

    let manifest = store
        .lock()
        .expect("鎖")
        .load_manifest(run_id)
        .expect("讀 manifest");

    assert_eq!(
        manifest.bot_personas.len(),
        9,
        "桌上有九個座位，快照就該有九份設定"
    );
}

/// 快照必須是**自足的內容**，不是名稱或「與預設的差異」。
///
/// 核心規格 3.3：「內容本身必須保存，只留 hash 不合格。」差異需要配上
/// 當時的預設值才讀得回完整設定，而預設值會隨版本改變。
#[test]
fn manifest_保存全部參數生效值與完整基準內容() {
    use poker_engine::bot::params::{BEHAVIOR_SPECS, PERSONA_SPECS};

    let mut request = request();
    request.hand_limit = 200;
    request.bots = vec![poker_ipc::BotSeatConfig {
        name: "緊凶".to_owned(),
        params: [("rangeWidth".to_owned(), 8_000)].into_iter().collect(),
    }];
    let config = request.to_session_config().expect("轉換");

    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let control = Arc::new(RunControl::default());
    let run_id = execute(&config, &request.bots, &[], &store, &control, 1_771_200_000, |_| {})
        .expect("執行");
    let manifest = store
        .lock()
        .expect("鎖")
        .load_manifest(run_id)
        .expect("讀 manifest");

    // 逐座快照：全部 21 欄都在，不只改過的那一欄
    let first = &manifest.bot_personas[0];
    let params = first.content["params"]
        .as_object()
        .expect("params 應為物件");
    assert_eq!(
        params.len(),
        PERSONA_SPECS.len() + BEHAVIOR_SPECS.len(),
        "應記下全部參數的生效值"
    );
    assert_eq!(params["rangeWidth"], 8_000, "改過的值要記對");
    assert_eq!(
        params["foldDiscipline"], 10_000,
        "沒改過的欄位也要記，否則日後還原不出當初跑了什麼"
    );

    // 基準內容：實際數字要在，不能只有名稱
    let preflop = &manifest.hero_strategy.content["preflop"];
    assert!(
        preflop["openingWidths"].as_object().is_some_and(|m| !m.is_empty()),
        "開牌寬度表必須存進快照"
    );
    assert!(
        preflop["playability"].as_object().is_some_and(|m| !m.is_empty()),
        "可玩性調整必須存進快照"
    );
    assert_eq!(
        preflop["raiseSizesCentiBb"]["open"], 250,
        "加注尺度必須存進快照"
    );
    // 用哪一份 equity 排序跑的必須記下來。只記取樣數不夠：事後看到
    // 「20000」還是得自己去猜那是資產還是現算的
    let content = &manifest.hero_strategy.content;
    assert_eq!(content["equityRankingSamples"], 20_000);
    assert_eq!(content["equityRankingSource"], "asset/v1");
    assert_eq!(content["equityRankingContentGrade"], true);

    assert!(manifest.hero_strategy.verify(), "內容與 hash 必須相符");
}

/// 補位的 Bot 名稱必須可辨識，不能全是「未命名」。
#[test]
fn 未指定的座位以座位序命名() {
    let mut request = request();
    request.hand_limit = 200;
    request.bots = vec![poker_ipc::BotSeatConfig {
        name: "自訂".to_owned(),
        params: std::collections::BTreeMap::new(),
    }];
    let config = request.to_session_config().expect("轉換");

    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let control = Arc::new(RunControl::default());
    let run_id = execute(&config, &request.bots, &[], &store, &control, 1_771_200_000, |_| {})
        .expect("執行");
    let manifest = store
        .lock()
        .expect("鎖")
        .load_manifest(run_id)
        .expect("讀 manifest");

    assert_eq!(manifest.bot_personas[0].name, "自訂");
    assert_eq!(manifest.bot_personas[8].name, "座位 8");
}

// ── 面板 D：自身策略的逐格覆寫 ─────────────────────────────────────────

fn override_of(class: &str, aggressive: u32, call: u32) -> poker_ipc::CellOverrideView {
    poker_ipc::CellOverrideView {
        seated: 9,
        hero: "BTN".to_owned(),
        bucket: "160-240".to_owned(),
        scenario: "unopened".to_owned(),
        class: class.to_owned(),
        aggressive,
        call,
    }
}

/// 覆寫必須進 `hero_strategy` 快照，且**不得**混進 Bot 的內容。
///
/// 核心規格 3.3 要求保存內容本身。使用者的覆寫若只留在他的設定畫面裡，
/// 這個 run 事後就再也還原不出「當初到底跑了什麼策略」。
#[test]
fn 自身策略的覆寫寫進_manifest_且不影響_bot_快照() {
    let mut request = request();
    request.hand_limit = 200;
    request.hero_overrides = vec![override_of("72o", 10_000, 0)];
    let config = request.to_session_config().expect("轉換");

    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let control = Arc::new(RunControl::default());
    let run_id = execute(
        &config,
        &request.bots,
        &request.hero_overrides,
        &store,
        &control,
        1_771_200_000,
        |_| {},
    )
    .expect("執行");
    let manifest = store
        .lock()
        .expect("鎖")
        .load_manifest(run_id)
        .expect("讀 manifest");

    let cells = manifest.hero_strategy.content["preflop"]["cellOverrides"]
        .as_array()
        .expect("覆寫應為陣列");
    assert_eq!(cells.len(), 1, "使用者的覆寫必須進快照");
    assert_eq!(cells[0]["node"], "9max/BTN/160-240/unopened");
    assert_eq!(cells[0]["class"], "72o");
    assert_eq!(cells[0]["aggressive"], 10_000);

    // Bot 用的是沒有覆寫的基準規則
    for persona in &manifest.bot_personas {
        assert!(
            persona.content["params"].is_object(),
            "Bot 快照的形狀不該被自身策略改動"
        );
    }
    assert!(manifest.hero_strategy.verify(), "內容與 hash 必須相符");
}

/// 不合法的覆寫必須**當場失敗**，不得靜默忽略。
///
/// 靜默忽略的後果是使用者以為自己改了一格，引擎照舊走參數，
/// 跑出來的統計對不上他在面板上看到的矩陣。
#[test]
fn 不合法的覆寫讓執行直接失敗() {
    let mut request = request();
    request.hand_limit = 200;
    // 主動 70% ＋ 跟注 50% ＝ 120%
    request.hero_overrides = vec![override_of("AA", 7_000, 5_000)];
    let config = request.to_session_config().expect("轉換");

    let store = Arc::new(Mutex::new(Store::open_in_memory().expect("資料庫")));
    let control = Arc::new(RunControl::default());
    let result = execute(
        &config,
        &request.bots,
        &request.hero_overrides,
        &store,
        &control,
        1_771_200_000,
        |_| {},
    );
    assert!(result.is_err(), "合計超過 100% 的覆寫不得被接受");

    // 節點不存在時同樣要失敗（UTG 不可能面對開牌）
    let mut bad_node = override_of("AA", 10_000, 0);
    bad_node.hero = "UTG".to_owned();
    bad_node.scenario = "vs-open-LJ".to_owned();
    assert!(execute(
        &config,
        &request.bots,
        &[bad_node],
        &store,
        &control,
        1_771_200_000,
        |_| {},
    )
    .is_err());
}
