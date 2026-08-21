//! 由儲存層組出 DTO 的自由函式。
//!
//! 這些函式收 `&Store` 而非擁有它，因為桌面殼要在 UI 執行緒與批次執行緒
//! 之間共用同一個資料庫連線（核心規格 3.2：批次執行期間仍可瀏覽既有
//! 報表與 log）。[`crate::IpcHandler`] 是擁有所有權的便利包裝，內部委派
//! 到這裡，兩條路徑因此不會有兩份實作。

use poker_engine::position::resolve;
use poker_storage::db::Store;

use crate::handler::IpcError;
use crate::view::{HandSummaryView, HandView, HoleCardVisibility, PowerPreviewView, RunView};

/// run 層級摘要。
///
/// # Errors
/// run 不存在或讀取失敗時回傳錯誤。
pub fn run_view(store: &Store, run_id: i64) -> Result<RunView, IpcError> {
    let manifest = store.load_manifest(run_id)?;
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

/// 指定手牌的完整檢視。
///
/// `visibility` 必須由呼叫端明示（核心規格 2.4）。遮蔽在此完成，
/// 未亮出的底牌不會進入回傳值。
///
/// # Errors
/// 手牌不存在或解碼失敗時回傳錯誤。
pub fn hand_view(
    store: &Store,
    run_id: i64,
    hand_index: u64,
    visibility: HoleCardVisibility,
) -> Result<HandView, IpcError> {
    let record = store.load_hand(run_id, hand_index)?;
    let positions = resolve(&record.occupied, usize::from(record.big_blind_seat));
    Ok(HandView::from_record(&record, &positions, visibility))
}

/// 逐手摘要分頁。
///
/// 損益取自儲存層寫入時算好的 `hero_delta`，**不由 payouts 重算**——
/// blob 不含各座投入額，重算會漏掉投入而得出恆為正的錯誤數字。
///
/// # Errors
/// 查詢或解碼失敗時回傳錯誤。
pub fn hand_summaries(
    store: &Store,
    run_id: i64,
    offset: u64,
    limit: u64,
) -> Result<Vec<HandSummaryView>, IpcError> {
    let rows = store.page_hand_summaries(run_id, offset, limit)?;
    Ok(rows
        .into_iter()
        .map(|(record, hero_delta)| HandSummaryView {
            hand_index: record.hand_index,
            instance_index: record.instance_index,
            seated: u8::try_from(record.occupied.iter().filter(|&&o| o).count())
                .unwrap_or(u8::MAX),
            hero_delta,
            board: record.board.iter().map(ToString::to_string).collect(),
        })
        .collect())
}

/// 某手數設定下的統計效力預覽。
///
/// UI 規格 F.5.1 要求面板 A 的手數滑桿旁即時顯示，
/// 不能等跑完才在報表揭露。
#[must_use]
pub fn power_previews(hand_limit: u64, players: usize) -> Vec<PowerPreviewView> {
    use poker_engine::stats::{preview_all, PLANNING_SIGMA_BB100};

    preview_all(hand_limit, players, PLANNING_SIGMA_BB100)
        .into_iter()
        .map(|preview| PowerPreviewView {
            level: preview.level.as_str().to_owned(),
            hands_per_slice: preview.hands_per_slice,
            half_width_bb100: preview
                .half_width_bb100
                .is_finite()
                .then_some(preview.half_width_bb100),
            usable: preview.usable,
        })
        .collect()
}
