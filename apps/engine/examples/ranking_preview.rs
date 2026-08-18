//! 檢視 equity 排序表（供人工合理性檢查）。
//!
//! 排序表是參數化策略的基礎，若排序本身不合理，後面全錯。
//! 這支工具把前後段列出來，讓熟悉撲克的人一眼判斷是否可信。
//!
//! 執行：cargo run --release --example ranking_preview

use poker_engine::strategy::ranking::EquityRanking;

fn main() {
    for opponents in [1usize, 5, 8] {
        let ranking = EquityRanking::compute(opponents, 20_000);
        let ordered = ranking.strongest_first();

        println!("\n對 {opponents} 個隨機對手（每類 20,000 樣本）");
        println!("{}", "=".repeat(52));

        println!("最強 15 類：");
        for (i, class) in ordered.iter().take(15).enumerate() {
            let equity = ranking.of(*class);
            println!(
                "  {:>2}. {:<4} {:>6.2}%",
                i + 1,
                class.label(),
                f64::from(u32::try_from(equity.equity_myriad).unwrap_or(0)) / 100.0
            );
        }

        println!("最弱 5 類：");
        for (i, class) in ordered.iter().rev().take(5).rev().enumerate() {
            let equity = ranking.of(*class);
            println!(
                "  {:>3}. {:<4} {:>6.2}%",
                165 + i,
                class.label(),
                f64::from(u32::try_from(equity.equity_myriad).unwrap_or(0)) / 100.0
            );
        }

        // 幾個基準牌的位置，供對照直覺
        for label in ["AKs", "AKo", "TT", "77", "A5s", "KJo", "T9s", "72o"] {
            if let Some(class) = ordered.iter().find(|c| c.label() == label) {
                let e = ranking.of(*class);
                println!(
                    "  · {label:<4} 排名 {:>3}／169，前 {:>4.1}%",
                    e.rank + 1,
                    f64::from(u32::try_from(ranking.percentile_myriad(*class)).unwrap_or(0)) / 100.0
                );
            }
        }
    }
}
