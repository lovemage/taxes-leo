//! 由 UI 驅動的批次執行。
//!
//! 面板 A 的設定經此轉為引擎設定，面板 E 的啟動／暫停／取消由此驅動。
//!
//! **本模組刻意不依賴 Tauri**，與 IPC 契約同一個理由：Tauri 在 Linux
//! 編不動，若把執行邏輯放進桌面殼，設定轉換、暫停語意與進度計算就完全
//! 無法在開發機測試。桌面殼只負責把 Tauri 的 command 與事件接到這裡。
//!
//! # 為什麼要跑在背景執行緒
//!
//! 核心規格 3.2：「批次模擬在背景執行緒跑，不阻塞 UI」，且
//! 「使用者可在運算中瀏覽既有報表與 log」。若在 command 裡同步跑完
//! 100 萬手，視窗會整個凍住十幾個小時。
//!
//! # 為什麼第一個進度事件在做任何事之前就送出
//!
//! 「按下開始」與「第一手跑完」之間，UI 只有一個 0% 的進度條。中間發生
//! 什麼事使用者完全看不到，於是任何一秒的延遲都被讀成「當掉了」。
//! [`execute`] 因此在載入內容之前就先推一次 [`RunPhase::PreparingStrategy`]，
//! 讓畫面在毫秒內就有明確狀態，而不是靠使用者猜。
//!
//! # 暫停與取消不得改變結果
//!
//! 核心規格 3.2：「暫停／續跑不可改變最終結果」。因此暫停只讓執行緒
//! 停在**手與手之間**等待，不介入任何一手的內部狀態；續跑後的 RNG
//! stream 與事件序列與不暫停時完全相同。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use poker_engine::bot::BotAgent;
use poker_engine::chips::Chips;
use poker_engine::pot::RakeConfig;
use poker_engine::rng::RNG_VERSION;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::strategy::baseline::BaselineRules;
use poker_engine::table::{AnteConfig, AnteMode, MuckPolicy, StraddleConfig, TableConfig};
use poker_storage::codec::{HandRecord, LOG_FORMAT_VERSION};
use poker_storage::manifest::{
    ContentSnapshot, ExecutionMode, InstanceRecord, RuleVariants, RunManifest, SCHEMA_VERSION,
};
use poker_storage::Store;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 面板 A 的設定，由前端傳入。
///
/// 欄位語意的權威來源是核心規格 2.1；此處只負責跨 IPC 傳遞。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub players: usize,
    pub auto_refill_enabled: bool,
    pub auto_refill_target: usize,
    /// 起始深度，整數 BB
    pub starting_stack_bb: u64,
    pub small_blind: u64,
    pub big_blind: u64,
    pub ante_mode: String,
    pub ante_amount: u64,
    pub straddle_mode: String,
    /// 抽水率，萬分比（5% = 500）。
    ///
    /// 前端傳萬分比而非百分比：引擎本來就以萬分比表示，實務上的
    /// 4.5% 這類抽水率在整數百分比裡根本表達不出來。前端負責
    /// 「% ↔ 萬分比」的顯示換算，跨 IPC 的一律是整數
    pub rake_basis_points: u32,
    pub rake_cap_bb: u64,
    pub rake_no_flop_no_drop: bool,
    /// 手數，以 10K 為單位傳入實際手數
    pub hand_limit: u64,
    /// master seed。字串傳遞，避免 u64 在 JS 失去精度
    pub master_seed: String,
    pub hero_seat: usize,
    /// 逐座 Bot 設定（面板 B／C）。長度不足時以預設補齊
    #[serde(default)]
    pub bots: Vec<crate::bots::BotSeatConfig>,
    /// 面板 D 的自身策略逐格覆寫。**只裝在使用者座位上**
    #[serde(default)]
    pub hero_overrides: Vec<crate::strategy::CellOverrideView>,
}

impl RunRequest {
    /// 轉為引擎設定。
    ///
    /// # Errors
    /// 欄位越界或 straddle 不合規則時回傳說明。
    pub fn to_session_config(&self) -> Result<SessionConfig, String> {
        let big_blind = self.big_blind.max(1);
        let ante_mode = match self.ante_mode.as_str() {
            "none" => AnteMode::None,
            "perPlayer" => AnteMode::PerPlayer,
            "bbAnte" => AnteMode::BbAnte,
            "btnAnte" => AnteMode::BtnAnte,
            other => return Err(format!("未知的 ante 模式：{other}")),
        };

        // straddle 金額依規則細則 2.3 自動計算：首段 2×BB，後段為前段 2 倍
        let straddle = match self.straddle_mode.as_str() {
            "none" => StraddleConfig::default(),
            "single" => StraddleConfig {
                seats: vec![2],
                amounts: vec![Chips::new(big_blind * 2)],
            },
            "double" => StraddleConfig {
                seats: vec![2, 3],
                amounts: vec![Chips::new(big_blind * 2), Chips::new(big_blind * 4)],
            },
            other => return Err(format!("未知的 straddle 模式：{other}")),
        };
        straddle
            .validate(Chips::new(big_blind))
            .map_err(|e| format!("straddle 設定不合規則：{e:?}"))?;

        let table = TableConfig {
            small_blind: Chips::new(self.small_blind),
            big_blind: Chips::new(big_blind),
            ante: AnteConfig {
                mode: ante_mode,
                amount: Chips::new(self.ante_amount),
            },
            straddle,
            rake: RakeConfig {
                basis_points: self.rake_basis_points,
                cap: Chips::new(self.rake_cap_bb * big_blind),
                no_flop_no_drop: self.rake_no_flop_no_drop,
            },
            muck: MuckPolicy::Realistic,
        };

        if self.rake_basis_points > 10_000 {
            return Err(format!(
                "抽水率不得超過 100%（收到 {} 萬分比）",
                self.rake_basis_points
            ));
        }

        let master_seed: u64 = self
            .master_seed
            .parse()
            .map_err(|_| format!("seed 不是合法的整數：{}", self.master_seed))?;

        let config = SessionConfig {
            table,
            players: self.players,
            starting_stacks: vec![Chips::new(self.starting_stack_bb * big_blind); self.players],
            auto_refill: self.auto_refill_enabled.then_some(self.auto_refill_target),
            hero_seat: self.hero_seat,
            hand_limit: self.hand_limit,
            master_seed,
        };
        config
            .validate()
            .map_err(|e| format!("設定不合法：{e:?}"))?;
        Ok(config)
    }
}

/// 執行所處的階段。
///
/// 有這個欄位之前，前端分不出「正在準備內容」與「已經卡死」——兩者
/// 在畫面上都是一條不動的 0% 進度條。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub enum RunPhase {
    /// 載入 equity 排序、驗證設定、建立 run 紀錄。**尚未發出任何一手**
    PreparingStrategy,
    /// 正在跑牌
    Running,
    Finished,
    Cancelled,
}

/// 執行進度，推送給前端。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RunProgress {
    #[ts(type = "number")]
    pub hands_done: u64,
    #[ts(type = "number")]
    pub hands_total: u64,
    #[ts(type = "number")]
    pub instances: u64,
    /// 目前的 bb/100 點估計
    pub bb_per_100: f64,
    pub paused: bool,
    pub finished: bool,
    pub cancelled: bool,
    /// 目前階段。前端據此顯示「準備內容中」而不是一條不動的 0%
    pub phase: RunPhase,
    /// 實際計算耗時（毫秒），**已扣掉暫停**。
    ///
    /// UI 規格 E.6 要求完成時顯示總時長。扣掉暫停是因為這個數字會被當成
    /// 效能訊號讀：把使用者去泡咖啡的五分鐘算進去，講的就不是引擎的速度。
    #[ts(type = "number")]
    pub elapsed_ms: u64,
}

/// 背景執行的控制握把。
#[derive(Debug, Default)]
pub struct RunControl {
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
    /// 背景執行緒已離開 [`execute`]，不論是跑完、被取消還是出錯。
    ///
    /// 沒有這個旗標的話，正常跑完的 run 其 `cancelled` 仍是 false，
    /// 下一次啟動會被誤判為「已有 run 正在執行」而永久拒絕
    pub finished: AtomicBool,
    pub hands_done: AtomicU64,
    /// 暫停累計的毫秒數。由 [`RunControl::checkpoint`] 累加，
    /// 從總時長扣掉之後才是實際計算耗時
    pub paused_millis: AtomicU64,
}

impl RunControl {
    /// 這個 run 是否仍佔用執行權。
    ///
    /// 呼叫端據此決定要不要拒絕新的 run。判斷放在這裡而不是桌面殼裡，
    /// 是因為桌面殼在 Linux 編不動，寫在那邊就測不到。
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.finished.load(Ordering::Relaxed) && !self.cancelled.load(Ordering::Relaxed)
    }

    /// 於手與手之間檢查暫停與取消。
    ///
    /// 回傳 `false` 代表應中止。暫停時在此自旋等待，
    /// **不介入任何一手的內部狀態**，因此續跑不改變結果。
    pub fn checkpoint(&self) -> bool {
        // 沒暫停是絕大多數的情形，先擋掉才不會每手都取一次時間
        if !self.paused.load(Ordering::Relaxed) {
            return !self.cancelled.load(Ordering::Relaxed);
        }

        let paused_at = std::time::Instant::now();
        while self.paused.load(Ordering::Relaxed) {
            if self.cancelled.load(Ordering::Relaxed) {
                self.accumulate_paused(paused_at);
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.accumulate_paused(paused_at);
        !self.cancelled.load(Ordering::Relaxed)
    }

    fn accumulate_paused(&self, since: std::time::Instant) {
        let millis = u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.paused_millis.fetch_add(millis, Ordering::Relaxed);
    }
}

/// 實際計算耗時：牆鐘時間扣掉暫停。
fn compute_ms(started: std::time::Instant, control: &RunControl) -> u64 {
    let wall = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    wall.saturating_sub(control.paused_millis.load(Ordering::Relaxed))
}

/// 執行一個 run 並落 log。
///
/// 批次執行每幾手推一次進度事件。
///
/// UI 規格 E.3：「進度更新頻率上限為每秒數次。逐手更新會讓 UI 執行緒被
/// 寫入與重繪吃滿。」狀態列會顯示這個值，因此不寫成字面量——兩邊各寫
/// 一份的話，改了一邊就等於在狀態列上對使用者說謊。
pub const PROGRESS_EVERY_HANDS: u64 = 250;

/// 逐手 log 每幾手 commit 一次（核心規格 3.2：批次交易，避免 UI 餓死）。
pub const WRITE_BATCH_HANDS: usize = 500;

/// `on_progress` 由呼叫端節流後推送給前端；核心規格要求進度更新
/// 不得逐手觸發，否則 UI 執行緒會被重繪吃滿。
pub fn execute(
    config: &SessionConfig,
    bots: &[crate::bots::BotSeatConfig],
    hero_overrides: &[crate::strategy::CellOverrideView],
    store: &Arc<Mutex<Store>>,
    control: &Arc<RunControl>,
    created_at: i64,
    mut on_progress: impl FnMut(RunProgress),
) -> Result<i64, String> {
    // 不論走哪條路徑離開（跑完、取消、中途錯誤、panic），這個 control
    // 都不再代表「進行中的 run」。用 guard 而非在每個 return 前手動標記，
    // 是因為漏掉任何一條路徑的後果都是「之後再也開不了新的 run」
    let _guard = FinishGuard(control);
    let started = std::time::Instant::now();

    // 在做任何事之前先讓畫面有東西看。這一行的成本是一個事件，
    // 換掉的是「按下開始之後什麼都沒發生」那段沉默
    on_progress(RunProgress {
        hands_done: 0,
        hands_total: config.hand_limit,
        instances: 0,
        bb_per_100: 0.0,
        paused: false,
        finished: false,
        cancelled: false,
        phase: RunPhase::PreparingStrategy,
        elapsed_ms: compute_ms(started, control),
    });

    let rules = BaselineRules::engineering_placeholder();
    let bot_configs = crate::bots::to_bot_configs(bots, config.players)?;
    // 面板 D 的逐格覆寫。驗證在這裡就做完，不合法的覆寫不該等到跑起來
    // 才被靜默忽略——使用者以為改了一格，實際上引擎照舊走參數
    let hero_overrides = crate::strategy::to_cell_overrides(hero_overrides)?;
    // 快照必須是**使用者實際用的那份策略**，因此帶著覆寫；Bot 用的是
    // 沒有覆寫的基準規則，兩者分開存才讀得回當初到底跑了什麼
    let mut hero_rules = rules.clone();
    hero_rules.overrides = hero_overrides.clone();

    // 內容在這裡載入，而不是在桌面殼啟動時預熱。載入失敗就當場結束，
    // 不得拿一份殘缺的排序跑完一整晚才發現統計是壞的
    let rankings = crate::rankings::load().map_err(|reason| {
        crate::log::error(&format!("run 無法啟動：{reason}"));
        format!("equity 排序內容不可用：{reason}")
    })?;
    if !rankings.is_content_grade() {
        crate::log::warn(&format!(
            "本 run 使用非內容級排序（{} 取樣，來源 {}）。結果不得作為統計依據",
            rankings.samples(),
            rankings.source().key()
        ));
    }

    let manifest = build_manifest(config, &rules, &hero_rules, &bot_configs, rankings, created_at);
    let run_id = {
        let mut guard = store.lock().map_err(|_| "資料庫鎖已毀損")?;
        guard
            .create_run(&manifest)
            .map_err(|e| format!("建立 run 失敗：{e:?}"))?
    };

    let hero = config.hero_seat;
    let big_blind = f64::from(u32::try_from(config.table.big_blind.units()).unwrap_or(2));
    let mut pending: Vec<(HandRecord, usize, i64)> = Vec::new();
    let mut delta_sum = 0.0f64;
    let mut aborted = false;

    let mut agent = BotAgent::new(
        rules.clone(),
        rankings.table().clone(),
        bot_configs.clone(),
        config.master_seed,
    );
    // 覆寫在 rangeWidth 縮放之後才裝上：覆寫是絕對頻率，被人格參數
    // 再乘一次就不是使用者寫下的那個數字了
    agent.set_seat_overrides(hero, hero_overrides);

    let summary = run_session(config, &mut agent, |played| {
        if aborted || !control.checkpoint() {
            aborted = true;
            return;
        }

        let record = HandRecord::from_played(played);
        let contributed = played.result.total_contributions[hero];
        let delta = record.hero_delta(hero, contributed);
        delta_sum += f64::from(i32::try_from(delta).unwrap_or(0)) / big_blind;
        pending.push((record, played.seated, delta));

        let done = control.hands_done.fetch_add(1, Ordering::Relaxed) + 1;

        // 批次交易寫入（核心規格 3.2：避免逐手 commit 讓 UI 餓死）
        if pending.len() >= WRITE_BATCH_HANDS {
            if let Ok(mut guard) = store.lock() {
                let _ = guard.write_hands(run_id, &pending);
            }
            pending.clear();
        }

        // 進度節流：每 PROGRESS_EVERY_HANDS 手一次，而非逐手
        if done % PROGRESS_EVERY_HANDS == 0 {
            on_progress(RunProgress {
                hands_done: done,
                hands_total: config.hand_limit,
                instances: played.instance_index + 1,
                bb_per_100: if done == 0 {
                    0.0
                } else {
                    delta_sum / f64::from(u32::try_from(done).unwrap_or(1)) * 100.0
                },
                paused: false,
                finished: false,
                cancelled: false,
                phase: RunPhase::Running,
                elapsed_ms: compute_ms(started, control),
            });
        }
    });

    if !pending.is_empty() {
        if let Ok(mut guard) = store.lock() {
            let _ = guard.write_hands(run_id, &pending);
        }
    }

    let mut final_manifest =
        build_manifest(config, &rules, &hero_rules, &bot_configs, rankings, created_at);
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
    final_manifest.completed = !aborted;

    {
        let mut guard = store.lock().map_err(|_| "資料庫鎖已毀損")?;
        guard
            .finish_run(run_id, &final_manifest, summary.hands_played)
            .map_err(|e| format!("結束 run 失敗：{e:?}"))?;
    }

    let done = control.hands_done.load(Ordering::Relaxed);
    // 先標記再送事件：前端一收到 finished 就可能立刻按下一個 run，
    // 順序反過來會有一小段時間新 run 被誤拒
    control.finished.store(true, Ordering::Relaxed);
    on_progress(RunProgress {
        hands_done: done,
        hands_total: config.hand_limit,
        instances: summary.instances.len() as u64,
        bb_per_100: if done == 0 {
            0.0
        } else {
            delta_sum / f64::from(u32::try_from(done).unwrap_or(1)) * 100.0
        },
        paused: false,
        finished: true,
        cancelled: aborted,
        phase: if aborted {
            RunPhase::Cancelled
        } else {
            RunPhase::Finished
        },
        elapsed_ms: compute_ms(started, control),
    });

    Ok(run_id)
}

/// 離開 [`execute`] 時標記 control 已結束。
struct FinishGuard<'a>(&'a RunControl);

impl Drop for FinishGuard<'_> {
    fn drop(&mut self) {
        self.0.finished.store(true, Ordering::Relaxed);
    }
}

fn build_manifest(
    config: &SessionConfig,
    rules: &BaselineRules,
    // 使用者座位實際生效的規則（基準 ＋ 面板 D 的逐格覆寫）
    hero_rules: &BaselineRules,
    bots: &[poker_engine::bot::BotConfig],
    // 實際載入的 equity 排序。記來源與等級而不只是取樣數：用 debug 替代品
    // 跑出來的 run，事後必須看得出來它不是正式內容
    rankings: &crate::rankings::Rankings,
    created_at: i64,
) -> RunManifest {
    RunManifest {
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: SCHEMA_VERSION,
        log_format_version: LOG_FORMAT_VERSION,
        rng_algorithm: RNG_VERSION.to_owned(),
        // 發牌用手序，策略取樣用全域決策序號——兩者是不同的 stream 索引，
        // 寫成同一句會讓日後想重現的人找錯地方
        stream_derivation: "splitmix64(master_seed, index, domain) → xoshiro256**；             deal 的 index 為手序，strategyMix 的 index 為全域決策序號"
            .to_owned(),
        master_seed: config.master_seed,
        execution_mode: ExecutionMode::Batch,
        hand_limit: config.hand_limit,
        players: config.players,
        hero_seat: config.hero_seat,
        starting_stacks: config.starting_stacks.iter().map(|c| c.units()).collect(),
        small_blind: config.table.small_blind.units(),
        big_blind: config.table.big_blind.units(),
        ante_mode: format!("{:?}", config.table.ante.mode),
        ante_amount: config.table.ante.amount.units(),
        straddle_amounts: config
            .table
            .straddle
            .amounts
            .iter()
            .map(|c| c.units())
            .collect(),
        rake_basis_points: config.table.rake.basis_points,
        rake_cap: config.table.rake.cap.units(),
        rake_no_flop_no_drop: config.table.rake.no_flop_no_drop,
        stack_policy: "bustOut".to_owned(),
        auto_refill_target: config.auto_refill,
        rule_variants: RuleVariants::default(),
        // 核心規格 3.3：內容本身必須保存，只留 hash 不合格。
        // 因此存的是整份規則與逐座全部 21 個生效值，不是名稱或差異
        hero_strategy: ContentSnapshot::new(
            hero_rules.name.clone(),
            hero_rules.version.clone(),
            serde_json::json!({
                "preflop": crate::snapshot::baseline(hero_rules),
                "postflopBaseline": {
                    "version": poker_engine::bot::POSTFLOP_BASELINE_VERSION,
                    "consultantApproved": false,
                    "equitySamples": poker_engine::bot::POSTFLOP_EQUITY_SAMPLES,
                    "opponentRange": "uniformRandomLegalHands",
                    "decisionBuckets": "fairShare+potOdds/v1",
                    "betSizesPercent": [50, 66, 75],
                },
                "postflopFallback": poker_engine::bot::POSTFLOP_FALLBACK_VERSION,
                "postflopNote": "翻後採 equity heuristic 工程基準，會依牌力、對手數與底池賠率行動；未經顧問簽核",
                "equityRankingSamples": rankings.samples(),
                "equityRankingSource": rankings.source().key(),
                "equityRankingContentGrade": rankings.is_content_grade(),
            }),
        ),
        // 存的是**實際送進引擎**的設定。使用者沒開過 Bot 面板時
        // request 的 bots 是空的，但桌上仍然坐著九個用預設值的 Bot——
        // 記空陣列等於謊稱這個 run 沒有 Bot
        bot_personas: bots
            .iter()
            .map(|bot| {
                ContentSnapshot::new(
                    bot.name.clone(),
                    rules.version.clone(),
                    crate::snapshot::bot(bot),
                )
            })
            .collect(),
        baseline_version: rules.version.clone(),
        instances: Vec::new(),
        created_at,
        completed: false,
        checkpoint_version: 1,
    }
}
