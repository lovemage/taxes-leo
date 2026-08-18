//! 展開完整 preflop baseline 並檢視結果（M0 閘門 2 的產製管線驗證）。
//!
//! 執行：cargo run --release --example generate_baseline

use std::collections::BTreeMap;
use std::time::Instant;

use poker_engine::betting::Action;
use poker_engine::position::PositionLabel;
use poker_engine::strategy::baseline::{distribution_for, BaselineRules};
use poker_engine::strategy::decision::StackBucket;
use poker_engine::strategy::distribution::FULL;
use poker_engine::strategy::hand_class::HandClass;
use poker_engine::strategy::preflop::{enumerate_nodes, PreflopNode, PreflopScenario};
use poker_engine::strategy::ranking::EquityRanking;

fn main() {
    let rules = BaselineRules::engineering_placeholder();
    println!("規則集：{}（{}）", rules.name, rules.version);
    println!(
        "顧問簽核：{}\n",
        if rules.consultant_approved {
            "已簽核"
        } else {
            "**未簽核，不得作為出貨 baseline**"
        }
    );

    // 依對手數預先算好排序表（節點的對手數以桌型近似）
    println!("計算 equity 排序表…");
    let start = Instant::now();
    let rankings: BTreeMap<usize, EquityRanking> = [1usize, 2, 3, 5, 8]
        .into_iter()
        .map(|opponents| (opponents, EquityRanking::compute(opponents, 20_000)))
        .collect();
    println!("  完成，耗時 {:.1} 秒\n", start.elapsed().as_secs_f64());

    // 展開全表
    println!("展開全表…");
    let nodes = enumerate_nodes();
    let start = Instant::now();
    let mut cells = 0usize;
    let mut failures = 0usize;
    for node in &nodes {
        let ranking = pick_ranking(&rankings, node);
        for class in HandClass::all() {
            match distribution_for(node, class, &rules, ranking) {
                Ok(d) => {
                    debug_assert_eq!(
                        d.entries().iter().map(|(_, w)| *w).sum::<u32>(),
                        FULL
                    );
                    cells += 1;
                }
                Err(_) => failures += 1,
            }
        }
    }
    let elapsed = start.elapsed();
    println!(
        "  節點 {} 個、格數 {}，耗時 {:.2} 秒（{:.0} 格／秒）",
        nodes.len(),
        cells,
        elapsed.as_secs_f64(),
        ratio(u64::try_from(cells).unwrap_or(0), elapsed.as_secs_f64())
    );
    println!("  正規化失敗：{failures} 格\n");

    // 抽樣檢視：9-max、200BB、unopened 各位置的開牌範圍
    println!("抽樣：9-max／[160,240) bucket／unopened 的開牌範圍寬度");
    println!("{:>6} {:>12} {:>10}", "位置", "開牌頻率合計", "約略寬度");
    println!("{}", "-".repeat(32));
    let ranking = &rankings[&5];
    for hero in [
        PositionLabel::Utg,
        PositionLabel::Lj,
        PositionLabel::Hj,
        PositionLabel::Co,
        PositionLabel::Btn,
        PositionLabel::Sb,
    ] {
        let node = PreflopNode {
            seated: 9,
            hero,
            bucket: StackBucket::VeryDeep,
            scenario: PreflopScenario::Unopened,
        };
        let mut total_raise = 0u64;
        for class in HandClass::all() {
            let d = distribution_for(&node, class, &rules, ranking).expect("產生");
            let raise: u32 = d
                .entries()
                .iter()
                .filter(|(a, _)| matches!(a, Action::RaiseTo(_) | Action::AllIn))
                .map(|(_, w)| *w)
                .sum();
            total_raise += u64::from(raise);
        }
        // 169 類的平均加注頻率即範圍寬度
        let width = ratio(total_raise, 169.0) / 100.0;
        println!("{:>6} {:>12} {:>9.1}%", hero.as_str(), total_raise, width);
    }

    // 列出 BTN 開牌範圍的邊界牌
    println!("\nBTN 開牌範圍的邊界（混合帶）：");
    let node = PreflopNode {
        seated: 9,
        hero: PositionLabel::Btn,
        bucket: StackBucket::VeryDeep,
        scenario: PreflopScenario::Unopened,
    };
    let mut mixed: Vec<(String, u32)> = Vec::new();
    for class in HandClass::all() {
        let d = distribution_for(&node, class, &rules, ranking).expect("產生");
        let raise: u32 = d
            .entries()
            .iter()
            .filter(|(a, _)| matches!(a, Action::RaiseTo(_) | Action::AllIn))
            .map(|(_, w)| *w)
            .sum();
        if raise > 0 && raise < FULL {
            mixed.push((class.label(), raise));
        }
    }
    mixed.sort_by_key(|(_, w)| std::cmp::Reverse(*w));
    for (label, weight) in mixed.iter().take(12) {
        println!("  {label:<4} 加注 {:.0}%", f64::from(*weight) / 100.0);
    }
    println!("  （共 {} 個混合格）", mixed.len());
}

/// 顯示用的除法。本檔是報表工具，浮點僅供輸出（核心規格 2.3）。
#[allow(clippy::cast_precision_loss)]
fn ratio(numerator: u64, denominator: f64) -> f64 {
    numerator as f64 / denominator
}

fn pick_ranking<'a>(
    rankings: &'a BTreeMap<usize, EquityRanking>,
    node: &PreflopNode,
) -> &'a EquityRanking {
    // 以情境近似「預期對抗人數」：面對加注多為單挑或三人，開牌則面對全桌
    let opponents = match node.scenario {
        PreflopScenario::Unopened => usize::from(node.seated).saturating_sub(1).min(8),
        PreflopScenario::VsLimp { limpers } => usize::from(limpers).max(1) + 1,
        PreflopScenario::VsOpen { .. } => 2,
        PreflopScenario::VsThreeBet { .. }
        | PreflopScenario::VsFourBet { .. }
        | PreflopScenario::VsSqueeze { .. } => 1,
    };
    rankings
        .range(..=opponents)
        .next_back()
        .map_or_else(|| &rankings[&1], |(_, r)| r)
}
