//! UI 與引擎之間的型別化 IPC 契約。
//!
//! 實做計劃鐵則 6：「牌局邏輯只在引擎執行，UI 永不放遊戲邏輯。」
//! 本 crate 是兩者之間唯一的資料通道定義：
//!
//! - **型別單一來源**：DTO 定義在 Rust，前端 TS 型別由 `ts-rs` 產生
//!   （見 `tests/export_bindings.rs`），兩邊不各自手寫。
//! - **隱藏資訊在邊界遮蔽**：未依現實規則亮出的底牌不會進入 DTO，
//!   UI 拿不到就不可能外洩（核心規格 2.4）。
//!
//! 這一層刻意不依賴 Tauri。M3 接上 Tauri command 時，command 只是薄殼，
//! 呼叫這裡的 handler 並把結果序列化，因此 IPC 契約可在無 GUI 環境下
//! 完整測試。

pub mod handler;
pub mod view;

pub use handler::{IpcError, IpcHandler};
pub use view::{
    ActionView, HandSummaryView, HandView, HoleCardVisibility, RunView, SeatView, StreetView,
};
