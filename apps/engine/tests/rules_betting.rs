//! 下注結構的驗收測試向量。
//!
//! 對應 `德州撲克規則細則.md` 第九章 R1–R6。每個案例都可人工驗算，
//! 案例編號與規格一致，改規格時兩邊必須一起改。
//!
//! 盲注一律 1/2（最小籌碼單位即 1），座位順序為 0,1,2,... 順時針。

use poker_engine::betting::{Action, BettingRound, SeatState};
use poker_engine::Chips;

fn c(n: u64) -> Chips {
    Chips::new(n)
}

/// 建立 1/2 的翻前回合。
///
/// 座位配置：0=SB(1)、1=BB(2)、2..=UTG 起。首行動者為座位 2。
fn preflop_1_2(seats: usize, stack: u64) -> BettingRound {
    let mut committed = vec![Chips::ZERO; seats];
    committed[0] = c(1);
    committed[1] = c(2);
    let mut stacks = vec![c(stack); seats];
    stacks[0] -= c(1);
    stacks[1] -= c(2);
    BettingRound::new_preflop(
        stacks,
        vec![SeatState::Active; seats],
        committed,
        2,
        c(2), // 最大強制下注 = BB
    )
}

/// 建立 1/2 + UTG straddle 的翻前回合。
///
/// 座位：0=SB(1)、1=BB(2)、2=UTG straddle。首行動者為 straddle 左邊（座位 3）。
fn preflop_with_straddle(seats: usize, stack: u64, straddles: &[u64]) -> BettingRound {
    let mut committed = vec![Chips::ZERO; seats];
    committed[0] = c(1);
    committed[1] = c(2);
    let mut stacks = vec![c(stack); seats];
    stacks[0] -= c(1);
    stacks[1] -= c(2);
    for (i, &amount) in straddles.iter().enumerate() {
        let seat = 2 + i;
        committed[seat] = c(amount);
        stacks[seat] -= c(amount);
    }
    let largest = c(*straddles.last().expect("至少一段 straddle"));
    let first_to_act = 2 + straddles.len();
    BettingRound::new_preflop(
        stacks,
        vec![SeatState::Active; seats],
        committed,
        first_to_act,
        largest,
    )
}

// ── R1 ──────────────────────────────────────────────────────────────────
// 1/2，UTG raise to 7，次位最小加注必須為 to 12，非 to 9。

#[test]
fn r1_最小加注額以增額計算而非以當前下注額加一個bb() {
    let mut round = preflop_1_2(6, 200);

    // UTG 面對 BB=2，增額 2，最小加注 to 4
    let legal = round.legal_actions().expect("UTG 應有合法行動");
    assert_eq!(legal.seat, 2);
    assert_eq!(legal.raise.expect("UTG 可加注").min_to, c(4));

    round.apply(Action::RaiseTo(c(7))).expect("raise to 7 合法");

    // 增額變成 7-2=5，下一位最小加注 to 7+5=12
    let legal = round.legal_actions().expect("次位應有合法行動");
    assert_eq!(legal.seat, 3);
    assert_eq!(
        legal.raise.expect("次位可加注").min_to,
        c(12),
        "最小加注必須是 to 12（7+5），不是 to 9"
    );
    assert_eq!(legal.call_to, Some(c(7)));
}

// ── R2 ──────────────────────────────────────────────────────────────────
// A raise to 10、B call、C all-in to 15（短額）→ A、B 只能 fold/call。

#[test]
fn r2_短額全下不重開已行動者的加注權() {
    // 座位 2=A、3=B、4=C（C 僅 15 籌碼，全下即短額）
    let mut committed = vec![Chips::ZERO; 6];
    committed[0] = c(1);
    committed[1] = c(2);
    let mut stacks = vec![c(200), c(200), c(200), c(200), c(15), c(200)];
    stacks[0] -= c(1);
    stacks[1] -= c(2);
    let mut round =
        BettingRound::new_preflop(stacks, vec![SeatState::Active; 6], committed, 2, c(2));

    round.apply(Action::RaiseTo(c(10))).expect("A raise to 10");
    assert_eq!(round.raise_increment(), c(8), "增額 = 10-2 = 8");

    round.apply(Action::Call).expect("B call 10");

    // C 全下 15：15 < 10+8=18，屬短額全下
    assert_eq!(round.to_act(), Some(4));
    round.apply(Action::AllIn).expect("C all-in to 15");
    assert_eq!(round.current_bet(), c(15));
    assert_eq!(round.raise_increment(), c(8), "短額全下不得更新增額");

    // 輪回座位 5（尚未行動）→ 加注權不受影響
    let legal = round.legal_actions().expect("座位 5 應有合法行動");
    assert_eq!(legal.seat, 5);
    assert!(
        legal.raise.is_some(),
        "尚未行動者的加注權不受短額全下影響"
    );
    round.apply(Action::Fold).expect("座位 5 fold");

    round.apply(Action::Fold).expect("SB fold");
    round.apply(Action::Fold).expect("BB fold");

    // A 已對 10 行動過，面對短額全下只能 fold 或 call
    let legal = round.legal_actions().expect("A 應有合法行動");
    assert_eq!(legal.seat, 2);
    assert!(
        legal.raise.is_none(),
        "A 已對當前下注額行動過，短額全下不得重開其加注權"
    );
    assert_eq!(legal.call_to, Some(c(15)));
    assert!(legal.can_fold);

    round.apply(Action::Call).expect("A call 15");

    // B 同理
    let legal = round.legal_actions().expect("B 應有合法行動");
    assert_eq!(legal.seat, 3);
    assert!(legal.raise.is_none(), "B 同樣不得加注");
}

// ── R3 ──────────────────────────────────────────────────────────────────
// 承 R2，若 C all-in to 18（達完整加注門檻）→ A、B 加注權重開，最小 to 26。

#[test]
fn r3_達門檻的全下重開加注權並更新增額() {
    let mut committed = vec![Chips::ZERO; 6];
    committed[0] = c(1);
    committed[1] = c(2);
    let mut stacks = vec![c(200), c(200), c(200), c(200), c(18), c(200)];
    stacks[0] -= c(1);
    stacks[1] -= c(2);
    let mut round =
        BettingRound::new_preflop(stacks, vec![SeatState::Active; 6], committed, 2, c(2));

    round.apply(Action::RaiseTo(c(10))).expect("A raise to 10");
    round.apply(Action::Call).expect("B call 10");
    round.apply(Action::AllIn).expect("C all-in to 18");

    assert_eq!(round.current_bet(), c(18));
    assert_eq!(round.raise_increment(), c(8), "增額 = 18-10 = 8");

    round.apply(Action::Fold).expect("座位 5 fold");
    round.apply(Action::Fold).expect("SB fold");
    round.apply(Action::Fold).expect("BB fold");

    let legal = round.legal_actions().expect("A 應有合法行動");
    assert_eq!(legal.seat, 2);
    let raise = legal
        .raise
        .expect("完整加注後 A 的加注權必須重開");
    assert_eq!(raise.min_to, c(26), "最小加注 to 18+8 = 26");
}

// ── R4 ──────────────────────────────────────────────────────────────────
// 兩個連續短額全下，增額不累加，已行動者仍不得加注。

#[test]
fn r4_多個短額全下的增額不累加() {
    // 2=A(深)、3=B(13)、4=C(15)、5=D(深)
    let mut committed = vec![Chips::ZERO; 6];
    committed[0] = c(1);
    committed[1] = c(2);
    let mut stacks = vec![c(200), c(200), c(200), c(13), c(15), c(200)];
    stacks[0] -= c(1);
    stacks[1] -= c(2);
    let mut round =
        BettingRound::new_preflop(stacks, vec![SeatState::Active; 6], committed, 2, c(2));

    round.apply(Action::RaiseTo(c(10))).expect("A raise to 10");
    assert_eq!(round.raise_increment(), c(8));

    // B 全下 13：13 < 18，短額
    round.apply(Action::AllIn).expect("B all-in to 13");
    assert_eq!(round.current_bet(), c(13));
    assert_eq!(round.raise_increment(), c(8), "短額不更新增額");

    // C 全下 15：門檻仍是 10+8=18（不是 13+某個累加值），15 < 18 仍為短額
    round.apply(Action::AllIn).expect("C all-in to 15");
    assert_eq!(round.current_bet(), c(15));
    assert_eq!(
        round.raise_increment(),
        c(8),
        "兩次短額全下的增額不得累加成完整加注"
    );

    round.apply(Action::Fold).expect("D fold");
    round.apply(Action::Fold).expect("SB fold");
    round.apply(Action::Fold).expect("BB fold");

    // A 仍不得加注
    let legal = round.legal_actions().expect("A 應有合法行動");
    assert_eq!(legal.seat, 2);
    assert!(
        legal.raise.is_none(),
        "連續兩個短額全下仍不重開 A 的加注權"
    );
    assert_eq!(legal.call_to, Some(c(15)));
}

// ── R5 ──────────────────────────────────────────────────────────────────
// 1/2 + UTG straddle to 4：當前下注額 4、行動從 straddle 左邊起、
// straddle 持有 option、最小加注 to 8。

#[test]
fn r5_straddle_為_live_blind_且持有翻前_option() {
    let mut round = preflop_with_straddle(6, 200, &[4]);

    assert_eq!(round.current_bet(), c(4), "當前下注額為 straddle 金額");
    assert_eq!(round.raise_increment(), c(4), "增額初始 = 最大 straddle");

    let legal = round.legal_actions().expect("首行動者應有合法行動");
    assert_eq!(legal.seat, 3, "行動從 straddle（座位 2）左邊的座位 3 起");
    assert_eq!(
        legal.raise.expect("可加注").min_to,
        c(8),
        "最小加注 to 4+4 = 8"
    );

    // 全部跟注／棄牌後，straddle 位仍保有 option
    round.apply(Action::Call).expect("座位 3 call");
    round.apply(Action::Fold).expect("座位 4 fold");
    round.apply(Action::Fold).expect("座位 5 fold");
    round.apply(Action::Fold).expect("SB fold");
    round.apply(Action::Fold).expect("BB fold");

    assert!(!round.is_complete(), "straddle 位尚未行動，回合不得結束");
    let legal = round.legal_actions().expect("straddle 位應有 option");
    assert_eq!(legal.seat, 2, "straddle 位擁有翻前最後行動權");
    assert!(legal.can_check, "無人加注時 straddle 位可過牌");
    assert!(legal.raise.is_some(), "straddle 位可行使 option 加注");
}

// ── R6 ──────────────────────────────────────────────────────────────────
// 1/2 + straddle to 4 + re-straddle to 8：行動從 re-straddle 左邊起。

#[test]
fn r6_double_straddle_的行動順序與最小加注() {
    let round = preflop_with_straddle(7, 200, &[4, 8]);

    assert_eq!(round.current_bet(), c(8), "當前下注額為最大 straddle");
    assert_eq!(round.raise_increment(), c(8));

    let legal = round.legal_actions().expect("首行動者應有合法行動");
    assert_eq!(
        legal.seat, 4,
        "行動從 re-straddle（座位 3）左邊的座位 4 起"
    );
    assert_eq!(
        legal.raise.expect("可加注").min_to,
        c(16),
        "最小加注 to 8+8 = 16"
    );
}

// ── 補充：BB option 與翻後首次下注 ─────────────────────────────────────

#[test]
fn bb_在無人加注時保有_option() {
    let mut round = preflop_1_2(6, 200);
    for _ in 0..4 {
        round.apply(Action::Fold).expect("UTG 起依序棄牌");
    }
    round.apply(Action::Call).expect("SB 補齊到 2");

    assert!(!round.is_complete(), "BB 尚未行動，回合不得結束");
    let legal = round.legal_actions().expect("BB 應有 option");
    assert_eq!(legal.seat, 1);
    assert!(legal.can_check);
    assert!(legal.raise.is_some(), "BB 可行使 option 加注");

    round.apply(Action::Check).expect("BB check");
    assert!(round.is_complete(), "BB check 後翻前結束");
}

#[test]
fn 翻後首次下注最小為一個_bb_且允許_check_raise() {
    let mut round = BettingRound::new_postflop(
        vec![c(200); 3],
        vec![SeatState::Active; 3],
        0,
        c(2),
    );

    let legal = round.legal_actions().expect("座位 0 應有合法行動");
    assert!(legal.can_check);
    assert_eq!(legal.raise.expect("可下注").min_to, c(2), "首次下注最小 1BB");

    round.apply(Action::Check).expect("座位 0 check");
    round.apply(Action::RaiseTo(c(5))).expect("座位 1 下注 5");

    // 座位 0 已 check，但其後發生完整加注 → 加注權重開（check-raise）
    round.apply(Action::Fold).expect("座位 2 fold");
    let legal = round.legal_actions().expect("座位 0 應有合法行動");
    assert_eq!(legal.seat, 0);
    assert!(legal.raise.is_some(), "現實規則允許 check-raise");
    assert_eq!(legal.raise.expect("可加注").min_to, c(10), "5+5 = 10");
}
