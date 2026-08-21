//! 重播幀。
//!
//! UI 規格 G.4：重播的底池、邊池與各座籌碼「**數值一律取自 log 的事件，
//! 不由 UI 重算**」。因此把「某個時間點的牌桌長什麼樣」算在這裡，
//! 前端只負責畫。
//!
//! # 為什麼公共牌逐幀帶而不是一次送完
//!
//! 每一幀只帶**當下已發出**的公共牌。整副 board 一次送到前端，就等於
//! 讓 UI 有機會在該街還沒到之前把牌畫出來；隱藏資訊的處理原則是
//! 「拿不到就畫不出來」，而不是「拿得到但約定不要畫」。
//!
//! 這個約束在重播上看起來多餘（手已經打完，board 本來就是公開資訊），
//! 但互動對打共用同一組幀，那裡未發的牌**絕對不能**離開引擎。
//! 兩邊用同一個模型，才不會有一邊忘記遮。

use poker_engine::betting::Action;
use poker_engine::hand::Street;
use poker_storage::codec::{HandRecord, PostKind};

use crate::view::{FrameView, StreetView};

/// 該街應可見的公共牌張數。
fn visible_cards(street: Street) -> usize {
    match street {
        Street::Preflop => 0,
        Street::Flop => 3,
        Street::Turn => 4,
        Street::River => 5,
    }
}

/// 逐座金額的累加狀態。
struct Ledger {
    /// 前幾街已結算的投入（含 ante 這類 dead money）
    carried: Vec<u64>,
    /// 本街投入。引擎的 `committed_to` 是**逐街**累計，因此分開記
    street: Vec<u64>,
    starting: Vec<u64>,
    folded: Vec<bool>,
}

impl Ledger {
    fn new(record: &HandRecord) -> Self {
        let seats = record.occupied.len();
        Self {
            carried: vec![0; seats],
            street: vec![0; seats],
            starting: record.starting_stacks.iter().map(|c| c.units()).collect(),
            folded: vec![false; seats],
        }
    }

    fn total(&self, seat: usize) -> u64 {
        self.carried[seat] + self.street[seat]
    }

    fn committed_all(&self) -> Vec<u64> {
        (0..self.carried.len()).map(|s| self.total(s)).collect()
    }

    fn stacks(&self) -> Vec<u64> {
        (0..self.carried.len())
            .map(|s| self.starting.get(s).copied().unwrap_or(0) - self.total(s))
            .collect()
    }

    fn pot(&self) -> u64 {
        (0..self.carried.len()).map(|s| self.total(s)).sum()
    }

    /// 換街：本街投入結算進 carried。
    fn advance_street(&mut self) {
        for seat in 0..self.carried.len() {
            self.carried[seat] += self.street[seat];
            self.street[seat] = 0;
        }
    }
}

/// 把一手紀錄展開成逐幀狀態。
///
/// 幀的順序即動畫的播放順序：強制下注 → 逐街（發牌 → 各座行動）→ 收池。
#[must_use]
pub fn build(record: &HandRecord) -> Vec<FrameView> {
    let mut ledger = Ledger::new(record);
    let mut frames = Vec::with_capacity(record.actions.len() + 8);
    let mut street = Street::Preflop;

    let board_text = |count: usize| -> Vec<String> {
        record
            .board
            .iter()
            .take(count)
            .map(std::string::ToString::to_string)
            .collect()
    };

    // ── 強制下注 ──
    //
    // ante 屬 dead money，不計入當街投入（規則細則 2.2），因此進 carried；
    // 盲注與 straddle 是翻前的當街投入，引擎的 committed_to 已經含著它們，
    // 放進 street 才不會被後續行動的 committed_to 重複加一次
    for post in &record.posts {
        let seat = usize::from(post.seat);
        match post.kind {
            PostKind::Ante => ledger.carried[seat] += post.amount.units(),
            _ => ledger.street[seat] = post.amount.units(),
        }
        frames.push(FrameView {
            kind: match post.kind {
                PostKind::Ante => "ante",
                PostKind::SmallBlind => "smallBlind",
                PostKind::BigBlind => "bigBlind",
                PostKind::Straddle => "straddle",
            }
            .to_owned(),
            street: StreetView::Preflop,
            seat: Some(post.seat),
            to: Some(post.amount.units()),
            board: Vec::new(),
            pot: ledger.pot(),
            committed: ledger.committed_all(),
            stacks: ledger.stacks(),
            folded: ledger.folded.clone(),
        });
    }

    // ── 逐街行動 ──
    for action in &record.actions {
        if action.street != street {
            street = action.street;
            ledger.advance_street();
            let count = visible_cards(street).min(record.board.len());
            frames.push(FrameView {
                kind: "deal".to_owned(),
                street: street.into(),
                seat: None,
                to: None,
                board: board_text(count),
                pot: ledger.pot(),
                committed: ledger.committed_all(),
                stacks: ledger.stacks(),
                folded: ledger.folded.clone(),
            });
        }

        let seat = usize::from(action.seat);
        ledger.street[seat] = action.committed_to.units();
        if matches!(action.action, Action::Fold) {
            ledger.folded[seat] = true;
        }

        let (kind, to) = match action.action {
            Action::Fold => ("fold", None),
            Action::Check => ("check", None),
            Action::Call => ("call", Some(action.committed_to.units())),
            Action::AllIn => ("allIn", Some(action.committed_to.units())),
            Action::RaiseTo(amount) => ("raiseTo", Some(amount.units())),
        };

        frames.push(FrameView {
            kind: kind.to_owned(),
            street: street.into(),
            seat: Some(action.seat),
            to,
            board: board_text(visible_cards(street).min(record.board.len())),
            pot: ledger.pot(),
            committed: ledger.committed_all(),
            stacks: ledger.stacks(),
            folded: ledger.folded.clone(),
        });
    }

    ledger.advance_street();

    // ── all-in 之後的 runout ──
    //
    // 全下之後沒有人再行動，剩下的公共牌不會由任何行動幀帶出來，
    // 但那些牌確實一張張發過。逐街補幀，動畫才不會直接跳到攤牌
    let mut shown = visible_cards(street).min(record.board.len());
    while shown < record.board.len() {
        shown = match shown {
            0 => 3,
            n => n + 1,
        }
        .min(record.board.len());
        street = match shown {
            0..=3 => Street::Flop,
            4 => Street::Turn,
            _ => Street::River,
        };
        frames.push(FrameView {
            kind: "deal".to_owned(),
            street: street.into(),
            seat: None,
            to: None,
            board: board_text(shown),
            pot: ledger.pot(),
            committed: ledger.committed_all(),
            stacks: ledger.stacks(),
            folded: ledger.folded.clone(),
        });
    }

    // ── 收池 ──
    frames.push(FrameView {
        kind: "settle".to_owned(),
        street: street.into(),
        seat: None,
        to: None,
        board: board_text(record.board.len()),
        pot: ledger.pot(),
        committed: ledger.committed_all(),
        stacks: ledger.stacks(),
        folded: ledger.folded.clone(),
    });

    frames
}

/// 本手最終進池總額，供呼叫端與 `payouts + refunds + rake` 對帳。
#[must_use]
pub fn total_pot(record: &HandRecord) -> u64 {
    let mut ledger = Ledger::new(record);
    for post in &record.posts {
        let seat = usize::from(post.seat);
        match post.kind {
            PostKind::Ante => ledger.carried[seat] += post.amount.units(),
            _ => ledger.street[seat] = post.amount.units(),
        }
    }
    let mut street = Street::Preflop;
    for action in &record.actions {
        if action.street != street {
            street = action.street;
            ledger.advance_street();
        }
        ledger.street[usize::from(action.seat)] = action.committed_to.units();
    }
    ledger.advance_street();
    ledger.pot()
}
