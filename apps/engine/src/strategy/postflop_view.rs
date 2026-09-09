//! 由 [`DecisionView`] 推導翻後策略節點（0907 計劃 §4.1）。
//!
//! 線路、底池類型、面對尺度、SPR 這些全部由**既有的公開資訊**推導：
//! 完整的 `PublicAction` history、`OpponentPublic` 與座位順序。不新增
//! `DecisionView` 欄位——那個結構刻意沒有任何欄位能承載隱藏資訊，
//! 每加一個欄位都要重新論證它依規則已公開。

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
            !opponent.folded
                && !opponent.all_in
                && opponent.position.postflop_order() > hero_order
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
/// 以「要跟的金額 ÷ 目前底池」落檔。**不自行判定全下**：那需要知道對手
/// 是不是真的推光了，而 `all_in` 旗標講的是對手的狀態，不是這次下注的
/// 尺度；混起來會讓「短碼跟注」被當成 overbet。
fn facing_size(view: &DecisionView) -> FacingSize {
    let to_call = view.to_call.units();
    if to_call == 0 {
        return FacingSize::None;
    }
    let pot = view.pot.units().max(1);
    // 千分比，避免浮點進入規則比對
    let ratio = to_call.saturating_mul(1_000) / pot;
    match ratio {
        0..=287 => FacingSize::Quarter,
        288..=400 => FacingSize::Third,
        401..=580 => FacingSize::Half,
        581..=700 => FacingSize::TwoThirds,
        701..=870 => FacingSize::ThreeQuarters,
        871..=1_150 => FacingSize::Pot,
        _ => FacingSize::Overbet,
    }
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
        .map(|opponent| opponent.stack.units().saturating_add(opponent.committed.units()))
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
    let hero_checked_this_street = current
        .iter()
        .any(|action| action.seat == hero && matches!(action.action, crate::betting::Action::Check));
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
