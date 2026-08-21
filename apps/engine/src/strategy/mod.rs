//! 策略層（M2）。
//!
//! 核心規格 2.4 的介面約定：
//! `StrategyProvider` 的唯一輸入是 `DecisionView`、自身 range 與由公開資訊
//! 推導的對手 range estimates，**不得接收完整 `GameState`**。
//!
//! 型別層的落實方式見 [`decision::DecisionView`]：它沒有任何欄位能承載
//! 他人底牌或牌堆順序，因此隱藏資訊不是「策略層自律不讀」，而是讀不到。

pub mod decision;
pub mod distribution;
pub mod hand_class;
pub mod opening;
pub mod playability;
pub mod preflop;
pub mod vs_open;
pub mod baseline;
pub mod calibration;
pub mod cell_override;
pub mod postflop;
pub mod range;
pub mod ranking;

pub use decision::{DecisionView, OpponentPublic, PublicAction, StackBucket};
pub use distribution::{ActionDistribution, DistributionError, Myriad, FULL};
pub use hand_class::HandClass;
pub use opening::OpeningWidths;
pub use playability::{PlayabilityAdjustments, PlayabilityCategory};
pub use preflop::{PreflopNode, PreflopScenario};
pub use vs_open::VsOpenWidths;
pub use baseline::{BaselineRules, ScenarioWidths};
pub use calibration::{Attribution, MatrixCell, ParameterRef, RangeMatrix, Verdict};
pub use cell_override::{CellOverrides, OverrideCell};
pub use range::{Range, RangeTracker, COMBO_COUNT};
pub use ranking::{ClassEquity, EquityRanking};

/// 自身 range：169 類各自的到達權重（萬分比）。
///
/// 翻前為策略表直接給定；翻後由 `RangeTracker` 依公開行動推導。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnRange {
    weights: Vec<Myriad>,
}

impl OwnRange {
    /// 全部 169 類等權重。
    #[must_use]
    pub fn uniform() -> Self {
        Self {
            weights: vec![FULL; 169],
        }
    }

    #[must_use]
    pub fn weight(&self, class: HandClass) -> Myriad {
        self.weights.get(class.index()).copied().unwrap_or(0)
    }

    pub fn set(&mut self, class: HandClass, weight: Myriad) {
        if self.weights.len() < 169 {
            self.weights.resize(169, 0);
        }
        self.weights[class.index()] = weight;
    }
}

/// 對手 range 估計。
///
/// 核心規格 2.4：必須「由公開資訊推導」。這個型別只保存推導結果，
/// 不持有任何實際底牌——推導過程若需要真實牌，就是資訊隔離已被破壞。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpponentRangeEstimates {
    per_seat: Vec<(usize, OwnRange)>,
}

impl OpponentRangeEstimates {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn set(&mut self, seat: usize, range: OwnRange) {
        match self.per_seat.iter_mut().find(|(s, _)| *s == seat) {
            Some(entry) => entry.1 = range,
            None => self.per_seat.push((seat, range)),
        }
    }

    #[must_use]
    pub fn get(&self, seat: usize) -> Option<&OwnRange> {
        self.per_seat
            .iter()
            .find(|(s, _)| *s == seat)
            .map(|(_, range)| range)
    }
}

/// 策略提供者。
///
/// 回傳**分佈**而非單一行動：取樣由引擎以該決策專屬的 RNG stream 執行
/// （核心規格 4.3 第 7 步），策略層不自行取樣，否則可重現性會被策略實作
/// 的內部亂數破壞。
pub trait StrategyProvider {
    /// 依規格 2.4 的簽章產生行動分佈。
    ///
    /// 回傳的分佈**不必**已套用 legal mask；引擎會在第 5 步統一套用
    /// （核心規格 4.3）。
    fn act(
        &self,
        view: &DecisionView,
        own_range: &OwnRange,
        opponents: &OpponentRangeEstimates,
    ) -> ActionDistribution;
}
