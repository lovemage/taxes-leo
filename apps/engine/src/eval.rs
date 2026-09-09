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

    /// 由重要到次要的五個牌面值（含踢腳）。
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn ranks(self) -> [u8; 5] {
        let mut out = [0u8; 5];
        let mut i = 0;
        while i < 5 {
            out[i] = ((self.0 >> (16 - i * 4)) & 0xF) as u8;
            i += 1;
        }
        out
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

/// 牌型的**關鍵牌值**，不含踢腳。
///
/// 用來回答「這手牌有沒有比公共牌本身更強」。踢腳被排除在外是刻意的：
/// 公對面上握 `AK` 的手，最佳五張是三條加 A 踢腳，但那個 A 完全沒有
/// 改善公共牌已經給的三條——把踢腳算成改善，整個牌力組會被公共牌灌水。
///
/// `Ord` 的語意即「類別較大，或類別相同但關鍵牌值較高」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyRank {
    pub category: Category,
    /// 由重要到次要的關鍵牌值；未使用的位置為 0
    pub key_ranks: [u8; 5],
}

impl KeyRank {
    /// 某類別有幾個牌值屬於「牌型本身」而非踢腳。
    #[must_use]
    pub const fn key_count(category: Category) -> usize {
        match category {
            // 高張沒有牌型，五張全是踢腳
            Category::HighCard => 0,
            Category::Pair | Category::Trips | Category::Quads => 1,
            Category::Straight | Category::StraightFlush => 1,
            Category::TwoPair | Category::FullHouse => 2,
            // 同花的五張都參與牌型，沒有踢腳可言
            Category::Flush => 5,
        }
    }

    #[must_use]
    fn from_hand_rank(rank: HandRank) -> Self {
        let category = rank.category();
        let all = rank.ranks();
        let count = Self::key_count(category);
        let mut key_ranks = [0u8; 5];
        key_ranks[..count].copy_from_slice(&all[..count]);
        Self {
            category,
            key_ranks,
        }
    }
}

/// 5～7 張牌的關鍵牌值。
///
/// # Panics
/// 牌數少於 5 時 panic，與 [`evaluate`] 相同。
#[must_use]
pub fn key_rank(cards: &[Card]) -> KeyRank {
    KeyRank::from_hand_rank(evaluate(cards))
}

/// **公共牌自身**的最佳部分牌型（3～5 張）。
///
/// [`evaluate`] 至少需要 5 張牌，因此翻牌（3 張）與轉牌（4 張）的公共牌
/// 根本進不去。而「英雄七張的最佳牌型是否優於只用公共牌的牌型」在翻牌
/// 又恆為真——英雄只有五張可用，最佳五張必然含兩張底牌。兩個問題合起來
/// 讓原本的定義在翻牌與轉牌不可計算，因此改為直接算公共牌自己。
///
/// 3／4 張時只可能是高張／一對／兩對（4 張）／三條／四條（4 張）：
/// 順子與同花由公共牌獨立成立，最少也要五張。
///
/// # Panics
/// 牌數不在 3～5 之間時 panic。
#[must_use]
pub fn board_partial_rank(board: &[Card]) -> KeyRank {
    assert!(
        (3..=5).contains(&board.len()),
        "公共牌必須是 3～5 張，收到 {}",
        board.len()
    );

    if board.len() == 5 {
        return key_rank(board);
    }

    let mut counts = [0u8; 15];
    for card in board {
        counts[card.rank.value() as usize] += 1;
    }
    let by_count = |want: u8| -> Vec<u8> {
        (2..=14u8)
            .rev()
            .filter(|&r| counts[r as usize] == want)
            .collect()
    };

    let mut key_ranks = [0u8; 5];
    if let Some(&quad) = by_count(4).first() {
        key_ranks[0] = quad;
        return KeyRank {
            category: Category::Quads,
            key_ranks,
        };
    }
    if let Some(&trip) = by_count(3).first() {
        key_ranks[0] = trip;
        return KeyRank {
            category: Category::Trips,
            key_ranks,
        };
    }
    let pairs = by_count(2);
    match pairs.as_slice() {
        [high, low, ..] => {
            key_ranks[0] = *high;
            key_ranks[1] = *low;
            KeyRank {
                category: Category::TwoPair,
                key_ranks,
            }
        }
        [only] => {
            key_ranks[0] = *only;
            KeyRank {
                category: Category::Pair,
                key_ranks,
            }
        }
        [] => KeyRank {
            category: Category::HighCard,
            key_ranks,
        },
    }
}

/// 英雄的底牌是否**確實改善**了公共牌。
///
/// 條件：英雄成牌的類別嚴格大於公共牌自身的類別，或類別相同但關鍵牌值
/// 嚴格較高。**踢腳一律不算改善**——這是「成牌」定義的一半，另一半是
/// 成牌類別至少一對（計劃 §2.3、§3.2）。
///
/// # Panics
/// 公共牌不在 3～5 張時 panic。
#[must_use]
pub fn improves_board(hole: [Card; 2], board: &[Card]) -> bool {
    let mut seven = Vec::with_capacity(board.len() + 2);
    seven.extend_from_slice(board);
    seven.extend_from_slice(&hole);
    key_rank(&seven) > board_partial_rank(board)
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

    // ── 公共牌部分牌型與「改善公共牌」（計劃 §3.2）────────────────

    fn hole(text: &str) -> [Card; 2] {
        let cards = hand(text);
        [cards[0], cards[1]]
    }

    #[test]
    fn 公共牌部分牌型在三張與四張時仍算得出來() {
        // evaluate 至少要五張，公共牌自己進不去，因此另走一條路
        assert_eq!(
            board_partial_rank(&hand("As 7d 2c")).category,
            Category::HighCard
        );
        assert_eq!(
            board_partial_rank(&hand("Ks Kd 7c")).category,
            Category::Pair
        );
        assert_eq!(
            board_partial_rank(&hand("Ks Kd 7c 7h")).category,
            Category::TwoPair
        );
        assert_eq!(
            board_partial_rank(&hand("2s 2d 2c")).category,
            Category::Trips
        );
        assert_eq!(
            board_partial_rank(&hand("2s 2d 2c 2h")).category,
            Category::Quads
        );
    }

    #[test]
    fn 三張與四張公共牌不可能自成順子或同花() {
        // 三張同花色、三張連號都還不夠；順子與同花最少要五張
        assert_eq!(
            board_partial_rank(&hand("9s 8s 7s")).category,
            Category::HighCard
        );
        assert_eq!(
            board_partial_rank(&hand("9s 8s 7s 6s")).category,
            Category::HighCard
        );
        // 五張才走完整評估
        assert_eq!(
            board_partial_rank(&hand("9s 8s 7s 6s 5s")).category,
            Category::StraightFlush
        );
    }

    #[test]
    fn 改善公共牌的_golden_向量() {
        // 計劃 §3.2 的對照表，逐條釘住
        let cases: [(&str, &str, bool); 5] = [
            // 三條面上的 AK：最佳五張是三條加 A 踢腳，但那個 A 沒有改善任何東西
            ("As Kd", "2s 2d 2c", false),
            // 同一個牌面，KK 湊出葫蘆
            ("Ks Kd", "2s 2d 2c", true),
            // 公對面上的 A2：一對 K 完全來自公共牌
            ("As 2d", "Ks Kd 7c", false),
            // 同一個牌面，77 湊出葫蘆
            ("7s 7d", "Ks Kd 7c", true),
            // 河牌公共牌自己就是順子，任何兩張無關牌都沒有改善它
            ("3s 2d", "As Kd Qc Jh Ts", false),
        ];

        for (hole_text, board_text, expected) in cases {
            assert_eq!(
                improves_board(hole(hole_text), &hand(board_text)),
                expected,
                "底牌 {hole_text} 對公共牌 {board_text}"
            );
        }
    }

    #[test]
    fn 踢腳一律不算改善() {
        // 公對面上握 A 高：踢腳從 7 變成 A，牌型完全沒變
        assert!(!improves_board(hole("Ah 3d"), &hand("Ks Kd 7c")));
        // 但握 K 就是三條，那是真的改善
        assert!(improves_board(hole("Kh 3d"), &hand("Ks Kd 7c")));
    }

    #[test]
    fn 同類別時比較關鍵牌值而不是踢腳() {
        // 公共牌一對 7，英雄一對 K：同樣是「一對」，但對子點數更高
        let board = hand("7s 7d 2c");
        assert!(improves_board(hole("Ks Kh"), &board));

        // 英雄一對 3：兩對（7 與 3）勝過單一對 7
        assert!(improves_board(hole("3s 3h"), &board));
    }

    #[test]
    fn 翻牌握任何底牌都不會恆為改善() {
        // 舊定義「英雄最佳五張是否優於只用公共牌」在翻牌恆為真——
        // 英雄只有五張可用，最佳五張必然含兩張底牌。新定義不會這樣
        let board = hand("As Ad Ac");
        assert!(
            !improves_board(hole("7s 2d"), &board),
            "三條 A 的面上握 72 什麼也沒改善"
        );
    }

    #[test]
    fn 關鍵牌值的個數依類別決定() {
        assert_eq!(KeyRank::key_count(Category::HighCard), 0);
        assert_eq!(KeyRank::key_count(Category::Pair), 1);
        assert_eq!(KeyRank::key_count(Category::TwoPair), 2);
        assert_eq!(KeyRank::key_count(Category::FullHouse), 2);
        assert_eq!(KeyRank::key_count(Category::Flush), 5, "同花的五張都參與牌型");
    }
}
