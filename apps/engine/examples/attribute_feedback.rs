//! 顧問意見的參數歸因示範。
//!
//! 顧問說「某格不對」之後，本工具算出：要滿足這個意見，哪個參數必須改到
//! 多少，以及**會連帶影響哪些手牌**。連帶影響若不可接受，代表模型缺參數。
//!
//! 執行：cargo run --release --example attribute_feedback

use poker_engine::card::Rank;
use poker_engine::position::PositionLabel;
use poker_engine::strategy::baseline::{expected_opponents, BaselineRules};
use poker_engine::strategy::calibration::{attribute, RangeMatrix, Verdict};
use poker_engine::strategy::decision::StackBucket;
use poker_engine::strategy::distribution::FULL;
use poker_engine::strategy::preflop::{PreflopNode, PreflopScenario};
use poker_engine::strategy::ranking::{class_of, EquityRanking};

fn main() {
    let rules = BaselineRules::engineering_placeholder();

    let node = PreflopNode {
        seated: 9,
        hero: PositionLabel::Btn,
        bucket: StackBucket::VeryDeep,
        scenario: PreflopScenario::Unopened,
    };
    let ranking = EquityRanking::compute(expected_opponents(&node), 20_000);
    let matrix = RangeMatrix::build(node, &rules, &ranking);

    // 模擬三則典型的顧問意見
    let feedback = [
        (
            class_of(Rank::Five, Rank::Five, false),
            Verdict::ShouldBeAggressive,
            "55 從 BTN 應該是明確開牌",
        ),
        (
            class_of(Rank::King, Rank::Two, true),
            Verdict::ShouldNotBeAggressive,
            "K2s 從 BTN 開牌太寬",
        ),
        (
            class_of(Rank::Queen, Rank::Nine, false),
            Verdict::ShouldBeAggressive,
            "Q9o 從 BTN 應該開牌",
        ),
    ];

    println!("節點：{}\n", node.key());

    for (class, verdict, comment) in feedback {
        let cell = matrix.cell(class);
        println!("{}", "=".repeat(66));
        println!("顧問意見：{comment}");
        println!(
            "  目前：{} 主動 {:.0}%（equity 排序前 {:.1}%）",
            class.label(),
            f64::from(cell.aggressive) / 100.0,
            f64::from(cell.percentile) / 100.0
        );

        let attributions = attribute(node, class, verdict, &rules, &ranking);
        if attributions.is_empty() {
            println!("  → 已符合，無需調整\n");
            continue;
        }

        for a in &attributions {
            let unit = if a.parameter.as_str() == "bucket_multiplier" {
                "×"
            } else {
                "%"
            };
            let scale = if unit == "×" { 10_000.0 } else { 100.0 };
            println!(
                "\n  途徑：{} 由 {:.2}{unit} 改為 {:.2}{unit}",
                a.parameter.as_str(),
                f64::from(a.current) / scale,
                f64::from(a.required) / scale
            );
            println!("    連帶影響 {} 格", a.collateral_count());
            if !a.pulled_in.is_empty() {
                println!(
                    "      一併納入：{}",
                    a.pulled_in
                        .iter()
                        .map(|c| c.label())
                        .collect::<Vec<_>>()
                        .join("、")
                );
            }
            if !a.pushed_out.is_empty() {
                println!(
                    "      一併排除：{}",
                    a.pushed_out
                        .iter()
                        .map(|c| c.label())
                        .collect::<Vec<_>>()
                        .join("、")
                );
            }
        }

        // 若連帶影響過大，明確提示模型可能缺參數
        if let Some(best) = attributions.iter().min_by_key(|a| a.collateral_count()) {
            if best.collateral_count() > 8 {
                println!(
                    "\n  ⚠ 最小連帶影響仍達 {} 格。若顧問不接受這些連帶改動，",
                    best.collateral_count()
                );
                println!("    代表 equity 排序無法表達他要的區分，模型需新增具名參數");
                println!("    （例如對子加成、同花連牌加成），而非繼續調寬度。");
            }
        }
        println!();
    }

    println!("{}", "=".repeat(66));
    println!("目前 BTN 開牌寬度：{:.1}%", f64::from(matrix.width_myriad()) / 100.0);
    println!("混合格 {} 個", matrix.mixed_cells().len());
    let _ = FULL;
}
