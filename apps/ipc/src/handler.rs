//! IPC 指令處理。
//!
//! 每個方法對應 M3 的一個 Tauri command。這裡不依賴 Tauri，
//! command 只會是薄殼：解析參數 → 呼叫這裡 → 序列化結果。

use poker_storage::db::{StorageError, Store};

use crate::view::{HandSummaryView, HandView, HoleCardVisibility, RunView};

#[derive(Debug)]
pub enum IpcError {
    Storage(StorageError),
    NotFound,
}

impl From<StorageError> for IpcError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::NotFound => Self::NotFound,
            other => Self::Storage(other),
        }
    }
}

pub struct IpcHandler {
    store: Store,
}

impl IpcHandler {
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    #[must_use]
    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// 取得 run 摘要。
    ///
    /// # Errors
    /// run 不存在或讀取失敗時回傳錯誤。
    pub fn get_run(&self, run_id: i64) -> Result<RunView, IpcError> {
        crate::views::run_view(&self.store, run_id)
    }

    /// 取得指定手牌的完整檢視。
    ///
    /// # Errors
    /// 手牌不存在或解碼失敗時回傳錯誤。
    pub fn get_hand(
        &self,
        run_id: i64,
        hand_index: u64,
        visibility: HoleCardVisibility,
    ) -> Result<HandView, IpcError> {
        crate::views::hand_view(&self.store, run_id, hand_index, visibility)
    }

    /// 分頁取得逐手摘要（面板 G 的列表）。
    ///
    /// # Errors
    /// 查詢或解碼失敗時回傳錯誤。
    pub fn list_hands(
        &self,
        run_id: i64,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<HandSummaryView>, IpcError> {
        crate::views::hand_summaries(&self.store, run_id, offset, limit)
    }
}
