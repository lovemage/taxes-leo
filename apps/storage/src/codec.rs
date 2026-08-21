//! 逐手事件的緊湊序列化。
//!
//! 實做計劃 M0 的 log 容量規格要求「SQLite ＋逐手事件緊湊序列化」，
//! 且 100 萬手 DB ≤ 2GB。JSON 每手動輒上千位元組，達不到門檻，
//! 因此這裡用位元組編碼：牌張以 0..52 的單一位元組表示，
//! 金額用 varint（多數金額很小，只佔 1～2 位元組）。
//!
//! 格式版本化：解碼時檢查 magic 與版本，不符即拒絕，
//! 不做「盡力而為」的解讀（核心規格 3.3：升級格式視為版本變更）。

use poker_engine::betting::Action;
use poker_engine::card::{Card, Rank, Suit};
use poker_engine::chips::Chips;
use poker_engine::hand::{HandResult, Street};
use poker_engine::session::PlayedHand;

/// log 格式版本。任何編碼變更都必須遞增，並同步寫入 `RunManifest`。
///
/// v2：新增 `revealed` 遮罩。重播時必須知道哪些底牌依現實規則實際亮出過，
/// 否則 IPC 層無從判定可傳給 UI 的範圍（核心規格 2.4）。
///
/// v3：新增起始籌碼、強制下注紀錄與每個行動的 `committed_to`。
/// UI 規格 G.4 要求重播的底池與各座籌碼「一律取自 log 的事件，不由 UI
/// 重算」；只存 (街別, 座位, 行動) 的話，跟注與 all-in 的金額必須靠
/// 重跑下注規則才推得出來，等於把規則邏輯複製到讀取端。
pub const LOG_FORMAT_VERSION: u16 = 3;

const MAGIC: [u8; 2] = *b"9M";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedAction {
    pub street: Street,
    pub seat: u8,
    pub action: Action,
    /// 該座在**本街**行動後的累計投入。
    ///
    /// 由引擎的 `HandEvent::Acted` 原樣保存。`Call` 與 `AllIn` 的金額
    /// 不出現在行動本身，重播要顯示底池就只能靠這個值
    pub committed_to: Chips,
}

/// 強制下注（ante／盲注／straddle）。
///
/// 底池在第一個自願行動之前就已經有錢，這些錢不在 `actions` 裡。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedPost {
    pub seat: u8,
    pub kind: PostKind,
    pub amount: Chips,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostKind {
    Ante,
    SmallBlind,
    BigBlind,
    Straddle,
}

/// 一手牌的完整可重播紀錄。
///
/// 保存底牌、公共牌與行動序列即可由規則引擎完全重建底池與分配；
/// 這裡另存 rake 與 payouts，讓重播能以「重算結果 vs 保存結果」
/// 互相驗證，對應實做計劃風險表的「逐手重播與原執行逐位元一致」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandRecord {
    pub hand_index: u64,
    pub instance_index: u64,
    /// 各座是否在桌（重播據此還原位置與標籤）
    pub occupied: Vec<bool>,
    pub big_blind_seat: u8,
    pub hole_cards: Vec<Option<[Card; 2]>>,
    /// 依現實規則**實際亮出**的底牌。只有這些是公開資訊，
    /// IPC 層據此決定可傳給 UI 的範圍（核心規格 2.4、規則細則 4.2）
    pub revealed: Vec<bool>,
    pub board: Vec<Card>,
    pub actions: Vec<RecordedAction>,
    /// 本手開打前各座籌碼（未入座者為 0）
    pub starting_stacks: Vec<Chips>,
    pub posts: Vec<RecordedPost>,
    pub payouts: Vec<Chips>,
    pub refunds: Vec<Chips>,
    pub rake: Chips,
}

impl HandRecord {
    /// 由引擎輸出建立紀錄。
    #[must_use]
    pub fn from_played(played: &PlayedHand) -> Self {
        let result: &HandResult = &played.result;
        use poker_engine::hand::HandEvent;

        let actions = result
            .events
            .iter()
            .filter_map(|event| match event {
                HandEvent::Acted {
                    street,
                    seat,
                    action,
                    committed_to,
                } => Some(RecordedAction {
                    street: *street,
                    seat: u8::try_from(*seat).expect("座位索引必小於 256"),
                    action: *action,
                    committed_to: *committed_to,
                }),
                _ => None,
            })
            .collect();

        let posts = result
            .events
            .iter()
            .filter_map(|event| {
                let (seat, kind, amount) = match event {
                    HandEvent::PostAnte { seat, amount } => (seat, PostKind::Ante, amount),
                    HandEvent::PostSmallBlind { seat, amount } => {
                        (seat, PostKind::SmallBlind, amount)
                    }
                    HandEvent::PostBigBlind { seat, amount } => (seat, PostKind::BigBlind, amount),
                    HandEvent::PostStraddle { seat, amount } => (seat, PostKind::Straddle, amount),
                    _ => return None,
                };
                Some(RecordedPost {
                    seat: u8::try_from(*seat).expect("座位索引必小於 256"),
                    kind,
                    amount: *amount,
                })
            })
            .collect();

        Self {
            hand_index: played.hand_index,
            instance_index: played.instance_index,
            occupied: result.hole_cards.iter().map(Option::is_some).collect(),
            big_blind_seat: u8::try_from(played.positions.big_blind_seat)
                .expect("座位索引必小於 256"),
            hole_cards: result.hole_cards.clone(),
            revealed: result.revealed.clone(),
            board: result.board.clone(),
            actions,
            starting_stacks: played.starting_stacks.clone(),
            posts,
            payouts: result.distribution.payouts.clone(),
            refunds: result.distribution.refunds.clone(),
            rake: result.distribution.rake,
        }
    }

    /// 使用者座位在本手的淨損益，供統計層免解碼直接聚合。
    #[must_use]
    pub fn hero_delta(&self, hero_seat: usize, contributed: Chips) -> i64 {
        let gained = self.payouts[hero_seat] + self.refunds[hero_seat];
        let gained = i64::try_from(gained.units()).expect("金額必在 i64 範圍");
        let paid = i64::try_from(contributed.units()).expect("金額必在 i64 範圍");
        gained - paid
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    BadMagic,
    UnsupportedVersion(u16),
    Truncated,
    BadCard(u8),
    BadAction(u8),
    BadStreet(u8),
    BadPost(u8),
}

// ── 編碼 ────────────────────────────────────────────────────────────────

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = u8::try_from(value & 0x7F).expect("遮罩後必小於 128");
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn card_byte(card: Card) -> u8 {
    u8::try_from(card.index()).expect("牌張索引小於 52")
}

fn street_byte(street: Street) -> u8 {
    match street {
        Street::Preflop => 0,
        Street::Flop => 1,
        Street::Turn => 2,
        Street::River => 3,
    }
}

/// 將一手紀錄編碼為位元組。
#[must_use]
pub fn encode(record: &HandRecord) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&LOG_FORMAT_VERSION.to_le_bytes());

    put_varint(&mut out, record.hand_index);
    put_varint(&mut out, record.instance_index);

    let seats = u8::try_from(record.occupied.len()).expect("座位數小於 256");
    out.push(seats);
    // 在座旗標打包成位元遮罩（最多 9 座，兩個位元組足夠）
    let mut mask = 0u16;
    for (seat, &occupied) in record.occupied.iter().enumerate() {
        if occupied {
            mask |= 1 << seat;
        }
    }
    out.extend_from_slice(&mask.to_le_bytes());

    // 亮牌遮罩（v2）
    let mut revealed_mask = 0u16;
    for (seat, &revealed) in record.revealed.iter().enumerate() {
        if revealed {
            revealed_mask |= 1 << seat;
        }
    }
    out.extend_from_slice(&revealed_mask.to_le_bytes());
    out.push(record.big_blind_seat);

    // 底牌：只寫在座者，順序即座位序
    for hole in record.hole_cards.iter().flatten() {
        out.push(card_byte(hole[0]));
        out.push(card_byte(hole[1]));
    }

    out.push(u8::try_from(record.board.len()).expect("公共牌不超過 5 張"));
    for &card in &record.board {
        out.push(card_byte(card));
    }

    // 起始籌碼（v3）。座位數已寫在前面，這裡逐座寫值
    for &stack in &record.starting_stacks {
        put_varint(&mut out, stack.units());
    }

    // 強制下注（v3）
    put_varint(&mut out, record.posts.len() as u64);
    for post in &record.posts {
        out.push(post.seat);
        out.push(match post.kind {
            PostKind::Ante => 0,
            PostKind::SmallBlind => 1,
            PostKind::BigBlind => 2,
            PostKind::Straddle => 3,
        });
        put_varint(&mut out, post.amount.units());
    }

    put_varint(&mut out, record.actions.len() as u64);
    for action in &record.actions {
        out.push(street_byte(action.street));
        out.push(action.seat);
        match action.action {
            Action::Fold => out.push(0),
            Action::Check => out.push(1),
            Action::Call => out.push(2),
            Action::AllIn => out.push(3),
            Action::RaiseTo(to) => {
                out.push(4);
                put_varint(&mut out, to.units());
            }
        }
        put_varint(&mut out, action.committed_to.units());
    }

    for &amount in &record.payouts {
        put_varint(&mut out, amount.units());
    }
    for &amount in &record.refunds {
        put_varint(&mut out, amount.units());
    }
    put_varint(&mut out, record.rake.units());

    out
}

// ── 解碼 ────────────────────────────────────────────────────────────────

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, CodecError> {
        let byte = *self.bytes.get(self.pos).ok_or(CodecError::Truncated)?;
        self.pos += 1;
        Ok(byte)
    }

    fn u16(&mut self) -> Result<u16, CodecError> {
        let lo = self.u8()?;
        let hi = self.u8()?;
        Ok(u16::from_le_bytes([lo, hi]))
    }

    fn varint(&mut self) -> Result<u64, CodecError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 64 {
                return Err(CodecError::Truncated);
            }
        }
    }

    fn card(&mut self) -> Result<Card, CodecError> {
        let byte = self.u8()?;
        if byte >= 52 {
            return Err(CodecError::BadCard(byte));
        }
        let rank = Rank::from_value(byte / 4 + 2).ok_or(CodecError::BadCard(byte))?;
        let suit = match byte % 4 {
            0 => Suit::Clubs,
            1 => Suit::Diamonds,
            2 => Suit::Hearts,
            _ => Suit::Spades,
        };
        Ok(Card::new(rank, suit))
    }
}

/// 由位元組還原一手紀錄。
///
/// # Errors
/// magic 或版本不符、位元組不足，或內容越界時回傳錯誤。
pub fn decode(bytes: &[u8]) -> Result<HandRecord, CodecError> {
    let mut r = Reader { bytes, pos: 0 };

    if [r.u8()?, r.u8()?] != MAGIC {
        return Err(CodecError::BadMagic);
    }
    let version = r.u16()?;
    if version != LOG_FORMAT_VERSION {
        return Err(CodecError::UnsupportedVersion(version));
    }

    let hand_index = r.varint()?;
    let instance_index = r.varint()?;

    let seats = usize::from(r.u8()?);
    let mask = r.u16()?;
    let occupied: Vec<bool> = (0..seats).map(|s| mask & (1 << s) != 0).collect();
    let revealed_mask = r.u16()?;
    let revealed: Vec<bool> = (0..seats).map(|s| revealed_mask & (1 << s) != 0).collect();
    let big_blind_seat = r.u8()?;

    let mut hole_cards = vec![None; seats];
    for seat in 0..seats {
        if occupied[seat] {
            hole_cards[seat] = Some([r.card()?, r.card()?]);
        }
    }

    let board_len = usize::from(r.u8()?);
    let mut board = Vec::with_capacity(board_len);
    for _ in 0..board_len {
        board.push(r.card()?);
    }

    let mut starting_stacks = Vec::with_capacity(seats);
    for _ in 0..seats {
        starting_stacks.push(Chips::new(r.varint()?));
    }

    let post_count = usize::try_from(r.varint()?).map_err(|_| CodecError::Truncated)?;
    let mut posts = Vec::with_capacity(post_count);
    for _ in 0..post_count {
        let seat = r.u8()?;
        let kind = match r.u8()? {
            0 => PostKind::Ante,
            1 => PostKind::SmallBlind,
            2 => PostKind::BigBlind,
            3 => PostKind::Straddle,
            other => return Err(CodecError::BadPost(other)),
        };
        posts.push(RecordedPost {
            seat,
            kind,
            amount: Chips::new(r.varint()?),
        });
    }

    let action_count = usize::try_from(r.varint()?).map_err(|_| CodecError::Truncated)?;
    let mut actions = Vec::with_capacity(action_count);
    for _ in 0..action_count {
        let street = match r.u8()? {
            0 => Street::Preflop,
            1 => Street::Flop,
            2 => Street::Turn,
            3 => Street::River,
            other => return Err(CodecError::BadStreet(other)),
        };
        let seat = r.u8()?;
        let action = match r.u8()? {
            0 => Action::Fold,
            1 => Action::Check,
            2 => Action::Call,
            3 => Action::AllIn,
            4 => Action::RaiseTo(Chips::new(r.varint()?)),
            other => return Err(CodecError::BadAction(other)),
        };
        actions.push(RecordedAction {
            street,
            seat,
            action,
            committed_to: Chips::new(r.varint()?),
        });
    }

    let mut payouts = Vec::with_capacity(seats);
    for _ in 0..seats {
        payouts.push(Chips::new(r.varint()?));
    }
    let mut refunds = Vec::with_capacity(seats);
    for _ in 0..seats {
        refunds.push(Chips::new(r.varint()?));
    }
    let rake = Chips::new(r.varint()?);

    Ok(HandRecord {
        hand_index,
        instance_index,
        occupied,
        big_blind_seat,
        hole_cards,
        revealed,
        board,
        actions,
        starting_stacks,
        posts,
        payouts,
        refunds,
        rake,
    })
}
