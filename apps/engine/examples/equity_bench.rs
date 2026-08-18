//! Equity 時間預算量測（實做計劃 M0 閘門）。
//!
//! 批次模式每次決策的時間預算直接決定「100 萬手 ≤12 小時」是否成立。
//! 這支程式量測不同對手數與樣本預算下的單次 equity 呼叫耗時，
//! 供 M0 凍結 sampling budget 使用。
//!
//! 執行：cargo run --release --example equity_bench
//!
//! 注意：本機數值只作趨勢參考。**正式數值必須在 258V／32GB 或更低規格的
//! 筆電上量測**（核心規格 7.1），不得以開發機結果代替。

use std::time::Instant;

use poker_engine::card::Card;
use poker_engine::equity::monte_carlo_vs_random;
use poker_engine::rng::{Rng, RngDomain};

fn card(text: &str) -> Card {
    Card::parse(text).expect("牌張")
}

/// 取第 `numerator/denominator` 分位。用整數運算避免浮點轉型，
/// 量測工具本身不該引入精度問題。
fn percentile(sorted: &[u128], numerator: usize, denominator: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (sorted.len() - 1) * numerator / denominator;
    sorted[index]
}

fn main() {
    let hero = [card("As"), card("Kd")];
    println!("Equity 單次呼叫耗時（微秒）\n");
    println!(
        "{:>6} {:>8} {:>10} {:>10} {:>10}",
        "對手", "樣本", "p50", "p95", "p99"
    );
    println!("{}", "-".repeat(48));

    for opponents in [1usize, 3, 5, 8] {
        for samples in [200u64, 1_000, 5_000] {
            let mut durations = Vec::with_capacity(200);
            for round in 0..200u64 {
                let mut rng = Rng::derive(round, opponents as u64, RngDomain::Equity);
                let start = Instant::now();
                let _ = monte_carlo_vs_random(hero, opponents, &[], samples, &mut rng);
                durations.push(start.elapsed().as_micros());
            }
            durations.sort_unstable();
            println!(
                "{:>6} {:>8} {:>10} {:>10} {:>10}",
                opponents,
                samples,
                percentile(&durations, 50, 100),
                percentile(&durations, 95, 100),
                percentile(&durations, 99, 100),
            );
        }
    }

    println!("\n以 100 萬手推估（每手假設 3 次 equity 呼叫）：");
    for (opponents, samples) in [(8usize, 200u64), (8, 1_000), (5, 1_000)] {
        let mut rng = Rng::derive(1, 1, RngDomain::Equity);
        let start = Instant::now();
        let rounds = 200;
        for _ in 0..rounds {
            let _ = monte_carlo_vs_random(hero, opponents, &[], samples, &mut rng);
        }
        let per_call = start.elapsed().as_secs_f64() / f64::from(rounds);
        let total_hours = per_call * 3.0 * 1_000_000.0 / 3_600.0;
        println!(
            "  {opponents} 對手 × {samples} 樣本：每次 {:.3} ms → 100 萬手約 {total_hours:.1} 小時",
            per_call * 1_000.0
        );
    }
}
