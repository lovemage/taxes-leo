//! 169 類起手牌的 equity 排序表。
//!
//! 這是參數化策略產生器的基礎：策略規則以「取 equity 排序的前 X%」表達，
//! 而不是逐格指定頻率。核心規格 4.1 要求的 727,038 格因此由**數十個百分比**
//! 展開而成，顧問審的是規則與抽樣，不是逐格。
//!
//! **排序是機械推導的**：對 N 個隨機對手跑 Monte Carlo，用固定 seed，
//! 結果可重現、可稽核。它是「equity 排序」，**不是 GTO 解**，
//! 不得對外宣稱為均衡策略（實做計劃強制規則 2）。

use crate::card::{Card, Rank, Suit};
use crate::equity::monte_carlo_vs_random;
use crate::rng::{Rng, RngDomain};
use crate::strategy::hand_class::HandClass;

/// 建立排序表時使用的固定 seed。改動它會改變整份 baseline，
/// 因此視為內容版本的一部分。
pub const RANKING_SEED: u64 = 0x9E37_79B9;

/// 一個手牌類別的 equity 量測結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassEquity {
    pub class: HandClass,
    /// 對 N 個隨機對手的勝率（萬分比）
    pub equity_myriad: u64,
    /// 在同一對手數下的排名，0 為最強
    pub rank: usize,
}

/// 某一對手數下的完整排序表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquityRanking {
    pub opponents: usize,
    pub samples: u64,
    /// 依索引存放（`HandClass::index()`）
    by_class: Vec<ClassEquity>,
    /// 依強度由高到低的類別索引
    order: Vec<usize>,
}

impl EquityRanking {
    /// 量測全部 169 類對 `opponents` 個隨機對手的 equity 並排序。
    ///
    /// 每個類別取該類的一個代表 combo。同一類別內不同 combo 的 equity
    /// 差異極小（同花與否已由類別區分），對排序不構成影響。
    #[must_use]
    pub fn compute(opponents: usize, samples: u64) -> Self {
        let mut measured: Vec<(HandClass, u64)> = HandClass::all()
            .into_iter()
            .enumerate()
            .map(|(index, class)| {
                // 每個類別用獨立的 stream，量測順序因此不影響結果
                let mut rng = Rng::derive(
                    RANKING_SEED,
                    (opponents as u64) << 32 | index as u64,
                    RngDomain::Equity,
                );
                let hole = representative_combo(class);
                let equity = monte_carlo_vs_random(hole, opponents, &[], samples, &mut rng);
                (class, equity.as_myriad())
            })
            .collect();

        // 由強到弱；equity 相同時以類別索引排序，確保結果確定
        measured.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.index().cmp(&b.0.index())));

        let mut by_class = vec![
            ClassEquity {
                class: HandClass::all()[0],
                equity_myriad: 0,
                rank: 0,
            };
            169
        ];
        let mut order = Vec::with_capacity(169);
        for (rank, &(class, equity_myriad)) in measured.iter().enumerate() {
            by_class[class.index()] = ClassEquity {
                class,
                equity_myriad,
                rank,
            };
            order.push(class.index());
        }

        Self {
            opponents,
            samples,
            by_class,
            order,
        }
    }

    #[must_use]
    pub fn of(&self, class: HandClass) -> ClassEquity {
        self.by_class[class.index()]
    }

    /// 由強到弱的類別序列。
    #[must_use]
    pub fn strongest_first(&self) -> Vec<HandClass> {
        self.order.iter().map(|&i| self.by_class[i].class).collect()
    }

    /// 該類別落在前百分之幾（萬分比）。0 為最強。
    #[must_use]
    pub fn percentile_myriad(&self, class: HandClass) -> u64 {
        let rank = self.of(class).rank as u64;
        rank * 10_000 / 169
    }
}

/// 取類別的代表 combo。
///
/// 對子取兩個不同花色；同花取相同花色；非同花取不同花色。
#[must_use]
pub fn representative_combo(class: HandClass) -> [Card; 2] {
    let high = class.high();
    let low = class.low();
    if class.is_pair() {
        [Card::new(high, Suit::Spades), Card::new(low, Suit::Hearts)]
    } else if class.is_suited() {
        [Card::new(high, Suit::Spades), Card::new(low, Suit::Spades)]
    } else {
        [Card::new(high, Suit::Spades), Card::new(low, Suit::Hearts)]
    }
}

/// 便利函式：確認某牌面組合可解析（供測試與工具使用）。
#[must_use]
pub fn class_of(high: Rank, low: Rank, suited: bool) -> HandClass {
    let a = Card::new(high, Suit::Spades);
    let b = Card::new(low, if suited { Suit::Spades } else { Suit::Hearts });
    HandClass::from_cards(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 代表_combo_的類別歸屬正確() {
        for class in HandClass::all() {
            let combo = representative_combo(class);
            assert_eq!(
                HandClass::from_cards(combo[0], combo[1]),
                class,
                "{} 的代表 combo 歸類錯誤",
                class.label()
            );
        }
    }

    #[test]
    fn 排序表涵蓋全部_169_類且排名互異() {
        // 樣本數壓低以加速單元測試；正式表由工具以高樣本產生
        let ranking = EquityRanking::compute(1, 400);
        let ordered = ranking.strongest_first();
        assert_eq!(ordered.len(), 169);

        let mut ranks: Vec<usize> = HandClass::all()
            .into_iter()
            .map(|c| ranking.of(c).rank)
            .collect();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), 169, "每個類別必須有唯一排名");
    }

    #[test]
    fn 排序結果可重現() {
        let a = EquityRanking::compute(2, 300);
        let b = EquityRanking::compute(2, 300);
        assert_eq!(a, b, "固定 seed 的排序表必須完全一致");
    }
}
