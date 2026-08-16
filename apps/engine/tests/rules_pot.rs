//! 底池分層、odd chip、抽水與守恆的驗收測試向量。
//!
//! 對應 `德州撲克規則細則.md` 第九章 R9、R10、R13、R14。

use poker_engine::pot::{build_layers, settle, RakeConfig};
use poker_engine::Chips;

fn c(n: u64) -> Chips {
    Chips::new(n)
}

// ── R9 ──────────────────────────────────────────────────────────────────
// 三人不等深度全下 → main + 2 side pot 分層正確；
// 棄牌者投入留在 pot 但無資格。

#[test]
fn r9_三人不等深度全下的分層與棄牌者資格() {
    // 座位 0 全下 20、座位 1 全下 50、座位 2 全下 100、座位 3 投入 30 後棄牌
    let contributions = vec![c(20), c(50), c(100), c(30)];
    let folded = vec![false, false, false, true];

    let layers = build_layers(&contributions, &folded);
    assert_eq!(layers.len(), 4, "投入有 20/30/50/100 四個級距");

    // 第 1 層：每人前 20 → 20*4 = 80，資格為未棄牌且投入 ≥20 的三人
    assert_eq!(layers[0].amount, c(80));
    assert_eq!(layers[0].eligible, vec![0, 1, 2]);

    // 第 2 層：20→30 之間，座位 1、2、3 各 10 → 30。
    // 座位 3 有投入但已棄牌，籌碼留在池中卻無資格（規則細則 5.1）
    assert_eq!(layers[1].amount, c(30));
    assert_eq!(layers[1].eligible, vec![1, 2]);

    // 第 3 層：30→50，座位 1、2 各 20 → 40
    assert_eq!(layers[2].amount, c(40));
    assert_eq!(layers[2].eligible, vec![1, 2]);

    // 第 4 層：50→100，僅座位 2 → 50（未被跟注，settle 會退還）
    assert_eq!(layers[3].amount, c(50));
    assert_eq!(layers[3].eligible, vec![2]);
}

#[test]
fn r9_分配時未被跟注的部分退還且不進底池() {
    let contributions = vec![c(20), c(50), c(100), c(30)];
    let folded = vec![false, false, false, true];
    // 座位 0 最強，其次座位 1，座位 2 最弱
    let ranks = vec![Some(30), Some(20), Some(10), None];

    let d = settle(&contributions, &folded, &ranks, 0, RakeConfig::NONE, true);

    // 座位 2 投入 100，第二高為 50，超出的 50 無人跟注須退還
    assert_eq!(d.refunds[2], c(50), "未被跟注的 50 必須退還");
    assert_eq!(d.refunds[0], Chips::ZERO);

    // main pot = 20*4 = 80，由最強的座位 0 取得
    assert_eq!(d.payouts[0], c(80));
    // 其餘層（30 與 40）座位 0 無資格，由次強的座位 1 取得
    assert_eq!(d.payouts[1], c(70));
    assert_eq!(d.payouts[2], Chips::ZERO, "最弱者不分得底池");
    assert_eq!(d.payouts[3], Chips::ZERO, "棄牌者不分得底池");

    d.assert_conserves(&contributions);
}

// ── R10 ─────────────────────────────────────────────────────────────────
// 三人 split，pot 無法整除 → odd chip 由按鈕左側起、有資格者順時針取得。

#[test]
fn r10_odd_chip_由按鈕左側起順時針分配() {
    // 四人各投入 25，總池 100；座位 1、2、3 三人並列，100/3 = 33 餘 1
    let contributions = vec![c(25), c(25), c(25), c(25)];
    let folded = vec![true, false, false, false];
    let ranks = vec![None, Some(50), Some(50), Some(50)];

    // 按鈕在座位 0 → 左側第一位是座位 1，該枚 odd chip 給座位 1
    let d = settle(&contributions, &folded, &ranks, 0, RakeConfig::NONE, true);
    assert_eq!(d.payouts[1], c(34), "按鈕左側第一位有資格者多得 1");
    assert_eq!(d.payouts[2], c(33));
    assert_eq!(d.payouts[3], c(33));
    assert_eq!(d.odd_chips.len(), 1);
    assert_eq!(d.odd_chips[0].seat, 1, "odd chip 去向必須可寫入 log");
    d.assert_conserves(&contributions);

    // 按鈕移到座位 1 → 左側第一位是座位 2
    let d = settle(&contributions, &folded, &ranks, 1, RakeConfig::NONE, true);
    assert_eq!(d.payouts[2], c(34));
    assert_eq!(d.odd_chips[0].seat, 2);
    d.assert_conserves(&contributions);
}

#[test]
fn r10_odd_chip_跳過無資格座位() {
    // 按鈕在座位 0，但座位 1 已棄牌 → odd chip 應落在下一個有資格者座位 2
    let contributions = vec![c(25), c(25), c(25), c(25)];
    let folded = vec![false, true, false, false];
    let ranks = vec![None, None, Some(50), Some(50)];

    let d = settle(&contributions, &folded, &ranks, 0, RakeConfig::NONE, true);
    // 池 100，兩人並列 → 50/50，無餘數
    assert_eq!(d.payouts[2], c(50));
    assert_eq!(d.payouts[3], c(50));
    assert!(d.odd_chips.is_empty(), "整除時不產生 odd chip");

    // 改成三人投入造成餘數：總池 75，兩人分 → 37 餘 1
    let contributions = vec![c(25), c(25), c(25)];
    let folded = vec![false, true, false];
    let ranks = vec![Some(50), None, Some(50)];
    let d = settle(&contributions, &folded, &ranks, 0, RakeConfig::NONE, true);
    assert_eq!(
        d.payouts[2],
        c(38),
        "按鈕左側的座位 1 無資格，odd chip 順時針落到座位 2"
    );
    assert_eq!(d.payouts[0], c(37));
    d.assert_conserves(&contributions);
}

// ── R13 ─────────────────────────────────────────────────────────────────
// 任一案例的籌碼守恆：開始總額 = 結束總額 + rake，誤差為 0。

#[test]
fn r13_籌碼守恆在抽水與多層邊池下成立() {
    let contributions = vec![c(37), c(88), c(88), c(12), c(200)];
    let folded = vec![false, false, true, false, false];
    let ranks = vec![Some(10), Some(90), None, Some(50), Some(20)];
    let rake = RakeConfig {
        basis_points: 500, // 5%
        cap: c(30),
        no_flop_no_drop: true,
    };

    // settle 內部即斷言守恆，這裡再顯式驗一次總額關係
    let d = settle(&contributions, &folded, &ranks, 2, rake, true);
    d.assert_conserves(&contributions);

    let inflow: Chips = contributions.iter().copied().sum();
    let payouts: Chips = d.payouts.iter().copied().sum();
    let refunds: Chips = d.refunds.iter().copied().sum();
    assert_eq!(inflow, payouts + refunds + d.rake);
}

#[test]
fn r13_rake_先向下取整再套用_cap() {
    // 池 199，5% = 9.95 → 向下取整為 9，未達 cap
    let contributions = vec![c(100), c(99)];
    let folded = vec![false, false];
    let ranks = vec![Some(10), Some(5)];
    let rake = RakeConfig {
        basis_points: 500,
        cap: c(30),
        no_flop_no_drop: false,
    };
    // 座位 0 投入 100、座位 1 投入 99 → 退還 1，可抽水底池為 198
    let d = settle(&contributions, &folded, &ranks, 0, rake, true);
    assert_eq!(d.refunds[0], c(1), "超出對手投入的 1 不屬於底池");
    assert_eq!(d.rake, c(9), "198 的 5% = 9.9，向下取整為 9");
    d.assert_conserves(&contributions);

    // cap 生效：池 1000，5% = 50，cap 30 → 取 30
    let contributions = vec![c(500), c(500)];
    let d = settle(&contributions, &[false, false], &ranks, 0, rake, true);
    assert_eq!(d.rake, c(30), "比例值 50 超過 cap 30，取 cap");
    d.assert_conserves(&contributions);
}

// ── R14 ─────────────────────────────────────────────────────────────────
// noFlopNoDrop 開啟且翻前結束 → 該手 rake = 0。

#[test]
fn r14_no_flop_no_drop_在未發_flop_時不抽水() {
    let contributions = vec![c(100), c(100)];
    let folded = vec![false, false];
    let ranks = vec![Some(10), Some(5)];
    let rake = RakeConfig {
        basis_points: 500,
        cap: c(30),
        no_flop_no_drop: true,
    };

    let d = settle(&contributions, &folded, &ranks, 0, rake, false);
    assert_eq!(d.rake, Chips::ZERO, "未發 flop 不抽水");
    assert_eq!(d.payouts[0], c(200));
    d.assert_conserves(&contributions);

    // 關閉 noFlopNoDrop 時，翻前結束仍然抽水
    let rake_always = RakeConfig {
        no_flop_no_drop: false,
        ..rake
    };
    let d = settle(&contributions, &folded, &ranks, 0, rake_always, false);
    assert_eq!(d.rake, c(10), "200 的 5% = 10");
    d.assert_conserves(&contributions);
}

// ── 補充：全員棄牌只剩一人時的無爭議底池 ───────────────────────────────

#[test]
fn 只剩一名未棄牌者時獨得底池且退還未跟注部分() {
    // 座位 2 加注到 50 後其餘棄牌
    let contributions = vec![c(10), c(2), c(50)];
    let folded = vec![true, true, false];
    let ranks = vec![None, None, Some(1)];

    let d = settle(&contributions, &folded, &ranks, 0, RakeConfig::NONE, false);
    // 第二高投入為 10 → 超出的 40 退還
    assert_eq!(d.refunds[2], c(40));
    assert_eq!(d.payouts[2], c(22), "10 + 2 + 10 = 22");
    d.assert_conserves(&contributions);
}
