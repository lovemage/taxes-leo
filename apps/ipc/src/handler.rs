//! IPC 指令處理。
//!
//! 每個方法對應 M3 的一個 Tauri command。這裡不依賴 Tauri，
//! command 只會是薄殼：解析參數 → 呼叫這裡 → 序列化結果。

use poker_engine::position::resolve;
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
        let manifest = self.store.load_manifest(run_id)?;
        Ok(RunView {
            run_id,
            hands_played: manifest.instances.iter().map(|i| i.hands).sum(),
            completed: manifest.completed,
            players: u8::try_from(manifest.players).unwrap_or(u8::MAX),
            hero_seat: u8::try_from(manifest.hero_seat).unwrap_or(u8::MAX),
            master_seed: manifest.master_seed,
            rng_algorithm: manifest.rng_algorithm,
            instance_count: manifest.instances.len() as u64,
        })
    }

    /// 取得指定手牌的完整檢視。
    ///
    /// `visibility` 必須由呼叫端明示。互動對打一律傳 `RevealedOnly`；
    /// 只有使用者在重播中明確開啟全底牌時才傳 `All`（核心規格 2.4）。
    ///
    /// # Errors
    /// 手牌不存在或解碼失敗時回傳錯誤。
    pub fn get_hand(
        &self,
        run_id: i64,
        hand_index: u64,
        visibility: HoleCardVisibility,
    ) -> Result<HandView, IpcError> {
        let record = self.store.load_hand(run_id, hand_index)?;
        let positions = resolve(&record.occupied, usize::from(record.big_blind_seat));
        Ok(HandView::from_record(&record, &positions, visibility))
    }

    /// 分頁取得逐手摘要（面板 G 的列表）。
    ///
    /// 摘要不含任何底牌，因此無須指定可見範圍。
    ///
    /// # Errors
    /// 查詢或解碼失敗時回傳錯誤。
    pub fn list_hands(
        &self,
        run_id: i64,
        offset: u64,
        limit: u64,
        hero_seat: usize,
    ) -> Result<Vec<HandSummaryView>, IpcError> {
        let records = self.store.page_hands(run_id, offset, limit)?;
        Ok(records
            .into_iter()
            .map(|record| {
                let gained = record.payouts[hero_seat] + record.refunds[hero_seat];
                HandSummaryView {
                    hand_index: record.hand_index,
                    instance_index: record.instance_index,
                    seated: u8::try_from(record.occupied.iter().filter(|&&o| o).count())
                        .unwrap_or(u8::MAX),
                    hero_delta: i64::try_from(gained.units()).unwrap_or(i64::MAX),
                    board: record.board.iter().map(ToString::to_string).collect(),
                }
            })
            .collect())
    }
}
