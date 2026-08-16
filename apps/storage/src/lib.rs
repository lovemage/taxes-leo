//! 本機儲存層：事件 log、`RunManifest` 與重播資料。
//!
//! 與 `poker-engine` 分開成獨立 crate 的理由：規則引擎維持零外部相依，
//! 測試快且不受套件版本影響；SQLite 只在這一層出現。
//!
//! 規格來源：核心規格 3.3（`RunManifest`）、7.2（載入延遲門檻）、
//! 實做計劃 M0 的 log 容量規格。

pub mod codec;
pub mod db;
pub mod manifest;

pub use codec::{decode, encode, CodecError, HandRecord, RecordedAction, LOG_FORMAT_VERSION};
pub use db::{StorageError, Store};
pub use manifest::{
    ContentSnapshot, ExecutionMode, InstanceRecord, RefillRecord, RuleVariants, RunManifest,
    SCHEMA_VERSION,
};
