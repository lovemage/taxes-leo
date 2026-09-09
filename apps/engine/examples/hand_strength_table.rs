//! 牌力分類對照表（0907 計劃 Stage 1 的第一個產物）。
//!
//! 計劃 §3.2 要求：「Stage 1 第一個產物是 §2.1 全部範例牌型（特別包含
//! A 高、K 高與各類強聽牌）對隨機合法對手組合的實測百分位對照表；依
//! 對照表修正門檻並留下 golden vectors，再交由牌手顧問確認。」
//!
//! 95%／80%／55%／35% 是校準起點，不是凍結值。
//!
//! 執行：cargo run --release --example hand_strength_table

use poker_engine::card::Card;
use poker_engine::strategy::hand_strength::{classify, HandStrengthConfig};

struct Case {
    expected: &'static str,
    label: &'static str,
    hole: &'static str,
    board: &'static str,
}

/// 計劃 §2.1 的全部範例牌型
const CASES: &[Case] = &[
    // ── nuts ────────────────────────────────────────────────────
    Case { expected: "nuts", label: "堅果同花", hole: "As 5s", board: "Ks 9s 2s" },
    Case { expected: "nuts", label: "葫蘆", hole: "9h 9d", board: "9s 4c 4d" },
    Case { expected: "nuts", label: "四條", hole: "7h 7d", board: "7s 7c 2d" },
    Case { expected: "nuts", label: "安全面頂三條", hole: "Kh Kd", board: "Ks 8c 3d" },
    // ── madePlusDraw ────────────────────────────────────────────
    Case { expected: "made-plus-draw", label: "頂對＋同花聽牌", hole: "As Ts", board: "Ah 7s 3s" },
    Case { expected: "made-plus-draw", label: "頂對＋兩頭順", hole: "Th 9d", board: "Ts 8c 7h" },
    // ── strongMade ──────────────────────────────────────────────
    Case { expected: "strong-made", label: "高兩對", hole: "Ah Kd", board: "As Kc 4d" },
    Case { expected: "strong-made", label: "三條", hole: "6h 6d", board: "6s Kc 2d" },
    Case { expected: "strong-made", label: "超對", hole: "Qh Qd", board: "9s 5c 2d" },
    Case { expected: "strong-made", label: "頂對頂踢腳", hole: "Ah Kd", board: "As 8c 3d" },
    // ── mediumMade ──────────────────────────────────────────────
    Case { expected: "medium-made", label: "頂對弱踢腳", hole: "Ah 4d", board: "As 9c 3d" },
    Case { expected: "medium-made", label: "第二對", hole: "9h 8d", board: "Ks 9c 3d" },
    Case { expected: "medium-made", label: "口袋對（中）", hole: "8h 8d", board: "Ks 6c 3d" },
    // ── bluffCatcher ────────────────────────────────────────────
    Case { expected: "bluff-catcher", label: "底對", hole: "3h 2d", board: "Ks 9c 3d" },
    Case { expected: "bluff-catcher", label: "弱口袋對", hole: "4h 4d", board: "Ks Qc 8d" },
    Case { expected: "bluff-catcher", label: "A 高", hole: "Ah 6d", board: "Ks 9c 3d" },
    Case { expected: "bluff-catcher", label: "K 高", hole: "Kh 6d", board: "Qs 9c 3d" },
    // ── strongDraw ──────────────────────────────────────────────
    Case { expected: "strong-draw", label: "同花聽牌", hole: "Js Ts", board: "9s 5s 2d" },
    Case { expected: "strong-draw", label: "兩頭順", hole: "9h 8d", board: "7s 6c 2d" },
    Case { expected: "strong-draw", label: "同花＋順子複合聽牌", hole: "Js Ts", board: "9s 8s 2d" },
    // ── weakDraw ────────────────────────────────────────────────
    Case { expected: "weak-draw", label: "卡順", hole: "9h 7d", board: "8s 5c 2d" },
    Case { expected: "weak-draw", label: "後門同花", hole: "Js 4s", board: "9s 6c 2d" },
    Case { expected: "weak-draw", label: "雙高張", hole: "Ah Kd", board: "9s 6c 2d" },
    // ── air ─────────────────────────────────────────────────────
    Case { expected: "air", label: "空氣牌", hole: "7h 4d", board: "Ks Qc 9d" },
    // ── 河牌與退化案例 ──────────────────────────────────────────
    Case { expected: "河牌", label: "河牌堅果同花", hole: "As 5s", board: "Ks 9s 2s 7d 3c" },
    Case { expected: "河牌", label: "河牌頂對", hole: "Ah Kd", board: "As 9c 3d 7h 2s" },
    Case { expected: "河牌", label: "公共牌即順子", hole: "3h 2d", board: "As Kd Qc Jh Ts" },
    Case { expected: "河牌", label: "四張同花公共牌", hole: "3h 2d", board: "As Ks Qs Js 4d" },
    Case { expected: "河牌", label: "公共牌葫蘆", hole: "3h 2d", board: "As Ad Ac Ks Kd" },
];

fn main() {
    let config = HandStrengthConfig::ENGINEERING;

    println!("牌力分類對照表");
    println!("**對手模型：均勻隨機合法組合**（對單一對手的牌力序位）\n");
    println!(
        "工程門檻（未簽核）：nuts {} ／ strongMade {} ／ mediumMade {} ／ 攤牌價值 {}",
        config.nuts_floor_myriad,
        config.strong_made_floor_myriad,
        config.medium_made_floor_myriad,
        config.showdown_value_floor_myriad
    );
    println!(
        "強聽牌門檻 {:.2} 張；高張補牌折算 {:.2} 張；污染補牌折算 {:.2} 張\n",
        f64::from(config.strong_draw_floor_centi) / 100.0,
        f64::from(config.overcard_out_weight_centi) / 100.0,
        f64::from(config.polluted_out_weight_centi) / 100.0,
    );

    println!(
        "{:<16} {:<8} {:<16} {:>8} {:>5} {:>8} {:>8}  {:<16} 旗標",
        "範例", "底牌", "公共牌", "百分位", "raw", "有效補牌", "聽牌補牌", "實測組別"
    );
    println!("{}", "-".repeat(120));

    let mut mismatches = 0usize;
    for case in CASES {
        let hole = parse_hole(case.hole);
        let board = parse_board(case.board);
        let snapshot = classify(hole, &board, &config);

        let mut flags = Vec::new();
        if snapshot.nut_draw {
            flags.push("堅果聽牌");
        }
        if snapshot.combo_draw {
            flags.push("複合聽牌");
        }
        if snapshot.backdoor_draw {
            flags.push("後門");
        }
        if !snapshot.improves_board {
            flags.push("未改善公共牌");
        }

        let actual = snapshot.group.key();
        let matched = case.expected == "河牌" || actual == case.expected;
        if !matched {
            mismatches += 1;
        }

        println!(
            "{:<16} {:<8} {:<16} {:>8} {:>5} {:>8.2} {:>8.2}  {:<16} {}{}",
            case.label,
            case.hole,
            case.board,
            snapshot.random_opponent_percentile_myriad,
            snapshot.raw_outs,
            f64::from(snapshot.effective_outs_centi) / 100.0,
            f64::from(snapshot.draw_outs_centi) / 100.0,
            actual,
            flags.join("／"),
            if matched { String::new() } else { format!("  ← 預期 {}", case.expected) }
        );
    }

    println!("\n與計劃 §2.1 的牌手直覺不符：{mismatches} 筆");
    if mismatches > 0 {
        println!(
            "不符的項目不必然是錯的：§2.1 的例子是對「對手續打範圍」的直覺，\n             而百分位對的是均勻隨機的合法組合。差異需由牌手顧問裁定\n             （計劃 §十 待確認 2 與 4）。"
        );
    }
    println!(
        "\n門檻尚未由牌手顧問簽核（consultantApproved = {}）。\
         上表是校準起點，不是凍結值。",
        config.consultant_approved
    );
}

fn parse_hole(text: &str) -> [Card; 2] {
    let cards = parse_board(text);
    [cards[0], cards[1]]
}

fn parse_board(text: &str) -> Vec<Card> {
    text.split_whitespace()
        .map(|t| Card::parse(t).unwrap_or_else(|| panic!("無法解析牌張 {t}")))
        .collect()
}
