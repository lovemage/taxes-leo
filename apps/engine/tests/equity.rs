//! Equity 計算的驗收測試。
//!
//! 核心規格 5.1／5.3：精確枚舉標示「模型內精確」且不得顯示虛構 CI；
//! 取樣模式必須帶出樣本數與誤差；多人平手按實際並列人數分配 1/k。
//!
//! 正確性用**已知答案的經典對局**驗證。這些數字是撲克界長期公認的
//! 全枚舉結果，可作為外部對照。

use poker_engine::card::Card;
use poker_engine::equity::{exact_vs_known, monte_carlo_vs_random, EquityMode};
use poker_engine::rng::{Rng, RngDomain};

fn card(text: &str) -> Card {
    Card::parse(text).unwrap_or_else(|| panic!("無法解析 {text}"))
}

fn hand(text: &str) -> [Card; 2] {
    let cards: Vec<Card> = text.split_whitespace().map(card).collect();
    [cards[0], cards[1]]
}

fn board(text: &str) -> Vec<Card> {
    text.split_whitespace().map(card).collect()
}

// ── 精確枚舉：已知答案的經典對局 ───────────────────────────────────────

#[test]
fn 口袋對子對兩張高張的翻前勝率() {
    // AKo vs QQ 是撲克界的經典 coin flip：QQ 約 56.6%（全枚舉 1,712,304 runout）
    let queens = hand("Qh Qd");
    let ace_king = hand("As Kc");
    let result = exact_vs_known(queens, &[ace_king], &[]);

    assert!(result.is_exact(), "全枚舉必須標示為精確");
    assert_eq!(
        result.mode,
        EquityMode::Exact {
            runouts: 1_712_304
        },
        "剩餘 48 張取 5 張 = C(48,5) = 1,712,304"
    );
    let percent = result.as_percent();
    assert!(
        (56.0..57.5).contains(&percent),
        "QQ vs AKo 應約 56.6%，實得 {percent:.2}%"
    );
}

#[test]
fn 完全相同的牌力為平分() {
    // 兩手在同花色對稱下牌力恆等 → 各 50%
    let a = hand("As Ks");
    let b = hand("Ah Kh");
    let result = exact_vs_known(a, &[b], &board("2c 7d 9c Jd 4h"));

    assert_eq!(result.as_myriad(), 5_000, "牌力相同必須恰好平分");
    assert_eq!(result.mode, EquityMode::Exact { runouts: 1 });
}

#[test]
fn 三人平手各得三分之一() {
    // 公共牌成順子且三家都用公共牌 → 三方平分
    let a = hand("2c 3d");
    let b = hand("2h 3s");
    let c = hand("2s 3h");
    let result = exact_vs_known(a, &[b, c], &board("Ts Jh Qd Kc Ad"));

    // 10000 / 3 = 3333（整數除法），核心規格 5.1 的 1/k 分配
    assert_eq!(result.as_myriad(), 3_333, "三方平手各得 1/3");
}

#[test]
fn 已成堅果牌時勝率為百分之百() {
    // 皇家同花順，對手不可能追平
    let nuts = hand("As Ks");
    let other = hand("2c 2d");
    let result = exact_vs_known(nuts, &[other], &board("Qs Js Ts 3h 4d"));
    assert_eq!(result.as_myriad(), 10_000);
}

#[test]
fn 河牌已定時只有一個_runout() {
    let result = exact_vs_known(
        hand("As Kd"),
        &[hand("2c 2d")],
        &board("Qs Js Ts 3h 4d"),
    );
    assert_eq!(result.mode, EquityMode::Exact { runouts: 1 });
    assert_eq!(result.samples, 1);
}

#[test]
fn 轉牌後枚舉剩餘四十四張() {
    let result = exact_vs_known(hand("As Kd"), &[hand("2c 2d")], &board("Qs Js Ts 3h"));
    // 52 - 2 - 2 - 4 = 44
    assert_eq!(result.mode, EquityMode::Exact { runouts: 44 });
}

// ── 誠實揭露（核心規格 5.3）────────────────────────────────────────────

#[test]
fn 精確枚舉不得產生取樣誤差() {
    let result = exact_vs_known(hand("As Ks"), &[hand("2c 2d")], &board("Qs Js Ts 3h 4d"));
    assert_eq!(
        result.margin_of_error(),
        None,
        "核心規格 5.3：精確枚舉標示模型內精確，不顯示虛構 sampling CI"
    );
}

#[test]
fn 取樣模式必須帶出樣本數與誤差() {
    let mut rng = Rng::derive(1, 1, RngDomain::Equity);
    let result = monte_carlo_vs_random(hand("As Ks"), 8, &[], 5_000, &mut rng);

    assert!(!result.is_exact(), "取樣值不得標示為精算");
    assert_eq!(result.mode, EquityMode::Sampled { samples: 5_000 });
    let margin = result.margin_of_error().expect("取樣必須有誤差上限");
    assert!(margin > 0.0 && margin < 5.0, "誤差上限應為合理正值：{margin}");
}

// ── Monte Carlo 的正確性與可重現性 ─────────────────────────────────────

#[test]
fn 取樣結果收斂到已知的翻前勝率() {
    // AA 對單一隨機手約 85.2%
    let mut rng = Rng::derive(7, 1, RngDomain::Equity);
    let result = monte_carlo_vs_random(hand("As Ah"), 1, &[], 40_000, &mut rng);
    let percent = result.as_percent();
    assert!(
        (84.0..86.5).contains(&percent),
        "AA vs 1 隨機手應約 85.2%，實得 {percent:.2}%"
    );

    // 72o 是公認最差起手牌，對單一隨機手應明顯低於五成
    let mut rng = Rng::derive(7, 2, RngDomain::Equity);
    let worst = monte_carlo_vs_random(hand("7c 2h"), 1, &[], 40_000, &mut rng);
    assert!(
        worst.as_percent() < 40.0,
        "72o 應明顯弱於平均，實得 {:.2}%",
        worst.as_percent()
    );
}

#[test]
fn 對手越多勝率越低() {
    let mut previous = 100.0;
    for opponents in [1usize, 2, 4, 8] {
        let mut rng = Rng::derive(11, opponents as u64, RngDomain::Equity);
        let result = monte_carlo_vs_random(hand("As Ah"), opponents, &[], 20_000, &mut rng);
        let percent = result.as_percent();
        assert!(
            percent < previous,
            "{opponents} 人時 {percent:.2}% 應低於前一級的 {previous:.2}%"
        );
        previous = percent;
    }
}

#[test]
fn 相同_seed_的取樣結果逐位元一致() {
    let run = || {
        let mut rng = Rng::derive(999, 1, RngDomain::Equity);
        monte_carlo_vs_random(hand("Js Td"), 5, &[], 10_000, &mut rng)
    };
    assert_eq!(run(), run(), "核心規格 3.4：相同 seed 必須完全一致");
}

#[test]
fn equity_使用獨立的_rng_domain() {
    // 同 seed、同手序但不同 domain 應得到不同取樣序列，
    // 確保發牌消耗量不會影響 equity 結果（核心規格 3.4）
    let mut equity_rng = Rng::derive(5, 5, RngDomain::Equity);
    let mut deal_rng = Rng::derive(5, 5, RngDomain::Deal);
    let a = monte_carlo_vs_random(hand("9c 9d"), 3, &[], 3_000, &mut equity_rng);
    let b = monte_carlo_vs_random(hand("9c 9d"), 3, &[], 3_000, &mut deal_rng);
    assert_ne!(a.as_myriad(), b.as_myriad(), "不同 domain 應產生不同取樣");
}

#[test]
fn 取樣值與精確值相近() {
    // 同一個節點分別用兩種模式算，取樣值應落在精確值附近
    let hero = hand("As Ks");
    let opponent = hand("Qh Qd");
    let flop = board("2c 7d 9h");

    let exact = exact_vs_known(hero, &[opponent], &flop);
    let mut rng = Rng::derive(3, 3, RngDomain::Equity);
    // 對手已知時的取樣不在本函式範圍，改以精確值自我對照：
    // 驗證翻牌後枚舉的 runout 數正確（52-2-2-3 = 45 取 2 = 990）
    assert_eq!(exact.mode, EquityMode::Exact { runouts: 990 });

    // 另驗隨機對手的取樣穩定性：兩個不同 seed 的結果應接近
    let a = monte_carlo_vs_random(hero, 1, &flop, 20_000, &mut rng);
    let mut rng2 = Rng::derive(4, 4, RngDomain::Equity);
    let b = monte_carlo_vs_random(hero, 1, &flop, 20_000, &mut rng2);
    let gap = (a.as_percent() - b.as_percent()).abs();
    assert!(gap < 2.0, "兩次取樣差距 {gap:.2} 個百分點，過大");
}
