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
//! # 暫停與取消不得改變結果
//!
//! 核心規格 3.2：「暫停／續跑不可改變最終結果」。因此暫停只讓執行緒
//! 停在**手與手之間**等待，不介入任何一手的內部狀態；續跑後的 RNG
//! stream 與事件序列與不暫停時完全相同。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use poker_engine::betting::Action;
use poker_engine::chips::Chips;
use poker_engine::pot::RakeConfig;
use poker_engine::hand::ActionProvider;
use poker_engine::rng::RNG_VERSION;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::strategy::DecisionView;
use poker_engine::table::{AnteConfig, AnteMode, MuckPolicy, StraddleConfig, TableConfig};
use poker_storage::codec::{HandRecord, LOG_FORMAT_VERSION};
use poker_storage::manifest::{
    ContentSnapshot, ExecutionMode, InstanceRecord, RuleVariants, RunManifest, SCHEMA_VERSION,
};
use poker_storage::Store;
use serde::{Deserialize, Serialize};

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
    pub rake_percent: u32,
    pub rake_cap_bb: u64,
    pub rake_no_flop_no_drop: bool,
    /// 手數，以 10K 為單位傳入實際手數
    pub hand_limit: u64,
    /// master seed。字串傳遞，避免 u64 在 JS 失去精度
    pub master_seed: String,
    pub hero_seat: usize,
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
                // 前端傳百分比，引擎用萬分比
                basis_points: self.rake_percent * 100,
                cap: Chips::new(self.rake_cap_bb * big_blind),
                no_flop_no_drop: self.rake_no_flop_no_drop,
            },
            muck: MuckPolicy::Realistic,
        };

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

/// 執行進度，推送給前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunProgress {
    pub hands_done: u64,
    pub hands_total: u64,
    pub instances: u64,
    /// 目前的 bb/100 點估計
    pub bb_per_100: f64,
    pub paused: bool,
    pub finished: bool,
    pub cancelled: bool,
}

/// 背景執行的控制握把。
#[derive(Debug, Default)]
pub struct RunControl {
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
    pub hands_done: AtomicU64,
}

impl RunControl {
    /// 於手與手之間檢查暫停與取消。
    ///
    /// 回傳 `false` 代表應中止。暫停時在此自旋等待，
    /// **不介入任何一手的內部狀態**，因此續跑不改變結果。
    pub fn checkpoint(&self) -> bool {
        while self.paused.load(Ordering::Relaxed) {
            if self.cancelled.load(Ordering::Relaxed) {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        !self.cancelled.load(Ordering::Relaxed)
    }
}

/// 執行一個 run 並落 log。
///
/// `on_progress` 由呼叫端節流後推送給前端；核心規格要求進度更新
/// 不得逐手觸發，否則 UI 執行緒會被重繪吃滿。
pub fn execute(
    config: &SessionConfig,
    store: &Arc<Mutex<Store>>,
    control: &Arc<RunControl>,
    created_at: i64,
    mut on_progress: impl FnMut(RunProgress),
) -> Result<i64, String> {
    let manifest = build_manifest(config, created_at);
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

    let summary = run_session(config, &mut CallingStation, |played| {
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
        if pending.len() >= 500 {
            if let Ok(mut guard) = store.lock() {
                let _ = guard.write_hands(run_id, &pending);
            }
            pending.clear();
        }

        // 進度節流：每 250 手一次，而非逐手
        if done % 250 == 0 {
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
            });
        }
    });

    if !pending.is_empty() {
        if let Ok(mut guard) = store.lock() {
            let _ = guard.write_hands(run_id, &pending);
        }
    }

    let mut final_manifest = build_manifest(config, created_at);
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
    });

    Ok(run_id)
}

fn build_manifest(config: &SessionConfig, created_at: i64) -> RunManifest {
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
        hero_strategy: ContentSnapshot::new(
            "示範策略",
            "v0",
            serde_json::json!({ "note": "顧問內容就緒前的佔位" }),
        ),
        bot_personas: Vec::new(),
        baseline_version: "none".to_owned(),
        instances: Vec::new(),
        created_at,
        completed: false,
        checkpoint_version: 1,
    }
}

/// 示範用的行動來源。M2 內容就緒後由使用者策略與 Bot 決策取代。
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
