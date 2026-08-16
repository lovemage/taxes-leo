//! `RunManifest`：一次執行的不可變設定快照。
//!
//! 核心規格 3.3 明列必要欄位，並特別要求：
//! 「重播以已保存的牌局事件為主，不重新執行策略決策。若為節省容量而重算
//! 衍生數值，必須能由同一 `RunManifest` 與仍受支援的版本完全重建；
//! **只有 hash 而沒有內容快照不合格**。」
//!
//! 因此策略與 Bot 設定一律保存**完整內容**，hash 只作為完整性校驗，
//! 不能取代內容。

use serde::{Deserialize, Serialize};

use crate::codec::LOG_FORMAT_VERSION;

/// schema 版本。資料表結構變更時遞增。
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionMode {
    /// 互動對打
    Interactive,
    /// 批次模擬
    Batch,
}

/// 內容快照：完整內容 ＋ 校驗用 hash。
///
/// 核心規格 3.3 要求內容本身必須保存，hash 單獨存在不合格，
/// 因此 `content` 不是 `Option`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSnapshot {
    pub name: String,
    pub version: String,
    /// 完整內容（策略 JSON、persona 參數等）
    pub content: serde_json::Value,
    /// 內容的 FNV-1a 64 位元 hash，僅供完整性校驗
    pub content_hash: String,
}

impl ContentSnapshot {
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>, content: serde_json::Value) -> Self {
        let serialized = content.to_string();
        Self {
            name: name.into(),
            version: version.into(),
            content_hash: fnv1a_hex(serialized.as_bytes()),
            content,
        }
    }

    /// 校驗內容與 hash 是否相符。
    #[must_use]
    pub fn verify(&self) -> bool {
        fnv1a_hex(self.content.to_string().as_bytes()) == self.content_hash
    }
}

/// FNV-1a 64 位元。演算法簡單且結果穩定，適合當長期不變的內容指紋。
#[must_use]
pub fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 桌次邊界紀錄（核心規格 3.3）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRecord {
    pub index: u64,
    pub first_hand: u64,
    pub last_hand: u64,
    pub hands: u64,
    /// 結束原因：heroBusted／notEnoughPlayers／handLimitReached
    pub end: String,
    pub refills: Vec<RefillRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefillRecord {
    pub hand_index: u64,
    pub seat: usize,
    pub buy_in: u64,
}

/// 規則變體紀錄。
///
/// 規則細則第七章：明確不實作的現實規則「必須在 `RunManifest` 的規則變體
/// 欄位留下明確紀錄，不得以『預設行為』帶過」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleVariants {
    pub burn_card: bool,
    pub run_it_twice: bool,
    pub chop_the_blinds: bool,
    pub time_bank: bool,
    /// realistic／alwaysShow
    pub muck_policy: String,
}

impl Default for RuleVariants {
    /// 規則細則第七章的現行決定：這些現實規則一律不實作。
    fn default() -> Self {
        Self {
            burn_card: false,
            run_it_twice: false,
            chop_the_blinds: false,
            time_bank: false,
            muck_policy: "realistic".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunManifest {
    // ── 版本與格式 ──
    pub engine_version: String,
    pub schema_version: u32,
    pub log_format_version: u16,
    pub rng_algorithm: String,
    /// stream 派生規則的文字描述，供跨版本重建時比對
    pub stream_derivation: String,

    // ── 執行參數 ──
    pub master_seed: u64,
    pub execution_mode: ExecutionMode,
    pub hand_limit: u64,

    // ── 桌型與籌碼快照 ──
    pub players: usize,
    pub hero_seat: usize,
    pub starting_stacks: Vec<u64>,
    pub small_blind: u64,
    pub big_blind: u64,
    pub ante_mode: String,
    pub ante_amount: u64,
    pub straddle_amounts: Vec<u64>,
    pub rake_basis_points: u32,
    pub rake_cap: u64,
    pub rake_no_flop_no_drop: bool,
    pub stack_policy: String,
    pub auto_refill_target: Option<usize>,
    pub rule_variants: RuleVariants,

    // ── 策略與 Bot 內容快照 ──
    pub hero_strategy: ContentSnapshot,
    pub bot_personas: Vec<ContentSnapshot>,
    pub baseline_version: String,

    // ── 桌次邊界與狀態 ──
    pub instances: Vec<InstanceRecord>,
    /// 建立時間（Unix 秒）。由呼叫端提供，引擎自身不讀系統時鐘，
    /// 以免時間進入可重現路徑
    pub created_at: i64,
    pub completed: bool,
    pub checkpoint_version: u32,
}

impl RunManifest {
    /// 檢查必要欄位是否齊備（核心規格 3.3）。
    ///
    /// # Errors
    /// 缺漏欄位或內容快照與 hash 不符時回傳說明。
    pub fn validate(&self) -> Result<(), String> {
        if self.rng_algorithm.is_empty() {
            return Err("缺少 RNG 演算法版本".to_owned());
        }
        if self.starting_stacks.len() != self.players {
            return Err("起始籌碼快照與座位數不符".to_owned());
        }
        if !self.hero_strategy.verify() {
            return Err("使用者策略的內容與 hash 不符".to_owned());
        }
        for persona in &self.bot_personas {
            if !persona.verify() {
                return Err(format!("Bot 人格 {} 的內容與 hash 不符", persona.name));
            }
        }
        if self.log_format_version != LOG_FORMAT_VERSION {
            return Err(format!(
                "log 格式版本 {} 與目前支援的 {LOG_FORMAT_VERSION} 不符",
                self.log_format_version
            ));
        }
        Ok(())
    }
}
