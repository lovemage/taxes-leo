//! 牌張表示。
//!
//! 花色順序無牌力意義（德州撲克不比花色），僅用於 side pot 之外的
//! 顯示與 log 還原。UI 規格 V.9 採四色牌組，色彩對應在前端處理。

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Suit {
    Clubs = 0,
    Diamonds = 1,
    Hearts = 2,
    Spades = 3,
}

impl Suit {
    pub const ALL: [Self; 4] = [Self::Clubs, Self::Diamonds, Self::Hearts, Self::Spades];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn symbol(self) -> char {
        match self {
            Self::Clubs => 'c',
            Self::Diamonds => 'd',
            Self::Hearts => 'h',
            Self::Spades => 's',
        }
    }
}

/// 牌面大小。數值即比較基準，A 恆為 14；A-2-3-4-5 順子的 A 當 1 用，
/// 由 `eval` 模組在偵測順子時單獨處理，不改變本型別的數值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Rank {
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13,
    Ace = 14,
}

impl Rank {
    pub const ALL: [Self; 13] = [
        Self::Two,
        Self::Three,
        Self::Four,
        Self::Five,
        Self::Six,
        Self::Seven,
        Self::Eight,
        Self::Nine,
        Self::Ten,
        Self::Jack,
        Self::Queen,
        Self::King,
        Self::Ace,
    ];

    #[must_use]
    pub const fn value(self) -> u8 {
        self as u8
    }

    #[must_use]
    pub const fn symbol(self) -> char {
        match self {
            Self::Two => '2',
            Self::Three => '3',
            Self::Four => '4',
            Self::Five => '5',
            Self::Six => '6',
            Self::Seven => '7',
            Self::Eight => '8',
            Self::Nine => '9',
            Self::Ten => 'T',
            Self::Jack => 'J',
            Self::Queen => 'Q',
            Self::King => 'K',
            Self::Ace => 'A',
        }
    }

    #[must_use]
    pub const fn from_value(value: u8) -> Option<Self> {
        Some(match value {
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            6 => Self::Six,
            7 => Self::Seven,
            8 => Self::Eight,
            9 => Self::Nine,
            10 => Self::Ten,
            11 => Self::Jack,
            12 => Self::Queen,
            13 => Self::King,
            14 => Self::Ace,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

impl Card {
    #[must_use]
    pub const fn new(rank: Rank, suit: Suit) -> Self {
        Self { rank, suit }
    }

    /// 由 `"As"`、`"Td"`、`"2c"` 這類兩字元表示法解析。log 與測試共用。
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        let rank_char = chars.next()?;
        let suit_char = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let rank = match rank_char.to_ascii_uppercase() {
            '2' => Rank::Two,
            '3' => Rank::Three,
            '4' => Rank::Four,
            '5' => Rank::Five,
            '6' => Rank::Six,
            '7' => Rank::Seven,
            '8' => Rank::Eight,
            '9' => Rank::Nine,
            'T' => Rank::Ten,
            'J' => Rank::Jack,
            'Q' => Rank::Queen,
            'K' => Rank::King,
            'A' => Rank::Ace,
            _ => return None,
        };
        let suit = match suit_char.to_ascii_lowercase() {
            'c' => Suit::Clubs,
            'd' => Suit::Diamonds,
            'h' => Suit::Hearts,
            's' => Suit::Spades,
            _ => return None,
        };
        Some(Self::new(rank, suit))
    }

    /// 0..52 的唯一索引，供牌堆與緊湊序列化使用。
    #[must_use]
    pub const fn index(self) -> usize {
        (self.rank as usize - 2) * 4 + self.suit as usize
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank.symbol(), self.suit.symbol())
    }
}

/// 完整 52 張牌，順序固定（供 seed 化洗牌使用）。
#[must_use]
pub fn full_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(52);
    for rank in Rank::ALL {
        for suit in Suit::ALL {
            deck.push(Card::new(rank, suit));
        }
    }
    deck
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 完整牌堆為_52_張且索引互異() {
        let deck = full_deck();
        assert_eq!(deck.len(), 52);
        let mut indices: Vec<usize> = deck.iter().map(|c| c.index()).collect();
        indices.sort_unstable();
        indices.dedup();
        assert_eq!(indices.len(), 52, "52 張牌的索引必須互異");
        assert_eq!(indices[0], 0);
        assert_eq!(indices[51], 51);
    }

    #[test]
    fn 解析與顯示互為反向() {
        for card in full_deck() {
            let text = card.to_string();
            assert_eq!(Card::parse(&text), Some(card), "解析 {text} 失敗");
        }
        assert_eq!(Card::parse("Xs"), None);
        assert_eq!(Card::parse("Ax"), None);
        assert_eq!(Card::parse("As2"), None);
    }
}
