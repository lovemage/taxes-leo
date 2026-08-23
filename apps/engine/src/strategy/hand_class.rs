//! 169 個起手牌類別。
//!
//! 核心規格 4.1／UI 規格 D.4：13×13 網格，對角線為對子（13）、
//! 上三角為同花（78）、下三角為非同花（78），合計 169。
//! v1 的策略編輯與 baseline 內容都以這 169 類為單位，combo 級下鑽列後續。

use crate::card::{Card, Rank};

/// 起手牌類別。`high` 恆不小於 `low`；對子的 `suited` 恆為 false。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandClass {
    high: Rank,
    low: Rank,
    suited: bool,
}

impl HandClass {
    /// 由兩張底牌歸類。
    #[must_use]
    pub fn from_cards(a: Card, b: Card) -> Self {
        let (high, low) = if a.rank >= b.rank {
            (a.rank, b.rank)
        } else {
            (b.rank, a.rank)
        };
        Self {
            high,
            low,
            // 對子不可能同花（同一牌面的兩張必為不同花色）
            suited: high != low && a.suit == b.suit,
        }
    }

    #[must_use]
    pub const fn high(self) -> Rank {
        self.high
    }

    #[must_use]
    pub const fn low(self) -> Rank {
        self.low
    }

    #[must_use]
    pub const fn is_pair(self) -> bool {
        matches!(self.high.value(), v if v == self.low.value())
    }

    #[must_use]
    pub const fn is_suited(self) -> bool {
        self.suited
    }

    /// 該類別涵蓋的實際 combo 數：對子 6、同花 4、非同花 12。
    ///
    /// 169 類**不等權**。牌手講的「範圍寬度 X%」是 1,326 個 combo 的
    /// 百分比，不是 169 類的百分比。用類別等權會把同花高估近一倍
    /// （46.2% vs 23.5%）、非同花低估三分之一（46.2% vs 70.6%），
    /// 算出來的寬度與顧問心中的數字對不上。
    #[must_use]
    pub const fn combos(self) -> u16 {
        if self.is_pair() {
            6
        } else if self.suited {
            4
        } else {
            12
        }
    }

    /// 兩張牌之間的間隔。相連為 0（AK、87），一洞為 1（AQ、86）。
    /// 對子恆為 0。
    #[must_use]
    pub const fn gap(self) -> u8 {
        let high = self.high.value();
        let low = self.low.value();
        if high <= low {
            0
        } else {
            high - low - 1
        }
    }

    /// 是否為 broadway（兩張皆 T 以上）。
    #[must_use]
    pub const fn is_broadway(self) -> bool {
        self.low.value() >= 10
    }

    /// 最高張是否為 A。
    #[must_use]
    pub const fn has_ace(self) -> bool {
        matches!(self.high.value(), 14)
    }

    /// 13×13 網格座標 `(row, col)`。
    ///
    /// 列與欄皆由 A 到 2。同花在上三角（col > row），非同花在下三角。
    #[must_use]
    pub fn grid(self) -> (usize, usize) {
        let high_index = usize::from(14 - self.high.value());
        let low_index = usize::from(14 - self.low.value());
        if self.suited {
            (high_index, low_index)
        } else {
            (low_index, high_index)
        }
    }

    /// 0..169 的唯一索引，供策略表的欄位鍵使用。
    #[must_use]
    pub fn index(self) -> usize {
        let (row, col) = self.grid();
        row * 13 + col
    }

    /// 策略內容與 UI 共用的字串表示（`AA`／`AKs`／`AKo`）。
    #[must_use]
    pub fn label(self) -> String {
        let high = self.high.symbol();
        let low = self.low.symbol();
        if self.is_pair() {
            format!("{high}{low}")
        } else if self.suited {
            format!("{high}{low}s")
        } else {
            format!("{high}{low}o")
        }
    }

    /// 由 [`Self::label`] 產生的字串反解回類別。
    ///
    /// 兩邊必須對稱：標籤是策略內容、覆寫節點鍵與離線排序資產共用的
    /// 欄位名，解不回來就等於那筆資料悄悄落在別的類別上。
    #[must_use]
    pub fn from_label(text: &str) -> Option<Self> {
        Self::all().into_iter().find(|class| class.label() == text)
    }

    /// 全部 169 類，依索引遞增。
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut out = Vec::with_capacity(169);
        for (row, &high) in RANKS_HIGH_TO_LOW.iter().enumerate() {
            for (col, &low) in RANKS_HIGH_TO_LOW.iter().enumerate() {
                out.push(if row == col {
                    Self {
                        high,
                        low,
                        suited: false,
                    }
                } else if col > row {
                    // 上三角：同花，高張為列
                    Self {
                        high,
                        low,
                        suited: true,
                    }
                } else {
                    // 下三角：非同花，高張為欄
                    Self {
                        high: low,
                        low: high,
                        suited: false,
                    }
                });
            }
        }
        out
    }
}

const RANKS_HIGH_TO_LOW: [Rank; 13] = [
    Rank::Ace,
    Rank::King,
    Rank::Queen,
    Rank::Jack,
    Rank::Ten,
    Rank::Nine,
    Rank::Eight,
    Rank::Seven,
    Rank::Six,
    Rank::Five,
    Rank::Four,
    Rank::Three,
    Rank::Two,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{full_deck, Suit};

    #[test]
    fn 恰好_169_類且索引互異() {
        let all = HandClass::all();
        assert_eq!(all.len(), 169);

        let mut indices: Vec<usize> = all.iter().map(|c| c.index()).collect();
        indices.sort_unstable();
        indices.dedup();
        assert_eq!(indices.len(), 169, "169 類的索引必須互異");

        let pairs = all.iter().filter(|c| c.is_pair()).count();
        let suited = all.iter().filter(|c| c.is_suited()).count();
        let offsuit = all.iter().filter(|c| !c.is_pair() && !c.is_suited()).count();
        assert_eq!((pairs, suited, offsuit), (13, 78, 78), "13 對子 + 78 同花 + 78 非同花");
    }

    #[test]
    fn combo_數逐類與實際發牌一致() {
        let deck = full_deck();
        let mut counted = std::collections::HashMap::new();
        for (i, &a) in deck.iter().enumerate() {
            for &b in &deck[i + 1..] {
                *counted.entry(HandClass::from_cards(a, b)).or_insert(0_u16) += 1;
            }
        }
        for class in HandClass::all() {
            assert_eq!(
                class.combos(),
                counted[&class],
                "{} 的 combo 數與實際發牌不符",
                class.label()
            );
        }
    }

    #[test]
    fn combo_總數為_1326() {
        let total: u32 = HandClass::all().iter().map(|c| u32::from(c.combos())).sum();
        assert_eq!(total, 1_326, "C(52,2) = 1326");
    }

    #[test]
    fn 三種牌型的_combo_權重與類別權重顯著不同() {
        let all = HandClass::all();
        let suited_combos: u32 = all
            .iter()
            .filter(|c| c.is_suited())
            .map(|c| u32::from(c.combos()))
            .sum();
        // 同花占 78/169 = 46.2% 的類別，卻只占 312/1326 = 23.5% 的 combo。
        // 這個差距正是「必須以 combo 加權」的理由
        assert_eq!(suited_combos, 312);
        assert!(
            suited_combos * 169 < 78 * 1_326 * 3 / 5,
            "同花的 combo 權重應遠低於類別權重"
        );
    }

    #[test]
    fn 由底牌歸類與順序無關() {
        let deck = full_deck();
        for (i, &a) in deck.iter().enumerate() {
            for &b in &deck[i + 1..] {
                assert_eq!(
                    HandClass::from_cards(a, b),
                    HandClass::from_cards(b, a),
                    "{a}{b} 的歸類不得因順序改變"
                );
            }
        }
    }

    #[test]
    fn 全部_1326_組合都落在_169_類中() {
        let deck = full_deck();
        let valid: std::collections::HashSet<HandClass> = HandClass::all().into_iter().collect();
        let mut combos = 0;
        for (i, &a) in deck.iter().enumerate() {
            for &b in &deck[i + 1..] {
                let class = HandClass::from_cards(a, b);
                assert!(valid.contains(&class), "{} 不在 169 類中", class.label());
                combos += 1;
            }
        }
        assert_eq!(combos, 1326, "C(52,2) = 1326");
    }

    #[test]
    fn 間隔計算正確() {
        assert_eq!(class_of_pair(Rank::Ace).gap(), 0, "對子間隔為 0");
        assert_eq!(class_of_two(Rank::Ace, Rank::King).gap(), 0, "AK 相連");
        assert_eq!(class_of_two(Rank::Ace, Rank::Queen).gap(), 1, "AQ 一洞");
        assert_eq!(class_of_two(Rank::Eight, Rank::Seven).gap(), 0, "87 相連");
        assert_eq!(class_of_two(Rank::King, Rank::Two).gap(), 10, "K2 相隔甚遠");
    }

    #[test]
    fn broadway_與_ace_判定正確() {
        assert!(class_of_two(Rank::King, Rank::Ten).is_broadway());
        assert!(!class_of_two(Rank::King, Rank::Nine).is_broadway());
        assert!(class_of_two(Rank::Ace, Rank::Two).has_ace());
        assert!(!class_of_two(Rank::King, Rank::Queen).has_ace());
    }

    fn class_of_pair(rank: Rank) -> HandClass {
        HandClass::from_cards(Card::new(rank, Suit::Spades), Card::new(rank, Suit::Hearts))
    }

    fn class_of_two(high: Rank, low: Rank) -> HandClass {
        HandClass::from_cards(Card::new(high, Suit::Spades), Card::new(low, Suit::Hearts))
    }

    #[test]
    fn 標籤符合慣例() {
        let ace_spade = Card::new(Rank::Ace, Suit::Spades);
        let ace_heart = Card::new(Rank::Ace, Suit::Hearts);
        let king_spade = Card::new(Rank::King, Suit::Spades);
        let king_heart = Card::new(Rank::King, Suit::Hearts);

        assert_eq!(HandClass::from_cards(ace_spade, ace_heart).label(), "AA");
        assert_eq!(HandClass::from_cards(ace_spade, king_spade).label(), "AKs");
        assert_eq!(HandClass::from_cards(ace_spade, king_heart).label(), "AKo");
    }

    #[test]
    fn 對角線為對子上三角為同花() {
        let all = HandClass::all();
        for class in all {
            let (row, col) = class.grid();
            if row == col {
                assert!(class.is_pair(), "對角線必須是對子");
            } else if col > row {
                assert!(class.is_suited(), "上三角必須是同花");
            } else {
                assert!(!class.is_suited() && !class.is_pair(), "下三角必須是非同花");
            }
        }
    }
}
