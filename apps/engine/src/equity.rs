//! Equity 計算：精確枚舉與可重現的 Monte Carlo。
//!
//! 規格來源：核心規格 5.1。
//!
//! 兩條鐵則寫進了型別：
//! - 「取樣模式必須顯示 Monte Carlo 樣本數與計算誤差；**不得把取樣值標示為
//!   『精算』**。」→ [`EquityMode`] 隨結果一起回傳，呼叫端無法遺失這個脈絡。
//! - 「多人平手按實際並列人數分配。」→ 平手以 `1/k` 計入，不併入勝場。
//!
//! Equity 使用專屬的 RNG domain（核心規格 3.4），與發牌、策略取樣互相獨立。

use crate::card::{full_deck, Card};
use crate::eval::evaluate;
use crate::rng::Rng;

/// 計算模式。必須隨結果傳遞，讓報表能誠實標示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquityMode {
    /// 枚舉全部剩餘 runout。可標示為「模型內精確」
    Exact { runouts: u64 },
    /// 分層 Monte Carlo。必須顯示樣本數與誤差
    Sampled { samples: u64 },
}

/// Equity 結果。
///
/// 份額以**萬分比**累計而非浮點：多人平手要按 `1/k` 分配，用整數萬分比
/// 累加可讓「總份額 = 樣本數 × 10000」成為可斷言的不變量，浮點則只能靠容差。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Equity {
    /// 累計份額（萬分比 × 樣本數）
    share_myriad: u64,
    pub samples: u64,
    pub mode: EquityMode,
}

impl Equity {
    /// 勝率，以萬分比表示（10000 = 100%）。
    #[must_use]
    pub const fn as_myriad(&self) -> u64 {
        if self.samples == 0 {
            return 0;
        }
        self.share_myriad / self.samples
    }

    /// 勝率百分比。**只供顯示**，規則與策略索引一律用 [`Self::as_myriad`]。
    ///
    /// 核心規格 2.3：「浮點只出現在統計輸出」。本函式即該邊界，
    /// 因此局部豁免 workspace 的浮點轉型 lint。份額與樣本數都遠小於
    /// f64 尾數的 2^53，實際不會失去精度。
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn as_percent(&self) -> f64 {
        if self.samples == 0 {
            return 0.0;
        }
        // 統計輸出允許浮點（核心規格 2.3）
        self.share_myriad as f64 / self.samples as f64 / 100.0
    }

    /// Monte Carlo 的 95% 誤差上限（百分點）。
    ///
    /// 精確枚舉沒有取樣誤差，回傳 `None`——核心規格 5.3 明訂
    /// 「精確枚舉 Equity 標示『模型內精確』，**不顯示虛構 sampling CI**」。
    ///
    /// 同 [`Self::as_percent`]，這是統計輸出邊界，局部豁免浮點轉型 lint。
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn margin_of_error(&self) -> Option<f64> {
        match self.mode {
            EquityMode::Exact { .. } => None,
            EquityMode::Sampled { samples } if samples > 0 => {
                let p = self.share_myriad as f64 / samples as f64 / 10_000.0;
                let se = (p * (1.0 - p) / samples as f64).sqrt();
                Some(1.96 * se * 100.0)
            }
            EquityMode::Sampled { .. } => None,
        }
    }

    /// 是否可標示為「模型內精確」。
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self.mode, EquityMode::Exact { .. })
    }
}

/// 對決一次 showdown，把份額計入英雄。
///
/// 多人平手按實際並列人數分配 `1/k`（核心規格 5.1）。
fn score_showdown(hero: u32, opponents: &[u32]) -> u64 {
    let best = opponents.iter().copied().max().unwrap_or(0);
    if hero > best {
        10_000
    } else if hero < best {
        0
    } else {
        // 並列人數 = 英雄 + 與英雄同分的對手
        let ties = 1 + opponents.iter().filter(|&&r| r == hero).count();
        10_000 / u64::try_from(ties).expect("並列人數必在 u64 範圍")
    }
}

/// 精確計算：對手底牌已知，枚舉全部剩餘公共牌。
///
/// 用於 all-in EV 與小狀態空間節點（核心規格 5.1）。
///
/// # Panics
/// 牌張重複或公共牌超過 5 張時 panic。
#[must_use]
pub fn exact_vs_known(
    hero: [Card; 2],
    opponents: &[[Card; 2]],
    board: &[Card],
) -> Equity {
    assert!(board.len() <= 5, "公共牌不得超過 5 張");

    let mut used: Vec<Card> = Vec::with_capacity(2 + opponents.len() * 2 + board.len());
    used.extend_from_slice(&hero);
    for hand in opponents {
        used.extend_from_slice(hand);
    }
    used.extend_from_slice(board);
    {
        let mut check = used.clone();
        check.sort_unstable();
        let before = check.len();
        check.dedup();
        assert_eq!(before, check.len(), "牌張重複");
    }

    let remaining: Vec<Card> = full_deck()
        .into_iter()
        .filter(|c| !used.contains(c))
        .collect();
    let need = 5 - board.len();

    let mut share = 0u64;
    let mut runouts = 0u64;
    let mut chosen: Vec<Card> = Vec::with_capacity(need);
    enumerate_runouts(
        &remaining,
        need,
        0,
        &mut chosen,
        &mut |extra: &[Card]| {
            share += score_one(hero, opponents, board, extra);
            runouts += 1;
        },
    );

    Equity {
        share_myriad: share,
        samples: runouts,
        mode: EquityMode::Exact { runouts },
    }
}

fn score_one(hero: [Card; 2], opponents: &[[Card; 2]], board: &[Card], extra: &[Card]) -> u64 {
    let mut full_board: Vec<Card> = Vec::with_capacity(5);
    full_board.extend_from_slice(board);
    full_board.extend_from_slice(extra);

    let mut cards = full_board.clone();
    cards.extend_from_slice(&hero);
    let hero_rank = evaluate(&cards).value();

    let opponent_ranks: Vec<u32> = opponents
        .iter()
        .map(|hand| {
            let mut cards = full_board.clone();
            cards.extend_from_slice(hand);
            evaluate(&cards).value()
        })
        .collect();

    score_showdown(hero_rank, &opponent_ranks)
}

/// Monte Carlo：對手底牌未知，由剩餘牌堆隨機抽出。
///
/// 用於翻前與大型多人節點。結果的 [`EquityMode::Sampled`] 會帶出樣本數，
/// 報表據此顯示誤差，不得標示為精算。
///
/// # Panics
/// 剩餘牌不足以發完對手底牌與公共牌時 panic。
#[must_use]
pub fn monte_carlo_vs_random(
    hero: [Card; 2],
    opponents: usize,
    board: &[Card],
    samples: u64,
    rng: &mut Rng,
) -> Equity {
    let mut used: Vec<Card> = Vec::with_capacity(2 + board.len());
    used.extend_from_slice(&hero);
    used.extend_from_slice(board);

    let deck: Vec<Card> = full_deck()
        .into_iter()
        .filter(|c| !used.contains(c))
        .collect();
    let need_board = 5 - board.len();
    let need_total = opponents * 2 + need_board;
    assert!(deck.len() >= need_total, "剩餘牌不足以完成取樣");

    let mut share = 0u64;
    let mut pool = deck;

    for _ in 0..samples {
        // 部分洗牌：只需前 need_total 張，不必洗完整副
        for i in 0..need_total {
            let bound = u64::try_from(pool.len() - i).expect("長度必在 u64 範圍");
            let offset = usize::try_from(rng.below(bound)).expect("索引必在 usize 範圍");
            pool.swap(i, i + offset);
        }

        let opponent_hands: Vec<[Card; 2]> = (0..opponents)
            .map(|k| [pool[k * 2], pool[k * 2 + 1]])
            .collect();
        let extra: Vec<Card> = pool[opponents * 2..opponents * 2 + need_board].to_vec();

        share += score_one(hero, &opponent_hands, board, &extra);
    }

    Equity {
        share_myriad: share,
        samples,
        mode: EquityMode::Sampled { samples },
    }
}

/// 枚舉 `remaining` 中所有 `need` 張的組合，逐一交給 `visit`。
///
/// 遞迴實作而非手寫索引推進：組合枚舉的索引邏輯容易寫錯，
/// 而 evaluator 的正確性完全依賴它枚舉完整。
fn enumerate_runouts(
    remaining: &[Card],
    need: usize,
    start: usize,
    chosen: &mut Vec<Card>,
    visit: &mut impl FnMut(&[Card]),
) {
    if chosen.len() == need {
        visit(chosen);
        return;
    }
    // 剩餘張數不足以湊滿時提早剪枝
    let still_needed = need - chosen.len();
    if remaining.len() < start + still_needed {
        return;
    }
    for index in start..remaining.len() {
        chosen.push(remaining[index]);
        enumerate_runouts(remaining, need, index + 1, chosen, visit);
        chosen.pop();
    }
}
