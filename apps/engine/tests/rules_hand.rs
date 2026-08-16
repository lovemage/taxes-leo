//! 整手循環的驗收測試向量。
//!
//! 對應 `德州撲克規則細則.md` 第九章 R7、R8、R11、R12，
//! 以及核心規格 3.4 的可重現性要求。

use poker_engine::betting::Action;
use poker_engine::chips::Chips;
use poker_engine::hand::{play_hand, ActionProvider, HandSetup};
use poker_engine::strategy::DecisionView;
use poker_engine::pot::RakeConfig;
use poker_engine::rng::{Rng, RngDomain};
use poker_engine::table::{AnteConfig, AnteMode, MuckPolicy, StraddleConfig, TableConfig};

fn c(n: u64) -> Chips {
    Chips::new(n)
}

/// 全部跟注到底的 provider：能過牌就過牌，否則跟注，籌碼不足則全下。
struct CallingStation;

impl ActionProvider for CallingStation {
    fn choose(&mut self, view: &DecisionView) -> Action {
        let legal = &view.legal;
        if legal.can_check {
            Action::Check
        } else if legal.call_to.is_some() {
            Action::Call
        } else {
            Action::AllIn
        }
    }
}

/// 除了指定座位外全部棄牌。
struct FoldAllExcept(usize);

impl ActionProvider for FoldAllExcept {
    fn choose(&mut self, view: &DecisionView) -> Action {
        let legal = &view.legal;
        if legal.seat == self.0 {
            if legal.can_check {
                Action::Check
            } else {
                Action::Call
            }
        } else if legal.can_check {
            Action::Check
        } else {
            Action::Fold
        }
    }
}

fn setup_9max(stack: u64) -> HandSetup {
    HandSetup {
        stacks: vec![c(stack); 9],
        occupied: vec![true; 9],
        button: 0,
        small_blind_seat: Some(1),
        big_blind_seat: 2,
    }
}

/// 每手都必須成立的不變量（核心規格 2.3）。
fn assert_hand_conserves(setup: &HandSetup, result: &poker_engine::hand::HandResult) {
    let before: Chips = setup.stacks.iter().copied().sum();
    let after: Chips = result.final_stacks.iter().copied().sum();
    assert_eq!(
        before,
        after + result.distribution.rake,
        "整手籌碼守恆破壞：開始 {before}，結束 {after} + rake {}",
        result.distribution.rake
    );
}

// ── R7 ──────────────────────────────────────────────────────────────────
// bbAnte 且 BB 籌碼 < ante + BB → ante 先付足，餘額為 blind。

#[test]
fn r7_bb_ante_付款順序為_ante_先_blind_後() {
    let config = TableConfig {
        ante: AnteConfig {
            mode: AnteMode::BbAnte,
            amount: c(1), // 每人份 1，9 人 → BB 代付 9
        },
        ..TableConfig::simple(1, 2)
    };

    // BB（座位 2）只有 10：先付 9 的 ante，剩 1 當 blind
    let mut setup = setup_9max(200);
    setup.stacks[2] = c(10);

    let mut rng = Rng::derive(1, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);

    assert_eq!(
        result.total_contributions[2],
        c(10),
        "BB 應付出全部 10（ante 9 + blind 1）後全下"
    );
    assert_hand_conserves(&setup, &result);
}

#[test]
fn r7_bb_ante_籌碼充足時_ante_與盲注分開計算() {
    let config = TableConfig {
        ante: AnteConfig {
            mode: AnteMode::BbAnte,
            amount: c(1),
        },
        ..TableConfig::simple(1, 2)
    };
    let setup = setup_9max(200);
    let mut rng = Rng::derive(2, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut FoldAllExcept(2));

    // 全員棄牌到 BB：BB 付 9 ante + 2 blind，SB 付 1
    // ante 是 dead money，不影響他人 call 額
    assert_eq!(result.total_contributions[1], c(1), "SB 只需付盲注");
    assert_hand_conserves(&setup, &result);
}

// ── R8 ──────────────────────────────────────────────────────────────────
// perPlayer ante，某座 all-in for ante → 仍發牌；
// ante 為 dead money，不影響他人 call 額。

#[test]
fn r8_per_player_ante_不影響他人的跟注金額() {
    let config = TableConfig {
        ante: AnteConfig {
            mode: AnteMode::PerPlayer,
            amount: c(1),
        },
        ..TableConfig::simple(1, 2)
    };
    let setup = setup_9max(200);
    let mut rng = Rng::derive(3, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);

    // 每人 1 ante；UTG 起全部跟注到 BB=2
    // 若 ante 被錯誤計入當街投入，跟注額會變成 1 而非 2
    let utg = 3;
    assert_eq!(
        result.total_contributions[utg],
        c(3),
        "UTG 應付 1 ante + 2 跟注；ante 不得抵扣跟注額"
    );
    assert_hand_conserves(&setup, &result);
}

#[test]
fn r8_籌碼不足以付_ante_者仍然發牌() {
    let config = TableConfig {
        ante: AnteConfig {
            mode: AnteMode::PerPlayer,
            amount: c(5),
        },
        ..TableConfig::simple(1, 2)
    };
    let mut setup = setup_9max(200);
    setup.stacks[5] = c(3); // 不足 5

    let mut rng = Rng::derive(4, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);

    assert!(
        result.hole_cards[5].is_some(),
        "籌碼不足以付 ante 者仍須發牌"
    );
    assert_eq!(result.total_contributions[5], c(3), "全下付 ante");
    assert_hand_conserves(&setup, &result);
}

// ── R11 / R12 ───────────────────────────────────────────────────────────
// 攤牌亮牌順序與 muck。

#[test]
fn r11_河牌全_check_時輸家可_muck() {
    let config = TableConfig::simple(1, 2);
    // 三人桌形式（9 座但只有 3 人在座），確保能走到攤牌
    let setup = HandSetup {
        stacks: vec![c(200); 3],
        occupied: vec![true; 3],
        button: 0,
        small_blind_seat: Some(1),
        big_blind_seat: 2,
    };

    let mut rng = Rng::derive(11, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);

    assert_eq!(result.board.len(), 5, "全程跟注應走到河牌");
    let revealed_count = result.revealed.iter().filter(|&&r| r).count();
    assert!(revealed_count >= 1, "至少一名玩家必須亮牌");
    assert!(
        revealed_count <= 3,
        "realistic 模式下不會超過在場人數"
    );
    assert_hand_conserves(&setup, &result);
}

#[test]
fn r12_muck_者的底牌不得列為已公開() {
    let config = TableConfig::simple(1, 2);
    let setup = HandSetup {
        stacks: vec![c(200); 3],
        occupied: vec![true; 3],
        button: 0,
        small_blind_seat: Some(1),
        big_blind_seat: 2,
    };

    // 找一個確實有人 muck 的 seed，驗證未亮牌者不被標記為已公開
    let mut found_muck = false;
    for seed in 0..200u64 {
        let mut rng = Rng::derive(seed, 1, RngDomain::Deal);
        let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);
        if result.board.len() < 5 {
            continue;
        }
        let contenders = result.folded.iter().filter(|&&f| !f).count();
        let revealed = result.revealed.iter().filter(|&&r| r).count();
        if revealed < contenders {
            found_muck = true;
            // 未亮牌者必然不是贏家（muck 判定即「贏不了才蓋牌」）
            for seat in 0..3 {
                if !result.folded[seat] && !result.revealed[seat] {
                    assert_eq!(
                        result.distribution.payouts[seat],
                        Chips::ZERO,
                        "muck 者不得分得底池"
                    );
                }
            }
            break;
        }
    }
    assert!(found_muck, "200 個 seed 中應至少出現一次 muck");
}

#[test]
fn always_show_政策下所有未棄牌者都亮牌() {
    let config = TableConfig {
        muck: MuckPolicy::AlwaysShow,
        ..TableConfig::simple(1, 2)
    };
    let setup = HandSetup {
        stacks: vec![c(200); 3],
        occupied: vec![true; 3],
        button: 0,
        small_blind_seat: Some(1),
        big_blind_seat: 2,
    };

    let mut rng = Rng::derive(11, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);

    let contenders = result.folded.iter().filter(|&&f| !f).count();
    let revealed = result.revealed.iter().filter(|&&r| r).count();
    assert_eq!(revealed, contenders, "AlwaysShow 下未棄牌者全部亮牌");
}

#[test]
fn 有人全下時所有未棄牌者一律亮牌() {
    let config = TableConfig::simple(1, 2);
    // 座位 1 籌碼極少，跟注即全下 → 產生 side pot 情境
    let setup = HandSetup {
        stacks: vec![c(200), c(3), c(200)],
        occupied: vec![true; 3],
        button: 0,
        small_blind_seat: Some(1),
        big_blind_seat: 2,
    };

    let mut rng = Rng::derive(21, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);

    let contenders = result.folded.iter().filter(|&&f| !f).count();
    let revealed = result.revealed.iter().filter(|&&r| r).count();
    assert_eq!(
        revealed, contenders,
        "有人全下時現實規則要求所有未棄牌者攤開底牌"
    );
    assert_hand_conserves(&setup, &result);
}

// ── 可重現性（核心規格 3.4）─────────────────────────────────────────────

#[test]
fn 相同_seed_產生逐事件一致的結果() {
    let config = TableConfig::simple(1, 2);
    let setup = setup_9max(200);

    let mut a = Rng::derive(777, 5, RngDomain::Deal);
    let first = play_hand(&config, &setup, &mut a, &mut CallingStation);

    let mut b = Rng::derive(777, 5, RngDomain::Deal);
    let second = play_hand(&config, &setup, &mut b, &mut CallingStation);

    assert_eq!(first.board, second.board, "公共牌必須一致");
    assert_eq!(first.hole_cards, second.hole_cards, "底牌必須一致");
    assert_eq!(first.events, second.events, "事件序列必須逐事件一致");
    assert_eq!(first.final_stacks, second.final_stacks);
}

#[test]
fn 不同_seed_產生不同牌局() {
    let config = TableConfig::simple(1, 2);
    let setup = setup_9max(200);

    let mut a = Rng::derive(1, 1, RngDomain::Deal);
    let first = play_hand(&config, &setup, &mut a, &mut CallingStation);
    let mut b = Rng::derive(2, 1, RngDomain::Deal);
    let second = play_hand(&config, &setup, &mut b, &mut CallingStation);

    assert_ne!(first.board, second.board);
}

// ── 跨情境守恆掃描 ──────────────────────────────────────────────────────

#[test]
fn 大量隨機牌局的籌碼守恆恆成立() {
    let config = TableConfig {
        ante: AnteConfig {
            mode: AnteMode::PerPlayer,
            amount: c(1),
        },
        straddle: StraddleConfig {
            seats: vec![3],
            amounts: vec![c(4)],
        },
        rake: RakeConfig {
            basis_points: 500,
            cap: c(60),
            no_flop_no_drop: true,
        },
        ..TableConfig::simple(1, 2)
    };

    for seed in 0..500u64 {
        // 讓籌碼深度不一，製造多層 side pot
        let mut setup = setup_9max(200);
        setup.stacks[1] = c(7 + seed % 40);
        setup.stacks[4] = c(15 + seed % 90);
        setup.stacks[6] = c(3 + seed % 25);

        let mut rng = Rng::derive(seed, seed, RngDomain::Deal);
        let result = play_hand(&config, &setup, &mut rng, &mut CallingStation);
        assert_hand_conserves(&setup, &result);
    }
}

#[test]
fn 全員棄牌時未跟注部分退還且不抽水() {
    let config = TableConfig {
        rake: RakeConfig {
            basis_points: 500,
            cap: c(60),
            no_flop_no_drop: true,
        },
        ..TableConfig::simple(1, 2)
    };
    let setup = setup_9max(200);

    let mut rng = Rng::derive(99, 1, RngDomain::Deal);
    let result = play_hand(&config, &setup, &mut rng, &mut FoldAllExcept(2));

    assert!(!result.flop_dealt, "全員棄牌應在翻前結束");
    assert_eq!(
        result.distribution.rake,
        Chips::ZERO,
        "noFlopNoDrop 開啟時翻前結束不抽水"
    );
    assert_hand_conserves(&setup, &result);
}
