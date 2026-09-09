//! Postflop 節點規模與快照體積精算（0907 計劃 Stage 0 的規模閘門）。
//!
//! 計劃 §4.2 要求「用程式列出真正可達的節點並建立數量／序列化體積閘門，
//! 不能先用笛卡兒積寫死內容」，而且要**分別**輸出核心節點、加入 facing
//! size 後、加入 pot type／有效籌碼後的數量——一次只給總數的話，沒有人
//! 說得出是哪一個維度把節點數推爆的。
//!
//! 執行：cargo run --release --example postflop_content_size

use std::collections::BTreeMap;

use poker_engine::hand::Street;
use poker_engine::strategy::postflop::{
    enumerate_postflop_nodes, node_count_report, BoardConnectivity, BoardSurface, PostflopNode,
    PostflopSituation,
};

/// 一個節點在完整自包含快照裡的估計位元組數。
///
/// canonical 節點鍵（約 60 bytes）＋ 六個動作的萬分比權重與 source／
/// version 欄位。核心規格 3.3 要求 manifest 能在沒有外部版本表的情況下
/// 獨立重建策略，因此估的是**完整內容**，不是指標。
const BYTES_PER_NODE: usize = 160;

/// 翻前既有的快照閘門，拿來對照（計劃 §6.2）。
const PREFLOP_BUDGET_KB: usize = 80;

fn main() {
    let nodes = enumerate_postflop_nodes();
    let report = node_count_report();

    println!("Postflop 節點規模精算\n");
    println!("節點集合版本：{}", report.node_set_version);
    println!("核心節點（街 × 狀態 × 線路 × 外觀 × 結構 × 牌力組）：{}", report.core);
    println!("＋面對下注三檔尺度：{}", report.with_facing_size);
    println!("＋底池類型（單次加注／3-bet／4-bet）：{}", report.with_pot_type);
    println!("＋九檔有效籌碼：{}", report.with_stack_bucket);
    println!(
        "（未排除的笛卡兒積：{}，排除規則擋掉 {} 個不可達組合）\n",
        report.cartesian,
        report.cartesian.saturating_sub(report.with_facing_size)
    );

    println!("依街別：");
    println!("{:>6} {:>10} {:>12} {:>12}", "街", "節點數", "無人下注", "面對下注");
    println!("{}", "-".repeat(44));
    for street in [Street::Flop, Street::Turn, Street::River] {
        let of_street: Vec<&PostflopNode> =
            nodes.iter().filter(|n| n.street == street).collect();
        let no_bet = of_street
            .iter()
            .filter(|n| n.situation == PostflopSituation::NoBet)
            .count();
        println!(
            "{:>6} {:>10} {:>12} {:>12}",
            street_name(street),
            of_street.len(),
            no_bet,
            of_street.len() - no_bet
        );
    }

    println!("\n依線路：");
    let mut by_line: BTreeMap<&str, usize> = BTreeMap::new();
    for node in &nodes {
        *by_line.entry(node.line.label()).or_default() += 1;
    }
    println!("{:>28} {:>10}", "線路", "節點數");
    println!("{}", "-".repeat(40));
    for (label, count) in &by_line {
        println!("{label:>28} {count:>10}");
    }

    println!("\n依牌面（外觀 × 結構）：");
    println!("{:>16} {:>8} {:>8}", "外觀", "乾燥", "濕潤");
    println!("{}", "-".repeat(34));
    for surface in BoardSurface::ALL {
        let count = |connectivity: BoardConnectivity| {
            nodes
                .iter()
                .filter(|n| n.surface == surface && n.connectivity == connectivity)
                .count()
        };
        println!(
            "{:>16} {:>8} {:>8}",
            surface.label(),
            count(BoardConnectivity::Dry),
            count(BoardConnectivity::Wet)
        );
    }

    println!("\n完整自包含快照的估計體積（每節點 {BYTES_PER_NODE} bytes）：");
    println!("{:>28} {:>12} {:>10}", "維度", "節點數", "體積");
    println!("{}", "-".repeat(52));
    for (label, count) in [
        ("核心", report.core),
        ("＋面對尺度", report.with_facing_size),
        ("＋底池類型", report.with_pot_type),
        ("＋有效籌碼", report.with_stack_bucket),
    ] {
        let kb = count * BYTES_PER_NODE / 1_024;
        println!("{label:>28} {count:>12} {kb:>8} KB");
    }

    let current_kb = report.with_facing_size * BYTES_PER_NODE / 1_024;
    println!(
        "\n目前列舉輸出 {} 個節點、約 {} KB，翻前既有閘門為 {} KB。",
        report.with_facing_size, current_kb, PREFLOP_BUDGET_KB
    );
    if current_kb > PREFLOP_BUDGET_KB {
        println!(
            "**超出翻前閘門 {} KB。** 使用者實際只會覆寫稀疏的一小部分節點，\n\
             因此快照存的是覆寫清單而不是全部節點；這個數字是「全部填滿」的上界，\n\
             供計劃 §6.2 決定編碼與上限時參考。",
            current_kb - PREFLOP_BUDGET_KB
        );
    }
}

const fn street_name(street: Street) -> &'static str {
    match street {
        Street::Preflop => "翻前",
        Street::Flop => "翻牌",
        Street::Turn => "轉牌",
        Street::River => "河牌",
    }
}
