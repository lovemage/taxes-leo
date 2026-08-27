//! 執行環境的現況，供 UI 規格 V.1 的 status bar 顯示。
//!
//! 這裡的每一個欄位都是「跑起來之後才知道、而且會影響資料能不能互相比較」
//! 的東西：引擎版本、RNG 演算法、儲存格式版本、批次執行的節流設定。
//!
//! 刻意不從 UI 端硬寫。核心規格 3.3 要求跨 engine 版本的 run 不得合併統計，
//! 前端若自己抄一份版本號，抄錯的那一刻使用者就會拿不可比的資料做決策。

use poker_engine::rng::RNG_VERSION;
use poker_storage::{SCHEMA_VERSION, LOG_FORMAT_VERSION};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::run::{PROGRESS_EVERY_HANDS, WRITE_BATCH_HANDS};

/// status bar 的四段內容：引擎 · SQLite · 動畫模式 · 批次 UI 更新頻率。
///
/// 動畫模式不在這裡——它是模式的函數（E.1：批次模擬不播放），由前端依
/// 目前模式決定，不需要往返一趟 IPC。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatusView {
    /// 引擎版本。跨版本的 run 不得合併統計（核心規格 3.3）
    pub engine_version: String,
    /// RNG 演算法與版本，與 `RunManifest.rngAlgorithm` 同一個字串
    pub rng_algorithm: String,
    /// SQLite schema 版本。與資料庫內的值不符時開不起來
    pub schema_version: u32,
    /// 逐手 log 的二進位格式版本
    pub log_format_version: u16,
    /// equity 排序來源：`asset/v1`／`debugFallback`／`unavailable`
    pub ranking_source: String,
    /// 排序是否為正式內容。false 代表畫面上的範圍不可作為統計依據
    pub ranking_content_grade: bool,
    /// 批次執行每幾手推一次進度事件（UI 規格 E.3）
    #[ts(type = "number")]
    pub progress_every_hands: u64,
    /// 逐手 log 每幾手 commit 一次（核心規格 3.2 的批次交易）
    #[ts(type = "number")]
    pub write_batch_hands: u64,
}

pub fn status() -> RuntimeStatusView {
    let rankings = crate::rankings::status();
    RuntimeStatusView {
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        rng_algorithm: RNG_VERSION.to_owned(),
        schema_version: SCHEMA_VERSION,
        log_format_version: LOG_FORMAT_VERSION,
        ranking_source: rankings.source.to_owned(),
        ranking_content_grade: rankings.content_grade,
        progress_every_hands: PROGRESS_EVERY_HANDS,
        write_batch_hands: WRITE_BATCH_HANDS as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 版本欄位空字串等於沒說，UI 會畫出一條看不出所以然的狀態列
    #[test]
    fn 版本欄位都有值() {
        let s = status();
        assert!(!s.engine_version.is_empty());
        assert!(!s.rng_algorithm.is_empty());
        assert!(!s.ranking_source.is_empty());
    }

    /// 節流設定必須與 run.rs 實際用的常數同源，否則狀態列講的是另一回事
    #[test]
    fn 節流設定與執行層同源() {
        let s = status();
        assert_eq!(s.progress_every_hands, PROGRESS_EVERY_HANDS);
        assert_eq!(s.write_batch_hands, WRITE_BATCH_HANDS as u64);
        assert!(s.progress_every_hands > 0, "每 0 手推一次等於逐手推");
    }
}
