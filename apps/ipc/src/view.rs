//! 跨 IPC 邊界的資料型別（DTO）。
//!
//! 實做計劃第七章：**Rust structs 為型別單一來源**，前端 TS 型別由此產生，
//! 避免兩邊各自手寫而漂移。
//!
//! # 隱藏資訊隔離
//!
//! 核心規格 2.4：「互動牌桌只顯示規則允許公開的牌；重播是否顯示未攤牌底牌
//! 採明確設定，預設不顯示。」
//!
//! 本模組的做法是**在邊界就遮蔽**，而不是把完整底牌送到前端再叫前端別畫。
//! UI 拿不到的資料就不可能因為前端 bug 而外洩，也讓「UI 零遊戲邏輯」
//! （實做計劃鐵則 6）少一個破口。

use poker_engine::card::Card;
use poker_engine::position::Positions;
use poker_storage::codec::{HandRecord, RecordedAction};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 底牌可見範圍。
///
/// 這是個**明確設定**，沒有預設值可以被意外略過——呼叫端必須指定，
/// 對應核心規格 2.4 的「採明確設定」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub enum HoleCardVisibility {
    /// 只送出依現實規則實際亮出的底牌。互動對打與預設重播用此模式
    RevealedOnly,
    /// 送出全部底牌。僅供使用者明確開啟的重播檢視使用
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub enum StreetView {
    Preflop,
    Flop,
    Turn,
    River,
}

impl From<poker_engine::hand::Street> for StreetView {
    fn from(street: poker_engine::hand::Street) -> Self {
        use poker_engine::hand::Street;
        match street {
            Street::Preflop => Self::Preflop,
            Street::Flop => Self::Flop,
            Street::Turn => Self::Turn,
            Street::River => Self::River,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ActionView {
    pub street: StreetView,
    pub seat: u8,
    /// fold／check／call／allIn／raiseTo
    pub kind: String,
    /// `raiseTo` 時為加注到的本街累計額，其餘為 null
    #[ts(type = "number | null")]
    pub to: Option<u64>,
}

impl From<&RecordedAction> for ActionView {
    fn from(action: &RecordedAction) -> Self {
        use poker_engine::betting::Action;
        let (kind, to) = match action.action {
            Action::Fold => ("fold", None),
            Action::Check => ("check", None),
            Action::Call => ("call", None),
            Action::AllIn => ("allIn", None),
            Action::RaiseTo(amount) => ("raiseTo", Some(amount.units())),
        };
        Self {
            street: action.street.into(),
            seat: action.seat,
            kind: kind.to_owned(),
            to,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SeatView {
    pub seat: u8,
    pub occupied: bool,
    /// 位置標籤，使用規則細則 8.4.1 的唯一命名（UTG／LJ／BTN…）。
    /// dead button／dead small blind 時該標籤不會出現在任何座位
    pub position: Option<String>,
    /// 底牌，`null` 表示依可見範圍遮蔽或該座無人
    pub hole_cards: Option<[String; 2]>,
    /// 該座底牌是否依現實規則實際亮出過
    pub revealed: bool,
    #[ts(type = "number")]
    pub payout: u64,
    #[ts(type = "number")]
    pub refund: u64,
}

/// 重播動畫的一幀（UI 規格 G.4）。
///
/// 每一幀是「牌桌在某個時間點的完整狀態」，不是「相對前一幀的變化」。
/// 前端因此可以直接跳到任意幀，不必從頭累加——逐步、拖曳與倒帶都成立。
///
/// 金額全部由引擎算好。G.4 明訂底池與各座籌碼不得由 UI 重算，
/// 因為那等於把下注規則複製一份到前端。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct FrameView {
    /// ante／smallBlind／bigBlind／straddle／deal／fold／check／call／raiseTo／allIn／settle
    pub kind: String,
    pub street: StreetView,
    /// 這一幀由誰行動。發牌與收池為 null
    pub seat: Option<u8>,
    /// 金額。`raiseTo` 為加注到的本街累計額，`call`／`allIn` 為跟到的額度，
    /// 強制下注為該筆金額，其餘為 null
    #[ts(type = "number | null")]
    pub to: Option<u64>,
    /// **這一幀已發出**的公共牌。未發的牌不在此，UI 拿不到就畫不出來
    pub board: Vec<String>,
    #[ts(type = "number")]
    pub pot: u64,
    /// 各座本手累計投入
    #[ts(type = "number[]")]
    pub committed: Vec<u64>,
    /// 各座剩餘籌碼
    #[ts(type = "number[]")]
    pub stacks: Vec<u64>,
    pub folded: Vec<bool>,
}

/// 一手牌的可視化資料。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct HandView {
    #[ts(type = "number")]
    pub hand_index: u64,
    #[ts(type = "number")]
    pub instance_index: u64,
    pub seated: u8,
    /// 按鈕**位置**。dead button 時該座位無人持有
    pub button: u8,
    pub dead_button: bool,
    pub dead_small_blind: bool,
    pub seats: Vec<SeatView>,
    pub board: Vec<String>,
    pub actions: Vec<ActionView>,
    /// 重播動畫的逐幀狀態。順序即播放順序
    pub frames: Vec<FrameView>,
    #[ts(type = "number")]
    pub rake: u64,
    /// 本手實際套用的底牌可見範圍，供 UI 顯示「重播已開啟全底牌」之類的提示
    pub visibility: HoleCardVisibility,
}

fn card_text(card: Card) -> String {
    card.to_string()
}

impl HandView {
    /// 由儲存的紀錄組出可傳給 UI 的檢視。
    ///
    /// `positions` 來自引擎的位置解析；`visibility` 決定其他座位的底牌遮蔽
    /// 範圍。`hero_seat` 的底牌不受 `visibility` 影響，恆可見。
    #[must_use]
    pub fn from_record(
        record: &HandRecord,
        positions: &Positions,
        visibility: HoleCardVisibility,
        hero_seat: usize,
    ) -> Self {
        let seats = record
            .occupied
            .iter()
            .enumerate()
            .map(|(seat, &occupied)| {
                let revealed = record.revealed.get(seat).copied().unwrap_or(false);
                // 遮蔽發生在這裡：未亮出且未開啟全揭露時，底牌根本不進入 DTO。
                //
                // 使用者自己的底牌是例外，**恆可見**（UI 規格 G.2）。這不是
                // 放寬遮蔽：牌本來就是發給他的，他在牌桌上一直看得到。
                // 遮掉自己的牌反而讓重播對不上他當時的視角
                let visible = matches!(visibility, HoleCardVisibility::All)
                    || revealed
                    || seat == hero_seat;
                let hole_cards = record
                    .hole_cards
                    .get(seat)
                    .and_then(|c| *c)
                    .filter(|_| visible)
                    .map(|cards| [card_text(cards[0]), card_text(cards[1])]);

                SeatView {
                    seat: u8::try_from(seat).unwrap_or(u8::MAX),
                    occupied,
                    position: positions
                        .labels
                        .get(seat)
                        .and_then(|l| *l)
                        .map(|label| label.as_str().to_owned()),
                    hole_cards,
                    revealed,
                    payout: record.payouts.get(seat).map_or(0, |c| c.units()),
                    refund: record.refunds.get(seat).map_or(0, |c| c.units()),
                }
            })
            .collect();

        Self {
            hand_index: record.hand_index,
            instance_index: record.instance_index,
            seated: u8::try_from(record.occupied.iter().filter(|&&o| o).count())
                .unwrap_or(u8::MAX),
            button: u8::try_from(positions.button).unwrap_or(u8::MAX),
            dead_button: positions.dead_button,
            dead_small_blind: positions.dead_small_blind,
            seats,
            board: record.board.iter().copied().map(card_text).collect(),
            actions: record.actions.iter().map(ActionView::from).collect(),
            frames: crate::frames::build(record),
            rake: record.rake.units(),
            visibility,
        }
    }
}

/// 逐手列表用的摘要（面板 G）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct HandSummaryView {
    #[ts(type = "number")]
    pub hand_index: u64,
    #[ts(type = "number")]
    pub instance_index: u64,
    pub seated: u8,
    /// 使用者座位在本手的淨損益，以最小籌碼單位計
    #[ts(type = "number")]
    pub hero_delta: i64,
    pub board: Vec<String>,
}

/// run 層級的摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    #[ts(type = "number")]
    pub run_id: i64,
    #[ts(type = "number")]
    pub hands_played: u64,
    pub completed: bool,
    pub players: u8,
    pub hero_seat: u8,
    /// master seed 是完整 u64 值域，可能超過 JS 的安全整數上限，
    /// 因此以字串傳遞。它只供顯示與重現設定，不參與前端運算
    #[ts(type = "string")]
    #[serde(with = "seed_as_string")]
    pub master_seed: u64,
    pub rng_algorithm: String,
    /// 桌次數。統計層以此判斷 block／cluster 是否足夠（核心規格 5.3）
    #[ts(type = "number")]
    pub instance_count: u64,
}

/// `master_seed` 以字串序列化。
///
/// u64 的上界超過 JavaScript 的 `Number.MAX_SAFE_INTEGER`，直接送數字會在
/// 前端靜默失去精度，而 seed 一旦失真就無法重現同一個 run。
mod seed_as_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// 執行前的統計效力預覽（UI 規格 F.5.1、核心規格 5.3.1）。
///
/// 由 Rust 計算而非前端重寫：算式雖然只是 `1.96σ/√(N/100)`，但 σ 的來源、
/// 切片數的定義與可用門檻都在引擎側，重寫一次就多一處會漂移的地方。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PowerPreviewView {
    /// 分析層級的顯示名稱
    pub level: String,
    #[ts(type = "number")]
    pub hands_per_slice: u64,
    /// 95% CI 半寬，bb/100。**一律計算**——區間再寬也是資訊，
    /// 換成「樣本不足」四個字反而讓使用者什麼都不知道。
    /// 僅在手數為 0 這種退化情形為 null
    #[ts(type = "number | null")]
    pub half_width_bb100: Option<f64>,
    /// 把半寬收到建議精度所需的總手數
    #[ts(type = "number")]
    pub hands_for_target: u64,
    /// 建議精度（半寬），bb/100
    pub target_half_width_bb100: f64,
    /// 是否已達建議精度。**這不是能不能用的閘門**，只是說服力的標示
    pub meets_target: bool,
}
