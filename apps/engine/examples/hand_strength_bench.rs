//! 牌力分類的延遲量測（0907 計劃 Stage 1 的最後一道關）。
//!
//! 計劃 §3.3：「列舉計算先做獨立 benchmark，再接進每次決策；驗證既有
//! 延遲預算（p50 ≤ 1 ms、p99 ≤ 5 ms）。」**不符合預算就不得接入產品
//! 決策路徑**——分類器要跑在每一個翻後節點上，慢一點就是整個 run 慢。
//!
//! 執行：cargo run --release --example hand_strength_bench

use std::time::Instant;

use poker_engine::card::{Card, Rank, Suit};
use poker_engine::rng::{Rng, RngDomain};
use poker_engine::strategy::hand_strength::{classify, HandStrengthConfig};

const SAMPLES: usize = 2_000;

fn main() {
    let config = HandStrengthConfig::ENGINEERING;

    for (label, board_len) in [("翻牌", 3usize), ("轉牌", 4), ("河牌", 5)] {
        let mut rng = Rng::derive(20_260_909, board_len as u64, RngDomain::Stats);
        let mut timings = Vec::with_capacity(SAMPLES);

        for _ in 0..SAMPLES {
            let (hole, board) = deal(&mut rng, board_len);
            let start = Instant::now();
            let snapshot = classify(hole, &board, &config);
            timings.push(u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX));
            // 讓最佳化器不能把整段算掉
            std::hint::black_box(snapshot.group);
        }

        timings.sort_unstable();
        let p50 = timings[timings.len() / 2];
        let p99 = timings[timings.len() * 99 / 100];
        let max = timings[timings.len() - 1];

        let verdict = if p50 <= 1_000 && p99 <= 5_000 {
            "符合預算"
        } else {
            "**超出預算，不得接入產品決策**"
        };
        println!(
            "{label}：p50 {} µs／p99 {} µs／max {} µs（{} 次）　{verdict}",
            p50, p99, max, SAMPLES
        );
    }

    println!("\n預算：p50 ≤ 1 ms、p99 ≤ 5 ms（核心規格 7.3 的翻後決策延遲）");
}

/// 發一手不重複的底牌與公共牌。
fn deal(rng: &mut Rng, board_len: usize) -> ([Card; 2], Vec<Card>) {
    let mut deck: Vec<Card> = Rank::ALL
        .into_iter()
        .flat_map(|rank| Suit::ALL.into_iter().map(move |suit| Card::new(rank, suit)))
        .collect();

    // Fisher-Yates 取前幾張即可
    for i in 0..(2 + board_len) {
        let j = i + usize::try_from(rng.below((deck.len() - i) as u64)).unwrap_or(0);
        deck.swap(i, j);
    }

    let hole = [deck[0], deck[1]];
    let board = deck[2..2 + board_len].to_vec();
    (hole, board)
}
