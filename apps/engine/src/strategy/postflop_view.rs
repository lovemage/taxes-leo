//! 由 [`DecisionView`] 推導翻後策略節點（0907 計劃 §4.1）。
//!
//! 線路、底池類型、面對尺度、SPR 這些全部由**既有的公開資訊**推導：
//! 完整的 `PublicAction` history、`OpponentPublic` 與座位順序。不新增
//! `DecisionView` 欄位——那個結構刻意沒有任何欄位能承載隱藏資訊，
//! 每加一個欄位都要重新論證它依規則已公開。

use std::collections::BTreeMap;

use crate::hand::Street;
use crate::strategy::decision::{DecisionView, PublicAction};
use crate::strategy::hand_strength::{classify, HandStrengthConfig, HandStrengthSnapshot};
use crate::strategy::postflop::{
    classify_board, AggressorRole, FacingSize, PostflopContext, PostflopLine, PostflopSizing,
    PotType, RelativeAggressorOrder,
};

/// 推導出的節點，連同分類快照一起回傳。
///
/// 快照是 UI 的「指定底牌分類預覽」與決策 trace 的資料來源，
/// 決策本身只用得到 `context.hand_strength`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostflopNodeView {
    pub context: PostflopContext,
    pub snapshot: HandStrengthSnapshot,
    pub sizing: PostflopSizing,
}

/// 由決策視圖建立翻後節點。
///
/// 翻前或公共牌張數不合法時回傳 `None`——那代表呼叫端把翻前節點送進了
/// 翻後路徑，靜默回一個「看起來合理」的 context 只會讓錯誤傳得更遠。
#[must_use]
pub fn postflop_node(view: &DecisionView, config: &HandStrengthConfig) -> Option<PostflopNodeView> {
    if matches!(view.street, Street::Preflop) {
        return None;
    }
    let board_textures = classify_board(&view.board)?;
    let snapshot = classify(view.hole_cards, &view.board, config);

    let active_opponents = view.active_opponents();
    let sizing = PostflopSizing {
        pot: view.pot,
        to_call: view.to_call,
    };

    let context = PostflopContext {
        legacy_facing_size: legacy_facing_size(view),
        relative_position: relative_position(view),
        decision_phase: decision_phase(view),
        continuation: continuation(view),
        street: view.street,
        board_textures,
        hand_strength: snapshot.group,
        active_players: u8::try_from(active_opponents.saturating_add(1)).unwrap_or(u8::MAX),
        hero_position: view.position,
        opponents_behind: opponents_behind(view),
        pot_type: pot_type(view),
        facing_size: facing_size(view),
        spr_centi: spr_centi(view),
        effective_stack_bucket: view.effective_stack_bucket,
        line: derive_line(view),
    };

    Some(PostflopNodeView {
        context,
        snapshot,
        sizing,
    })
}

/// 英雄身後仍會行動的對手數。
///
/// 多人底池的位置優勢由這個數字表達，而不是壓縮成單一 IP／OOP
/// （核心規格 4.2）。
fn opponents_behind(view: &DecisionView) -> u8 {
    let hero_order = view.position.postflop_order();
    let count = view
        .opponents
        .iter()
        .filter(|opponent| {
            !opponent.folded && !opponent.all_in && opponent.position.postflop_order() > hero_order
        })
        .count();
    u8::try_from(count).unwrap_or(u8::MAX)
}

/// 由翻前的加注次數判定底池類型。
fn pot_type(view: &DecisionView) -> PotType {
    let raises = view
        .history
        .iter()
        .filter(|action| matches!(action.street, Street::Preflop) && action.raised)
        .count();
    match raises {
        0 | 1 => PotType::SingleRaised,
        2 => PotType::ThreeBet,
        _ => PotType::FourBet,
    }
}

/// 面對的下注尺度級距。
///
/// 分母是**這次進攻打進去之前的底池**，不是含了這注的當下底池。
/// `DecisionView.pot` 已經包含對手剛投入的籌碼，直接拿 `to_call / pot`
/// 分檔會把「底池 100、對手下注 100」算成 100/200＝50%，滿池下注因此
/// 落到 Half；使用者編輯「面對滿池」的覆寫永遠命不中一般的滿池下注。
///
/// 基準由公開的 action history 還原：`PublicAction::committed_to` 是該座
/// 在本街的累計投入，走一遍就能同時得到每一步的注額層級與跑動底池，
/// 因此再加注、英雄已有投入、多人中途跟注都算得對，不必對所有情境硬套
/// `pot - to_call`。
///
/// **不自行判定全下**：那需要知道對手是不是真的推光了，而 `all_in` 旗標
/// 講的是對手的狀態，不是這次下注的尺度；混起來會讓「短碼跟注」被當成
/// overbet。籌碼不足的全下不會把注額推高，引擎因此不標 `raised`，
/// 在這裡也就不會被當成一次新的進攻。
fn facing_size(view: &DecisionView) -> FacingSize {
    if view.to_call.units() == 0 {
        return FacingSize::None;
    }
    let (bet, pot_before) = facing_bet_basis(view);
    facing_ratio(bet, pot_before.max(1))
}

/// 還原「最後一次進攻加了多少」與「它下進去的底池有多大」。
///
/// 回傳 `(下注量, 基準底池)`：
///
/// - **下注量**是把注額層級由 `level` 推到 `committed_to` 的增量。開注時
///   層級是 0，增量就是下注額；再加注時是加注的增量，不是加注到的總額。
/// - **基準底池**是進攻者行動當下的底池，**加上他為了跟平前一手注所投入
///   的部分**。這是「滿池加注」的定義：面對 50 的注、底池 150 時，滿池
///   加注是先跟 50（底池 200）再加 200。在他之後才跟進的籌碼不算進基準
///   ——那些錢在他決定尺度時還不在池子裡。
///
/// 本街沒有任何進攻紀錄時（歷史被截斷，或手寫 fixture 只填了 `to_call`）
/// 退回 `to_call / (pot - to_call)`。那仍然是「下注前底池」的語意，
/// 只是把英雄要跟的金額當成下注量，多人底池會偏小。
fn facing_bet_basis(view: &DecisionView) -> (u64, u64) {
    let current = view.current_street_history();

    // 本街之前的底池：當下底池扣掉本街各座的累計投入。已棄牌者投入的
    // 籌碼留在池子裡，因此以「每座最後一筆 committed_to」計算
    let mut latest: BTreeMap<usize, u64> = BTreeMap::new();
    for action in &current {
        latest.insert(action.seat, action.committed_to.units());
    }
    let street_total: u64 = latest.values().sum();
    let mut pot = view.pot.units().saturating_sub(street_total);

    let mut committed: BTreeMap<usize, u64> = BTreeMap::new();
    let mut level = 0u64;
    let mut basis = None;
    for action in &current {
        let before = committed.get(&action.seat).copied().unwrap_or(0);
        let after = action.committed_to.units();
        if action.raised {
            basis = Some((
                after.saturating_sub(level),
                pot.saturating_add(level.saturating_sub(before)),
            ));
        }
        pot = pot.saturating_add(after.saturating_sub(before));
        committed.insert(action.seat, after);
        level = level.max(after);
    }

    basis.unwrap_or_else(|| {
        let to_call = view.to_call.units();
        (to_call, view.pot.units().saturating_sub(to_call))
    })
}

/// SPR × 100。
///
/// 有效籌碼取「英雄還能推到的上限」與「最大的未棄牌對手可投入額」中的
/// 較小者。英雄的剩餘籌碼沒有直接欄位，但 `all_in_to` 就是他能推到的
/// 本街累計上限，用它當上界是安全的近似——偏大的話 SPR 也偏大，
/// 而規則條件是範圍，不會因此落到更激進的一檔。
fn spr_centi(view: &DecisionView) -> u32 {
    let hero_ceiling = view.legal.all_in_to.map_or(0, |chips| chips.units());
    let opponent_ceiling = view
        .opponents
        .iter()
        .filter(|opponent| !opponent.folded)
        .map(|opponent| {
            opponent
                .stack
                .units()
                .saturating_add(opponent.committed.units())
        })
        .max()
        .unwrap_or(0);
    let effective = hero_ceiling.min(opponent_ceiling);
    let pot = view.pot.units().max(1);
    u32::try_from(effective.saturating_mul(100) / pot).unwrap_or(u32::MAX)
}

/// 由公開行動歷史推導線路。
fn derive_line(view: &DecisionView) -> PostflopLine {
    let hero = view.seat;

    let previous = previous_street(view.street);
    let previous_aggressor = previous
        .and_then(|street| last_aggressor_on(&view.history, street))
        .map(|action| action.seat);

    // 跨街回溯、不含本街：即使上一街 check-through，仍保留更早街的主動方
    let last_aggressor = view
        .history
        .iter()
        .rfind(|action| action.street < view.street && action.raised);

    let previous_street_aggressor = role_of(previous_aggressor, hero);
    let last_role = role_of(last_aggressor.map(|action| action.seat), hero);

    let relative_aggressor_order = match (last_role, last_aggressor) {
        (AggressorRole::Opponent, Some(action)) => {
            if view.position.postflop_order() < action.position.postflop_order() {
                RelativeAggressorOrder::HeroBefore
            } else {
                RelativeAggressorOrder::HeroAfter
            }
        }
        _ => RelativeAggressorOrder::NotApplicable,
    };

    let current: Vec<&PublicAction> = view.current_street_history();
    let hero_checked_this_street = current.iter().any(|action| {
        action.seat == hero && matches!(action.action, crate::betting::Action::Check)
    });
    let aggressive_count = current.iter().filter(|action| action.raised).count();

    PostflopLine::new(
        previous_street_aggressor,
        last_role,
        relative_aggressor_order,
        hero_checked_this_street,
        // 本街第一次進攻是下注，之後都是加注
        u8::from(aggressive_count > 0),
        u8::try_from(aggressive_count.saturating_sub(1)).unwrap_or(u8::MAX),
    )
}

const fn previous_street(street: Street) -> Option<Street> {
    match street {
        Street::Preflop => None,
        Street::Flop => Some(Street::Preflop),
        Street::Turn => Some(Street::Flop),
        Street::River => Some(Street::Turn),
    }
}

fn last_aggressor_on(history: &[PublicAction], street: Street) -> Option<&PublicAction> {
    history
        .iter()
        .rfind(|action| action.street == street && action.raised)
}

const fn role_of(seat: Option<usize>, hero: usize) -> AggressorRole {
    match seat {
        None => AggressorRole::None,
        Some(seat) if seat == hero => AggressorRole::Hero,
        Some(_) => AggressorRole::Opponent,
    }
}

/// Exact interval comparisons, including values immediately around rational boundaries.
#[must_use]
pub fn facing_ratio(bet: u64, pot: u64) -> FacingSize {
    let b = u128::from(bet);
    let p = u128::from(pot.max(1));
    if b * 3 <= p {
        FacingSize::UpToThird
    } else if b * 2 <= p {
        FacingSize::ThirdToHalf
    } else if b * 3 <= p * 2 {
        FacingSize::HalfToTwoThirds
    } else if b < p {
        FacingSize::TwoThirdsToPot
    } else {
        FacingSize::PotOrMore
    }
}
fn relative_position(view: &DecisionView) -> crate::strategy::postflop::RelativePosition {
    use crate::strategy::postflop::RelativePosition as P;
    let mut before = false;
    let mut after = false;
    for opponent in view.opponents.iter().filter(|o| !o.folded && !o.all_in) {
        if opponent.position.postflop_order() < view.position.postflop_order() {
            before = true;
        } else {
            after = true;
        }
    }
    match (before, after) {
        (false, true) => P::First,
        (true, true) => P::Middle,
        (true, false) => P::Last,
        (false, false) => P::Alone,
    }
}
fn decision_phase(view: &DecisionView) -> crate::strategy::postflop::DecisionPhase {
    use crate::strategy::postflop::DecisionPhase as P;
    match view
        .history
        .iter()
        .rev()
        .find(|a| a.street == view.street && a.seat == view.seat)
    {
        None => P::Unacted,
        Some(a) if matches!(a.action, crate::betting::Action::Check) => P::Checked,
        Some(_) => P::BetRaised,
    }
}
/// Name the historical continuation of the *preflop* aggressor, not any later raiser.
fn continuation(view: &DecisionView) -> crate::strategy::postflop::Continuation {
    use crate::strategy::postflop::Continuation as C;
    let aggressor = |street| {
        view.history
            .iter()
            .rev()
            .find(|a| a.street == street && a.raised)
            .map(|a| a.seat)
    };
    let Some(pre) = aggressor(Street::Preflop) else {
        return C::Other;
    };
    // At an unopened street name Hero's opportunity; facing action name its aggressor.
    let actor = if view.to_call.units() == 0 {
        Some(view.seat)
    } else {
        aggressor(view.street)
    };
    if actor != Some(pre) {
        return C::Other;
    }
    let clean = |street| {
        let raises: Vec<_> = view
            .history
            .iter()
            .filter(|a| a.street == street && a.raised)
            .collect();
        !raises.is_empty() && raises.iter().all(|a| a.seat == pre)
    };
    if view
        .history
        .iter()
        .any(|a| a.street == view.street && a.raised && a.seat != pre)
    {
        return C::Other;
    }
    let flop = aggressor(Street::Flop);
    let turn = aggressor(Street::Turn);
    match view.street {
        Street::Flop => C::FlopCbet,
        Street::Turn if clean(Street::Flop) => C::DoubleBarrel,
        Street::River if clean(Street::Flop) && clean(Street::Turn) => C::TripleBarrel,
        Street::Turn if flop.is_none() => C::DelayedCbet,
        Street::River if flop.is_none() && turn.is_none() => C::DelayedCbet,
        _ => C::Other,
    }
}

fn legacy_facing_size(view: &DecisionView) -> FacingSize {
    if view.to_call.units() == 0 {
        return FacingSize::None;
    }
    let (bet, pot) = facing_bet_basis(view);
    match u128::from(bet) * 1000 / u128::from(pot.max(1)) {
        0..=287 => FacingSize::Quarter,
        288..=400 => FacingSize::Third,
        401..=580 => FacingSize::Half,
        581..=700 => FacingSize::TwoThirds,
        701..=870 => FacingSize::ThreeQuarters,
        871..=1150 => FacingSize::Pot,
        _ => FacingSize::Overbet,
    }
}
