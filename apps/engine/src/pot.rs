//! 底池分層、抽水與分配。
//!
//! 規格來源：核心規格 2.3、`德州撲克規則細則.md` 第五、六章。
//!
//! 不變量（核心規格 2.3）：
//! `Σ本手總投入 = Σ分配 + Σ退還 + rake`，誤差為 0。
//! 這條由 [`Distribution::assert_conserves`] 在每次分配後強制檢查。

use crate::chips::Chips;

/// 一層底池（main pot 或某個 side pot）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PotLayer {
    pub amount: Chips,
    /// 對本層有分配資格的座位。
    ///
    /// 規則細則 5.1：已棄牌玩家的投入留在對應層，但不具分配資格，
    /// 因此棄牌者不會出現在這裡，其籌碼仍計入 `amount`。
    pub eligible: Vec<usize>,
}

/// 一枚 odd chip 的去向，寫入事件 log（核心規格 2.3 要求逐枚記錄）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OddChipAward {
    pub layer: usize,
    pub seat: usize,
}

/// 抽水設定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RakeConfig {
    /// 萬分比（5% = 500），避免浮點進入規則層
    pub basis_points: u32,
    /// 每手抽水上限
    pub cap: Chips,
    /// 未發出 flop 即不抽水
    pub no_flop_no_drop: bool,
}

impl RakeConfig {
    pub const NONE: Self = Self {
        basis_points: 0,
        cap: Chips::ZERO,
        no_flop_no_drop: false,
    };
}

/// 分配結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distribution {
    /// 每座從底池取得的籌碼
    pub payouts: Vec<Chips>,
    /// 每座取回的未被跟注下注
    pub refunds: Vec<Chips>,
    pub rake: Chips,
    pub layers: Vec<PotLayer>,
    pub odd_chips: Vec<OddChipAward>,
}

impl Distribution {
    /// 驗證籌碼守恆：投入總額 = 分配 + 退還 + rake，誤差必須為 0。
    ///
    /// # Panics
    /// 不守恆時 panic。規則層的守恆破壞不可容忍，也不設浮點容差。
    pub fn assert_conserves(&self, contributions: &[Chips]) {
        let inflow: Chips = contributions.iter().copied().sum();
        let outflow: Chips = self.payouts.iter().copied().sum::<Chips>()
            + self.refunds.iter().copied().sum::<Chips>()
            + self.rake;
        assert_eq!(
            inflow, outflow,
            "籌碼守恆破壞：投入 {inflow} ≠ 分配+退還+rake {outflow}"
        );
    }
}

/// 退還未被跟注的下注。
///
/// 現實規則：下注中超出任何對手所能跟注的部分不構成賭注，必須退還給下注者，
/// 也不計入可抽水底池。規則細則第五章未明列此條，但「完全比照現實德州撲克」
/// 涵蓋它，且不做退還會讓籌碼守恆在「他人全下較少」的情境下失衡。
///
/// 回傳「調整後投入」與「退還額」。
#[must_use]
fn refund_uncalled(contributions: &[Chips]) -> (Vec<Chips>, Vec<Chips>) {
    let n = contributions.len();
    let mut refunds = vec![Chips::ZERO; n];
    let mut adjusted = contributions.to_vec();

    let mut sorted: Vec<Chips> = contributions.to_vec();
    sorted.sort_unstable();
    let highest = sorted[n - 1];
    let second = if n >= 2 { sorted[n - 2] } else { Chips::ZERO };

    if highest > second {
        // 最高投入者只有一人，超出第二高的部分無人跟注
        let seat = contributions
            .iter()
            .position(|&c| c == highest)
            .expect("最高投入者必然存在");
        refunds[seat] = highest - second;
        adjusted[seat] = second;
    }
    (adjusted, refunds)
}

/// 依各座實際總投入分層建立 main pot 與各 side pot。
///
/// 規則細則 5.1：每層保存參與資格；棄牌者的投入留在對應層但不具資格。
#[must_use]
pub fn build_layers(contributions: &[Chips], folded: &[bool]) -> Vec<PotLayer> {
    let mut levels: Vec<Chips> = contributions
        .iter()
        .copied()
        .filter(|c| !c.is_zero())
        .collect();
    levels.sort_unstable();
    levels.dedup();

    let mut layers = Vec::with_capacity(levels.len());
    let mut previous = Chips::ZERO;

    for level in levels {
        // 本層每人的投入 = min(該座投入, level) - min(該座投入, 前一層上界)
        let amount: Chips = contributions
            .iter()
            .map(|&c| c.min_of(level) - c.min_of(previous))
            .sum();

        let eligible: Vec<usize> = contributions
            .iter()
            .enumerate()
            .filter(|&(seat, &c)| !folded[seat] && c >= level)
            .map(|(seat, _)| seat)
            .collect();

        if !amount.is_zero() {
            layers.push(PotLayer { amount, eligible });
        }
        previous = level;
    }
    layers
}

/// 計算整手 rake。
///
/// 核心規格 2.1：以整手可抽水底池計算，先依比例向下取整至最小籌碼單位，
/// 再套用每手 cap。退還的未跟注部分不屬於底池，不計入基數。
#[must_use]
pub fn compute_rake(pot_total: Chips, config: RakeConfig, flop_dealt: bool) -> Chips {
    if config.no_flop_no_drop && !flop_dealt {
        return Chips::ZERO;
    }
    let raw = pot_total.mul_basis_points_floor(config.basis_points);
    raw.min_of(config.cap)
}

/// 從各層扣除 rake，由 main pot（最先形成的一層）起依序扣。
///
/// 核心規格 2.1：main／side pot 的分配不得重複扣除 rake，因此 rake 只在
/// 這裡從層金額扣一次，分配階段不再處理。
fn deduct_rake(layers: &mut [PotLayer], mut remaining: Chips) {
    for layer in layers.iter_mut() {
        if remaining.is_zero() {
            break;
        }
        let take = remaining.min_of(layer.amount);
        layer.amount -= take;
        remaining -= take;
    }
    debug_assert!(remaining.is_zero(), "rake 超過底池總額");
}

/// 完成一手的底池分配。
///
/// - `contributions`：每座本手總投入
/// - `folded`：每座是否已棄牌
/// - `ranks`：每座的攤牌強度，數值越大越強；`None` 表示無資格（棄牌或未攤牌）
/// - `button_seat`：按鈕**位置**索引。dead button 時仍傳該位置，
///   因為 odd chip 依按鈕位置而非持有者起算（規則細則 8.4）
#[must_use]
pub fn settle(
    contributions: &[Chips],
    folded: &[bool],
    ranks: &[Option<u32>],
    button_seat: usize,
    rake: RakeConfig,
    flop_dealt: bool,
) -> Distribution {
    let n = contributions.len();
    let (adjusted, refunds) = refund_uncalled(contributions);

    let mut layers = build_layers(&adjusted, folded);
    let pot_total: Chips = layers.iter().map(|l| l.amount).sum();
    let rake_taken = compute_rake(pot_total, rake, flop_dealt);
    deduct_rake(&mut layers, rake_taken);

    let mut payouts = vec![Chips::ZERO; n];
    let mut odd_chips = Vec::new();

    for (index, layer) in layers.iter().enumerate() {
        if layer.amount.is_zero() {
            continue;
        }
        let winners = layer_winners(layer, ranks);
        if winners.is_empty() {
            // 該層有資格者全數未攤牌：理論上不應發生，交由上層處理
            continue;
        }

        let count = winners.len() as u64;
        let share = Chips::new(layer.amount.units() / count);
        let odd = layer.amount.units() % count;

        for &seat in &winners {
            payouts[seat] += share;
        }

        // odd chip 自按鈕位置左側起、順時針分配給本層贏家。
        // 用計數而非 take(odd as usize)，避免 u64→usize 的轉型。
        let mut remaining_odd = odd;
        for seat in clockwise_from(button_seat, n) {
            if remaining_odd == 0 {
                break;
            }
            if winners.contains(&seat) {
                payouts[seat] += Chips::new(1);
                odd_chips.push(OddChipAward { layer: index, seat });
                remaining_odd -= 1;
            }
        }
        debug_assert_eq!(remaining_odd, 0, "odd chip 未分配完：贏家集合與座位序不一致");
    }

    let distribution = Distribution {
        payouts,
        refunds,
        rake: rake_taken,
        layers,
        odd_chips,
    };
    distribution.assert_conserves(contributions);
    distribution
}

/// 本層贏家：有資格者之中攤牌強度最高者，可能並列。
fn layer_winners(layer: &PotLayer, ranks: &[Option<u32>]) -> Vec<usize> {
    let best = layer
        .eligible
        .iter()
        .filter_map(|&s| ranks[s])
        .max();
    let Some(best) = best else {
        return Vec::new();
    };
    layer
        .eligible
        .iter()
        .copied()
        .filter(|&s| ranks[s] == Some(best))
        .collect()
}

/// 自 `from` 左側第一位起的順時針座位序。
fn clockwise_from(from: usize, n: usize) -> impl Iterator<Item = usize> {
    (1..=n).map(move |offset| (from + offset) % n)
}
