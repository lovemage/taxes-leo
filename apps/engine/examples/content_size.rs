//! Preflop baseline 的內容體積精算（實做計劃 M0 閘門 2）。
//!
//! 閘門 2 要求交出「格數清單（桌型 × 位置 × bucket × 節點情境 × 169）」。
//! 先前文件裡的 32 萬是估算值，這支程式給出精確數字，並依維度拆解，
//! 讓「哪個維度該砍」這件事有依據可談。
//!
//! 執行：cargo run --release --example content_size

use std::collections::BTreeMap;

use poker_engine::strategy::preflop::{all_buckets, enumerate_nodes, positions_for};

const HAND_CLASSES: usize = 169;

fn main() {
    let nodes = enumerate_nodes();
    let total_cells = nodes.len() * HAND_CLASSES;

    println!("Preflop baseline 內容體積精算\n");
    println!("節點總數：{}", format_thousands(nodes.len()));
    println!("每節點手牌類別：{HAND_CLASSES}");
    println!("**總格數：{}**\n", format_thousands(total_cells));

    // 依桌型拆解
    println!("依桌型：");
    println!("{:>6} {:>10} {:>14}", "桌型", "節點數", "格數");
    println!("{}", "-".repeat(34));
    for seated in 6u8..=9 {
        let count = nodes.iter().filter(|n| n.seated == seated).count();
        println!(
            "{:>4}max {:>10} {:>14}",
            seated,
            format_thousands(count),
            format_thousands(count * HAND_CLASSES)
        );
    }

    // 依情境類型拆解
    println!("\n依情境類型：");
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for node in &nodes {
        let kind = node.scenario.key();
        let bucket_key = kind.split('-').take(2).collect::<Vec<_>>().join("-");
        let label: &str = match bucket_key.as_str() {
            "unopened" => "unopened",
            "vs-limp" => "vs limp",
            "vs-open" => "vs open",
            "vs-3bet" => "vs 3-bet",
            "vs-4bet" => "vs 4-bet",
            "vs-squeeze" => "vs squeeze",
            _ => "其他",
        };
        *by_kind.entry(label).or_default() += 1;
    }
    println!("{:>12} {:>10} {:>14} {:>8}", "情境", "節點數", "格數", "占比");
    println!("{}", "-".repeat(48));
    for (label, count) in &by_kind {
        println!(
            "{:>12} {:>10} {:>14} {:>7.1}%",
            label,
            format_thousands(*count),
            format_thousands(count * HAND_CLASSES),
            percent(*count, nodes.len())
        );
    }

    // 維度貢獻
    println!("\n維度：");
    let position_slots: usize = (6u8..=9).map(|s| positions_for(s).len()).sum();
    println!("  桌型 6–9              4 種");
    println!("  位置槽（4 種桌型合計）{position_slots} 個");
    println!("  有效籌碼 bucket        {} 檔", all_buckets().len());
    println!("  手牌類別               {HAND_CLASSES} 類");

    // 若砍維度可省多少
    println!("\n若要縮減，各方案的效果：");
    let nine_max_only = nodes.iter().filter(|n| n.seated == 9).count();
    println!(
        "  只做 9-max（其餘桌型後補）：{} 格，省 {:.0}%",
        format_thousands(nine_max_only * HAND_CLASSES),
        100.0 - percent(nine_max_only, nodes.len())
    );

    let core_buckets = 4;
    println!(
        "  bucket 由 {} 檔減為 {core_buckets} 檔：{} 格，省 {:.0}%",
        all_buckets().len(),
        format_thousands(total_cells / all_buckets().len() * core_buckets),
        100.0 - percent(core_buckets, all_buckets().len())
    );

    let no_squeeze_4bet = nodes
        .iter()
        .filter(|n| {
            let k = n.scenario.key();
            !k.starts_with("vs-squeeze") && !k.starts_with("vs-4bet")
        })
        .count();
    println!(
        "  先不做 4-bet 與 squeeze：{} 格，省 {:.0}%",
        format_thousands(no_squeeze_4bet * HAND_CLASSES),
        100.0 - percent(no_squeeze_4bet, nodes.len())
    );

    // 產製成本推估
    println!("\n產製方式的量級對照：");
    println!(
        "  人工逐格填寫（樂觀 3 秒／格）：{:.0} 人日（8 小時計）",
        person_days(total_cells, 3.0)
    );
    println!("  參數化產生器：規則數十條 → 全表自動展開，顧問審規則與抽樣");
}

/// 百分比。本檔是報表輸出工具，浮點僅供顯示，符合核心規格 2.3
/// 「浮點只出現在統計輸出」；計數本身皆為整數。
#[allow(clippy::cast_precision_loss)]
fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 / whole as f64 * 100.0
}

/// 人日推估，同樣只供顯示。
#[allow(clippy::cast_precision_loss)]
fn person_days(cells: usize, seconds_per_cell: f64) -> f64 {
    cells as f64 * seconds_per_cell / 3600.0 / 8.0
}

fn format_thousands(value: usize) -> String {
    let text = value.to_string();
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i > 0 && (text.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
