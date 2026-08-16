//! 牌力評估（5～7 張取最佳五張）。
//!
//! 核心規格 5.1：「Complete-hand evaluator 必須精確。」
//! 因此這裡用逐級分類判定，不用查表法。查表快，但正確性要靠外部表檔背書；
//! 分類法的每一步都能對照撲克規則人工驗算，符合「正確性優先於速度」
//! （實做計劃第一章第 3 點）。
//!
//! 類別由高至低嚴格排序，因此由高往低判定、命中即回傳是正確的：
//! 同花順 > 四條 > 葫蘆 > 同花 > 順子 > 三條 > 兩對 > 一對 > 高牌。

use crate::card::Card;

/// 牌型類別。數值即排序基準。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Category {
    HighCard = 0,
    Pair = 1,
    TwoPair = 2,
    Trips = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    Quads = 7,
    StraightFlush = 8,
}

/// 可比較的牌力值。數值越大越強，相等即為並列（chop）。
///
/// 編碼：`category << 20 | r0<<16 | r1<<12 | r2<<8 | r3<<4 | r4`，
/// `r0..r4` 為由重要到次要的五個關鍵牌面值。因類別佔最高位，
/// 跨類別比較恆正確。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandRank(u32);

impl HandRank {
    #[must_use]
    fn new(category: Category, ranks: [u8; 5]) -> Self {
        let mut value = (category as u32) << 20;
        for (i, r) in ranks.iter().enumerate() {
            value |= u32::from(*r) << (16 - i * 4);
        }
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u32 {
        self.0
    }

    /// 由 [`HandRank::value`] 的輸出還原。供 log 與跨模組傳遞使用。
    #[must_use]
    pub const fn from_value(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn category(self) -> Category {
        match self.0 >> 20 {
            8 => Category::StraightFlush,
            7 => Category::Quads,
            6 => Category::FullHouse,
            5 => Category::Flush,
            4 => Category::Straight,
            3 => Category::Trips,
            2 => Category::TwoPair,
            1 => Category::Pair,
            _ => Category::HighCard,
        }
    }
}

/// 由 5～7 張牌評出最佳五張的牌力。
///
/// # Panics
/// 牌數少於 5 時 panic：那代表呼叫端的街別推進有誤。
#[must_use]
pub fn evaluate(cards: &[Card]) -> HandRank {
    assert!(cards.len() >= 5, "評估牌力至少需要 5 張，收到 {}", cards.len());

    // 各牌面值的張數（索引 2..=14）
    let mut rank_counts = [0u8; 15];
    // 各花色的張數與其牌面遮罩
    let mut suit_counts = [0u8; 4];
    let mut suit_masks = [0u16; 4];
    let mut rank_mask = 0u16;

    for card in cards {
        let r = card.rank.value();
        let s = card.suit.index();
        rank_counts[r as usize] += 1;
        suit_counts[s] += 1;
        suit_masks[s] |= 1 << r;
        rank_mask |= 1 << r;
    }

    // ── 同花順 ─────────────────────────────────────────────────────
    if let Some(suit) = suit_counts.iter().position(|&n| n >= 5) {
        if let Some(high) = straight_high(suit_masks[suit]) {
            return HandRank::new(Category::StraightFlush, [high, 0, 0, 0, 0]);
        }
    }

    // 依張數分組，組內由大到小
    let by_count = |want: u8| -> Vec<u8> {
        (2..=14u8)
            .rev()
            .filter(|&r| rank_counts[r as usize] == want)
            .collect()
    };
    let quads = by_count(4);
    let trips = by_count(3);
    let pairs = by_count(2);

    // ── 四條 ───────────────────────────────────────────────────────
    if let Some(&quad) = quads.first() {
        let kicker = highest_excluding(&rank_counts, &[quad], 1);
        return HandRank::new(Category::Quads, [quad, kicker[0], 0, 0, 0]);
    }

    // ── 葫蘆 ───────────────────────────────────────────────────────
    // 兩組三條時，較小的那組充當對子（例：777 555 → 777 55）
    if let Some(&set) = trips.first() {
        let pair = trips.get(1).copied().or_else(|| pairs.first().copied());
        if let Some(pair) = pair {
            return HandRank::new(Category::FullHouse, [set, pair, 0, 0, 0]);
        }
    }

    // ── 同花 ───────────────────────────────────────────────────────
    if let Some(suit) = suit_counts.iter().position(|&n| n >= 5) {
        let top = top_ranks_from_mask(suit_masks[suit], 5);
        return HandRank::new(Category::Flush, top);
    }

    // ── 順子 ───────────────────────────────────────────────────────
    if let Some(high) = straight_high(rank_mask) {
        return HandRank::new(Category::Straight, [high, 0, 0, 0, 0]);
    }

    // ── 三條 ───────────────────────────────────────────────────────
    if let Some(&set) = trips.first() {
        let k = highest_excluding(&rank_counts, &[set], 2);
        return HandRank::new(Category::Trips, [set, k[0], k[1], 0, 0]);
    }

    // ── 兩對 ───────────────────────────────────────────────────────
    if pairs.len() >= 2 {
        let (high, low) = (pairs[0], pairs[1]);
        let k = highest_excluding(&rank_counts, &[high, low], 1);
        return HandRank::new(Category::TwoPair, [high, low, k[0], 0, 0]);
    }

    // ── 一對 ───────────────────────────────────────────────────────
    if let Some(&pair) = pairs.first() {
        let k = highest_excluding(&rank_counts, &[pair], 3);
        return HandRank::new(Category::Pair, [pair, k[0], k[1], k[2], 0]);
    }

    // ── 高牌 ───────────────────────────────────────────────────────
    HandRank::new(Category::HighCard, top_ranks_from_mask(rank_mask, 5))
}

/// 由牌面遮罩找出最高的順子頂張；無順子回傳 `None`。
///
/// A-2-3-4-5（輪子）另行處理：把 A 視為 1，頂張為 5。
fn straight_high(mask: u16) -> Option<u8> {
    // 輪子：A 補到第 1 位
    let wheel_mask = if mask & (1 << 14) != 0 {
        mask | 1 << 1
    } else {
        mask
    };
    (5..=14u8).rev().find(|&high| {
        (0..5).all(|offset| wheel_mask & (1 << (high - offset)) != 0)
    })
}

/// 由遮罩取出最高的 n 個牌面值，不足處補 0。
fn top_ranks_from_mask(mask: u16, n: usize) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut filled = 0;
    for r in (2..=14u8).rev() {
        if filled == n {
            break;
        }
        if mask & (1 << r) != 0 {
            out[filled] = r;
            filled += 1;
        }
    }
    out
}

/// 取出最高的 n 個「不在排除清單內」的牌面值，供 kicker 使用。
fn highest_excluding(rank_counts: &[u8; 15], exclude: &[u8], n: usize) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut filled = 0;
    for r in (2..=14u8).rev() {
        if filled == n {
            break;
        }
        if rank_counts[r as usize] > 0 && !exclude.contains(&r) {
            out[filled] = r;
            filled += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Card;

    fn hand(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|t| Card::parse(t).unwrap_or_else(|| panic!("無法解析牌張 {t}")))
            .collect()
    }

    #[test]
    fn 輪子順子的頂張為_5() {
        let r = evaluate(&hand("As 2d 3c 4h 5s"));
        assert_eq!(r.category(), Category::Straight);
        // 頂張 5 的順子必須弱於頂張 6 的順子
        assert!(r < evaluate(&hand("2s 3d 4c 5h 6s")));
    }

    #[test]
    fn 輪子同花順弱於六高同花順() {
        let steel = evaluate(&hand("As 2s 3s 4s 5s"));
        assert_eq!(steel.category(), Category::StraightFlush);
        assert!(steel < evaluate(&hand("2s 3s 4s 5s 6s")));
    }

    #[test]
    fn 兩組三條組成葫蘆時取較小者當對子() {
        // 777 555 + K → 葫蘆 7 帶 5，而非 7 帶 K
        let r = evaluate(&hand("7s 7d 7c 5s 5d 5c Kh"));
        assert_eq!(r.category(), Category::FullHouse);
        assert_eq!(r, evaluate(&hand("7s 7d 7c 5s 5d 2c 3h")));
    }

    #[test]
    fn 七張中不得用第六第七張湊高牌() {
        // 高牌只取最高五張
        let r = evaluate(&hand("As Kd Qc Jh 9s 3d 2c"));
        assert_eq!(r, evaluate(&hand("As Kd Qc Jh 9s")));
    }
}
