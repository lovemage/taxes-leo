//! 範圍追蹤：自我 range 與由公開資訊推導的對手 range。
//!
//! 核心規格 2.4：`StrategyProvider` 的輸入是 `DecisionView`、自身 range 與
//! **由公開資訊推導的**對手 range estimates。
//! 核心規格 5.1：聯合範圍必須套用 **reach weight** 與 **card removal**。
//!
//! # 為什麼是 combo 層級而不是 169 類
//!
//! Card removal 只有在 combo 層才算得對。公共牌出現 `As` 時，`AA` 從 6 個
//! combo 掉到 3 個、`AKs` 從 4 個掉到 3 個、`AKo` 從 12 個掉到 9 個——
//! 三者的削減比例都不同。用 169 類等權處理會把 blocker 效應整個抹平，
//! 而 blocker 正是翻前翻後決策的關鍵之一。
//!
//! 1,326 個 combo 的權重陣列只有幾 KB，成本可以接受。
//!
//! # 隔離性
//!
//! 本模組**沒有任何管道取得他人底牌**。對手範圍只由「觀察到的公開行動」
//! 加「對該行動的頻率假設」推導。頻率假設由呼叫端提供，型別上收的是
//! `HandClass -> Myriad` 的函式，拿不到實際牌。

use crate::card::{full_deck, Card};
use crate::strategy::distribution::{Myriad, FULL};
use crate::strategy::hand_class::HandClass;

/// C(52,2)。
pub const COMBO_COUNT: usize = 1_326;

/// 由兩張牌算出 0..1326 的唯一索引。
///
/// # Panics
/// 兩張牌相同時 panic。
#[must_use]
pub fn combo_index(a: Card, b: Card) -> usize {
    let (low, high) = {
        let (x, y) = (a.index(), b.index());
        assert_ne!(x, y, "combo 不得由同一張牌組成");
        if x < y {
            (x, y)
        } else {
            (y, x)
        }
    };
    // base(low) = Σ_{k<low}(51-k) = 51·low - low(low-1)/2
    let base = 51 * low - low * low.saturating_sub(1) / 2;
    base + (high - low - 1)
}

/// 全部 1,326 個 combo，依索引遞增。
#[must_use]
pub fn all_combos() -> Vec<(Card, Card)> {
    let deck = full_deck();
    let mut sorted = deck;
    sorted.sort_by_key(|card: &Card| card.index());

    let mut out = vec![(sorted[0], sorted[1]); COMBO_COUNT];
    for (i, &a) in sorted.iter().enumerate() {
        for &b in &sorted[i + 1..] {
            out[combo_index(a, b)] = (a, b);
        }
    }
    out
}

/// 一個座位的範圍：1,326 個 combo 的到達權重（萬分比）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    weights: Vec<Myriad>,
}

impl Range {
    /// 全範圍：每個 combo 權重為 100%。
    #[must_use]
    pub fn full() -> Self {
        Self {
            weights: vec![FULL; COMBO_COUNT],
        }
    }

    /// 空範圍。
    #[must_use]
    pub fn empty() -> Self {
        Self {
            weights: vec![0; COMBO_COUNT],
        }
    }

    /// 由 169 類的權重展開為 combo 權重。
    ///
    /// 同一類別內的 combo 共用該類權重——類別層的策略內容本來就不區分
    /// 類內 combo，這裡如實展開，不額外造假細節。
    #[must_use]
    pub fn from_class_weights(class_weight: &impl Fn(HandClass) -> Myriad) -> Self {
        let mut weights = vec![0; COMBO_COUNT];
        for (a, b) in all_combos() {
            weights[combo_index(a, b)] = class_weight(HandClass::from_cards(a, b));
        }
        Self { weights }
    }

    #[must_use]
    pub fn weight_of(&self, a: Card, b: Card) -> Myriad {
        self.weights[combo_index(a, b)]
    }

    pub fn set_weight(&mut self, a: Card, b: Card, weight: Myriad) {
        self.weights[combo_index(a, b)] = weight;
    }

    /// 仍有正權重的 combo 數。
    #[must_use]
    pub fn live_combos(&self) -> usize {
        self.weights.iter().filter(|w| **w > 0).count()
    }

    /// 全部權重的總和，供比較範圍大小。
    #[must_use]
    pub fn total_weight(&self) -> u64 {
        self.weights.iter().map(|w| u64::from(*w)).sum()
    }

    /// 某手牌類別在本範圍中仍存活的 combo 數。
    ///
    /// Card removal 之後，同一類別的存活數會低於 [`HandClass::combos`]。
    #[must_use]
    pub fn live_combos_of(&self, class: HandClass) -> usize {
        all_combos()
            .into_iter()
            .filter(|(a, b)| {
                HandClass::from_cards(*a, *b) == class && self.weight_of(*a, *b) > 0
            })
            .count()
    }

    /// Card removal：移除任何含有已知牌的 combo。
    ///
    /// 已知牌包含公共牌與英雄自己的底牌。核心規格 5.1 要求聯合範圍
    /// 必須套用 card removal，否則會把不可能的組合算進 equity。
    pub fn remove_cards(&mut self, known: &[Card]) {
        if known.is_empty() {
            return;
        }
        for (a, b) in all_combos() {
            if known.contains(&a) || known.contains(&b) {
                self.weights[combo_index(a, b)] = 0;
            }
        }
    }

    /// Reach weighting：依觀察到的公開行動更新權重。
    ///
    /// `frequency` 回傳「該手牌採取此行動的頻率」（萬分比）。
    /// 新權重 = 舊權重 × 頻率，即貝氏更新的分子部分；
    /// 不做正規化，因為範圍的**相對**權重才是後續計算需要的。
    ///
    /// 型別上只收 `HandClass -> Myriad`，因此無法從這個介面取得實際牌張，
    /// 對應核心規格 2.4「只使用公開資訊」。
    pub fn apply_reach(&mut self, frequency: &impl Fn(HandClass) -> Myriad) {
        for (a, b) in all_combos() {
            let index = combo_index(a, b);
            if self.weights[index] == 0 {
                continue;
            }
            let f = frequency(HandClass::from_cards(a, b));
            let updated = u64::from(self.weights[index]) * u64::from(f) / u64::from(FULL);
            self.weights[index] = Myriad::try_from(updated).unwrap_or(FULL);
        }
    }

    /// 各手牌類別的權重總和，供 UI 以 13×13 呈現對手範圍。
    #[must_use]
    pub fn class_totals(&self) -> Vec<u64> {
        let mut totals = vec![0u64; 169];
        for (a, b) in all_combos() {
            let class = HandClass::from_cards(a, b);
            totals[class.index()] += u64::from(self.weight_of(a, b));
        }
        totals
    }
}

/// 全桌的範圍追蹤。
///
/// **結構上沒有任何欄位能承載他人底牌**，與 `DecisionView` 同一個原則：
/// 隔離不是靠自律，是靠型別。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeTracker {
    ranges: Vec<Range>,
    /// 已知牌：英雄底牌與已發出的公共牌
    known: Vec<Card>,
}

impl RangeTracker {
    /// 開新一手：所有座位為全範圍。
    #[must_use]
    pub fn new(seats: usize) -> Self {
        Self {
            ranges: vec![Range::full(); seats],
            known: Vec::new(),
        }
    }

    /// 登記已知牌並對全部範圍套用 card removal。
    ///
    /// 英雄的底牌也要登記——它們同樣是對手不可能持有的牌（blocker）。
    pub fn observe_known_cards(&mut self, cards: &[Card]) {
        for card in cards {
            if !self.known.contains(card) {
                self.known.push(*card);
            }
        }
        for range in &mut self.ranges {
            range.remove_cards(cards);
        }
    }

    /// 依觀察到的公開行動更新某座範圍。
    pub fn observe_action(&mut self, seat: usize, frequency: &impl Fn(HandClass) -> Myriad) {
        if let Some(range) = self.ranges.get_mut(seat) {
            range.apply_reach(frequency);
        }
    }

    /// 該座棄牌後不再參與，範圍清空。
    pub fn fold(&mut self, seat: usize) {
        if let Some(range) = self.ranges.get_mut(seat) {
            *range = Range::empty();
        }
    }

    #[must_use]
    pub fn range_of(&self, seat: usize) -> &Range {
        &self.ranges[seat]
    }

    #[must_use]
    pub fn known_cards(&self) -> &[Card] {
        &self.known
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Rank, Suit};

    fn card(text: &str) -> Card {
        Card::parse(text).expect("牌張")
    }

    #[test]
    fn combo_索引涵蓋全部_1326_且互異() {
        let deck = full_deck();
        let mut seen = vec![false; COMBO_COUNT];
        let mut count = 0;
        for (i, &a) in deck.iter().enumerate() {
            for &b in &deck[i + 1..] {
                let index = combo_index(a, b);
                assert!(index < COMBO_COUNT, "{a}{b} 的索引 {index} 越界");
                assert!(!seen[index], "{a}{b} 的索引重複");
                seen[index] = true;
                count += 1;
            }
        }
        assert_eq!(count, COMBO_COUNT);
        assert!(seen.iter().all(|s| *s), "1326 個索引必須全部被用到");
    }

    #[test]
    fn combo_索引與順序無關() {
        let a = card("As");
        let b = card("Kd");
        assert_eq!(combo_index(a, b), combo_index(b, a));
    }

    #[test]
    fn 全範圍的類別_combo_數與牌型一致() {
        let range = Range::full();
        let aces = HandClass::from_cards(
            Card::new(Rank::Ace, Suit::Spades),
            Card::new(Rank::Ace, Suit::Hearts),
        );
        assert_eq!(range.live_combos_of(aces), 6, "AA 有 6 個 combo");
        assert_eq!(range.live_combos(), COMBO_COUNT);
    }

    /// Card removal 的核心測試：不同牌型的削減比例不同。
    ///
    /// 這正是必須做到 combo 層級的理由——169 類等權處理會把這個差異抹平。
    #[test]
    fn 移除單張牌對各牌型的削減比例不同() {
        let mut range = Range::full();
        range.remove_cards(&[card("As")]);

        let aces = HandClass::from_cards(card("Ah"), card("Ad"));
        let ak_suited = HandClass::from_cards(card("Ah"), card("Kh"));
        let ak_offsuit = HandClass::from_cards(card("Ah"), card("Kd"));
        let kings = HandClass::from_cards(card("Kh"), card("Kd"));

        assert_eq!(range.live_combos_of(aces), 3, "AA 由 6 降為 3");
        assert_eq!(range.live_combos_of(ak_suited), 3, "AKs 由 4 降為 3");
        assert_eq!(range.live_combos_of(ak_offsuit), 9, "AKo 由 12 降為 9");
        assert_eq!(range.live_combos_of(kings), 6, "KK 不受影響");
    }

    #[test]
    fn 移除多張牌後總_combo_數正確() {
        let mut range = Range::full();
        let board = [card("As"), card("Kh"), card("7d")];
        range.remove_cards(&board);
        // 52-3 = 49 張可用 → C(49,2) = 1176
        assert_eq!(range.live_combos(), 1_176);
    }

    #[test]
    fn reach_weighting_依頻率縮放權重() {
        let mut range = Range::full();
        let aces = HandClass::from_cards(card("Ah"), card("Ad"));
        // AA 以 100% 加注，其餘 20%
        range.apply_reach(&|class| if class == aces { FULL } else { 2_000 });

        assert_eq!(range.weight_of(card("Ah"), card("Ad")), FULL);
        assert_eq!(range.weight_of(card("7c"), card("2d")), 2_000);
    }

    #[test]
    fn 連續兩次_reach_相乘而非取代() {
        let mut range = Range::full();
        range.apply_reach(&|_| 5_000);
        range.apply_reach(&|_| 5_000);
        assert_eq!(
            range.weight_of(card("Ah"), card("Ad")),
            2_500,
            "兩次 50% 應得 25%，代表更新是相乘而非取代"
        );
    }

    #[test]
    fn reach_不會讓已移除的_combo_復活() {
        let mut range = Range::full();
        range.remove_cards(&[card("As")]);
        range.apply_reach(&|_| FULL);
        assert_eq!(
            range.weight_of(card("As"), card("Kd")),
            0,
            "card removal 之後任何 reach 更新都不得讓該 combo 復活"
        );
    }

    #[test]
    fn 英雄底牌對對手構成_blocker() {
        let mut tracker = RangeTracker::new(3);
        // 英雄拿 AsAh，對手就不可能持有這兩張
        tracker.observe_known_cards(&[card("As"), card("Ah")]);

        let opponent = tracker.range_of(1);
        let aces = HandClass::from_cards(card("Ad"), card("Ac"));
        assert_eq!(
            opponent.live_combos_of(aces),
            1,
            "英雄持有兩張 A 時，對手的 AA 只剩 AdAc 一個 combo"
        );
    }

    #[test]
    fn 棄牌後範圍清空() {
        let mut tracker = RangeTracker::new(3);
        tracker.fold(1);
        assert_eq!(tracker.range_of(1).total_weight(), 0);
        assert!(tracker.range_of(0).total_weight() > 0, "其他座位不受影響");
    }

    #[test]
    fn 類別權重總和可供_ui_以十三乘十三呈現() {
        let range = Range::full();
        let totals = range.class_totals();
        assert_eq!(totals.len(), 169);

        let aces = HandClass::from_cards(card("Ah"), card("Ad"));
        assert_eq!(
            totals[aces.index()],
            u64::from(FULL) * 6,
            "AA 的總權重為 6 個 combo 各 100%"
        );
    }

    #[test]
    fn 由類別權重展開後類內_combo_同權() {
        let aces = HandClass::from_cards(card("Ah"), card("Ad"));
        let range = Range::from_class_weights(&|class| if class == aces { 7_000 } else { 0 });

        assert_eq!(range.weight_of(card("As"), card("Ah")), 7_000);
        assert_eq!(range.weight_of(card("Ac"), card("Ad")), 7_000);
        assert_eq!(range.weight_of(card("Ks"), card("Kh")), 0);
        assert_eq!(range.live_combos(), 6, "只有 AA 的 6 個 combo 存活");
    }
}
