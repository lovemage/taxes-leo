//! Hand evaluator 的驗收測試。
//!
//! 實做計劃 M1：「Hand evaluator，對照公開測試向量驗證」。
//! 核心規格 5.1：「Complete-hand evaluator 必須精確。」
//!
//! 最後一則是窮舉交叉驗證：對隨機的 7 張牌，比對「分類法直接評 7 張」
//! 與「暴力枚舉 C(7,5)=21 種組合取最強」的結果是否一致。這是本檔最有
//! 價值的一則——它用一個獨立且顯然正確的實作來檢查主實作。

use poker_engine::card::{full_deck, Card};
use poker_engine::eval::{evaluate, Category, HandRank};

fn hand(text: &str) -> Vec<Card> {
    text.split_whitespace()
        .map(|t| Card::parse(t).unwrap_or_else(|| panic!("無法解析牌張 {t}")))
        .collect()
}

fn rank_of(text: &str) -> HandRank {
    evaluate(&hand(text))
}

// ── 類別排序 ────────────────────────────────────────────────────────────

#[test]
fn 九個類別的強弱順序正確() {
    let ordered = [
        ("7h 5s 4d 3c 2h", Category::HighCard),
        ("7h 7s 4d 3c 2h", Category::Pair),
        ("7h 7s 4d 4c 2h", Category::TwoPair),
        ("7h 7s 7d 4c 2h", Category::Trips),
        ("8h 7s 6d 5c 4h", Category::Straight),
        ("Ah Jh 8h 5h 2h", Category::Flush),
        ("7h 7s 7d 4c 4h", Category::FullHouse),
        ("7h 7s 7d 7c 4h", Category::Quads),
        ("8h 7h 6h 5h 4h", Category::StraightFlush),
    ];

    for (text, expected) in ordered {
        assert_eq!(rank_of(text).category(), expected, "{text} 的類別判定錯誤");
    }

    for window in ordered.windows(2) {
        let (weaker, stronger) = (window[0].0, window[1].0);
        assert!(
            rank_of(weaker) < rank_of(stronger),
            "{weaker} 應弱於 {stronger}"
        );
    }
}

// ── 順子邊界 ────────────────────────────────────────────────────────────

#[test]
fn 輪子是最小的順子且_qka23_不成順() {
    let wheel = rank_of("As 2d 3c 4h 5s");
    assert_eq!(wheel.category(), Category::Straight);
    assert!(wheel < rank_of("2s 3d 4c 5h 6s"), "輪子必須是最小順子");

    // A 只能當 1 或 14，不能繞回：Q-K-A-2-3 不成順子
    let not_straight = rank_of("Qs Kd Ac 2h 3s");
    assert_eq!(
        not_straight.category(),
        Category::HighCard,
        "K-A-2-3 不得判為順子"
    );
}

#[test]
fn 皇家同花順是最強牌() {
    let royal = rank_of("As Ks Qs Js Ts");
    assert_eq!(royal.category(), Category::StraightFlush);

    // 任意補入第 6 張，最佳五張仍是皇家同花順（沒有牌能超越它）
    for card in full_deck() {
        let mut cards = hand("As Ks Qs Js Ts");
        if cards.contains(&card) {
            continue;
        }
        cards.push(card);
        assert_eq!(
            evaluate(&cards),
            royal,
            "補入 {card} 後不應改變皇家同花順的牌力"
        );
    }
}

// ── Kicker 比較 ─────────────────────────────────────────────────────────

#[test]
fn 同對子時比_kicker() {
    assert!(rank_of("9h 9s Kd 4c 2h") > rank_of("9h 9s Qd 4c 2h"));
    // 第三 kicker 才分勝負
    assert!(rank_of("9h 9s Kd Qc 5h") > rank_of("9h 9s Kd Qc 4h"));
    // 完全相同的牌力 → 並列
    assert_eq!(rank_of("9h 9s Kd Qc 5h"), rank_of("9c 9d Ks Qh 5d"));
}

#[test]
fn 兩對先比大對再比小對最後比_kicker() {
    assert!(rank_of("Kh Ks 3d 3c 2h") > rank_of("Qh Qs Jd Jc Ah"));
    assert!(rank_of("Kh Ks 5d 5c 2h") > rank_of("Kh Ks 3d 3c Ah"));
    assert!(rank_of("Kh Ks 3d 3c Ah") > rank_of("Kh Ks 3d 3c Qh"));
}

#[test]
fn 四條比_kicker_而葫蘆先比三條() {
    assert!(rank_of("7h 7s 7d 7c Ah") > rank_of("7h 7s 7d 7c Kh"));
    assert!(rank_of("8h 8s 8d 2c 2h") > rank_of("7h 7s 7d Ac Ah"));
}

#[test]
fn 同花比最高張且依序往下() {
    assert!(rank_of("Ah Jh 8h 5h 2h") > rank_of("Kh Qh Jh 9h 7h"));
    assert!(rank_of("Ah Jh 8h 5h 3h") > rank_of("Ah Jh 8h 5h 2h"));
}

// ── 七張取五 ────────────────────────────────────────────────────────────

#[test]
fn 七張中同花優先於使用其他兩張() {
    // 五張紅心同花，另有一對 K：同花勝出
    let r = rank_of("Ah Jh 8h 5h 2h Ks Kd");
    assert_eq!(r.category(), Category::Flush);
}

#[test]
fn 七張中葫蘆優先於同花() {
    // 五張紅心同花，同時 K 三條 + 對子構成葫蘆
    let r = rank_of("Kh Kd Kc 8h 5h 2h Ah");
    assert_eq!(
        r.category(),
        Category::Flush,
        "只有三條沒有對子時，同花仍勝出"
    );

    let r = rank_of("Kh Kd Kc 8h 8s 5h 2h");
    assert_eq!(r.category(), Category::FullHouse, "葫蘆勝過同花");
}

// ── 窮舉交叉驗證 ────────────────────────────────────────────────────────

/// 獨立的參考實作：暴力枚舉 C(7,5) 的 21 種組合，取最強者。
fn brute_force_best(cards: &[Card]) -> HandRank {
    assert_eq!(cards.len(), 7);
    let mut best: Option<HandRank> = None;
    for a in 0..7 {
        for b in (a + 1)..7 {
            // 排除 a、b 兩張，剩下五張即一種組合
            let five: Vec<Card> = (0..7)
                .filter(|&i| i != a && i != b)
                .map(|i| cards[i])
                .collect();
            let r = evaluate(&five);
            best = Some(best.map_or(r, |cur: HandRank| cur.max(r)));
        }
    }
    best.expect("必有組合")
}

/// 確定性的線性同餘產生器。測試不得依賴外部 RNG crate，也不得每次跑出
/// 不同結果——可重現是本專案的鐵則（核心規格 3.4）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        self.0 >> 33
    }
}

#[test]
fn 七張評估與窮舉取五的結果一致() {
    let deck = full_deck();
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);

    for case in 0..20_000 {
        // 由固定 seed 抽 7 張互異的牌
        let mut chosen: Vec<Card> = Vec::with_capacity(7);
        while chosen.len() < 7 {
            let idx = usize::try_from(rng.next() % 52).expect("索引必在 usize 範圍");
            let card = deck[idx];
            if !chosen.contains(&card) {
                chosen.push(card);
            }
        }

        let direct = evaluate(&chosen);
        let brute = brute_force_best(&chosen);
        assert_eq!(
            direct,
            brute,
            "第 {case} 組不一致：{}",
            chosen
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}
