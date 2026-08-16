//! 9-max 德州撲克模擬平台的規則引擎。
//!
//! 本 crate 是牌局的**唯一權威**（實做計劃第一章鐵則 2）。牌局規則、合法行動、
//! 邊池、抽水、Bot 決策與統計只在這裡執行；UI 透過 IPC 取得結果，不複製任何
//! 遊戲邏輯。
//!
//! 規格來源：
//! - [`9max平台核心規格.md`] — 產品邊界、桌型、統計、效能
//! - [`德州撲克規則細則.md`] — 牌局規則（完全比照現實德州撲克，TDA 標準）
//!
//! 規則層一律使用整數最小籌碼單位，浮點只出現在統計輸出。

pub mod betting;
pub mod chips;
pub mod pot;

pub use betting::{Action, BettingRound, LegalActions, RaiseRange, SeatState};
pub use chips::Chips;
pub use pot::{Distribution, OddChipAward, PotLayer, RakeConfig};
