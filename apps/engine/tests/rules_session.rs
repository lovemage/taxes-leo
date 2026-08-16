//! 破產離桌、自動補位、dead button／dead blind 與位置標籤的驗收測試。
//!
//! 對應 `德州撲克規則細則.md` 第九章 R15–R23，以及 8.4.1 的標籤規則。

use poker_engine::betting::Action;
use poker_engine::chips::Chips;
use poker_engine::hand::ActionProvider;
use poker_engine::strategy::DecisionView;
use poker_engine::position::{advance_big_blind, resolve, PositionLabel};
use poker_engine::session::{run_session, InstanceEnd, SessionConfig, MIN_PLAYERS};
use poker_engine::table::TableConfig;

use PositionLabel::{Bb, Btn, Co, Hj, Lj, Sb, Utg, Utg1, Utg2};

fn c(n: u64) -> Chips {
    Chips::new(n)
}

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

fn base_config(players: usize) -> SessionConfig {
    SessionConfig {
        table: TableConfig::simple(1, 2),
        players,
        starting_stacks: vec![c(400); players],
        auto_refill: None,
        hero_seat: 0,
        hand_limit: 50,
        master_seed: 12345,
    }
}

// ── R15 ─────────────────────────────────────────────────────────────────
// SB 位玩家於前一手破產離桌 → 該手 dead small blind，不收 SB；
// BB 位仍前進一個在桌玩家。

#[test]
fn r15_sb_座位無人時為_dead_small_blind() {
    // 9 座，座位 1 已離桌。BB 在座位 2 → SB 位為座位 1（無人）
    let mut occupied = vec![true; 9];
    occupied[1] = false;

    let p = resolve(&occupied, 2);
    assert!(p.dead_small_blind, "SB 座位無人時必須為 dead small blind");
    assert_eq!(p.small_blind_seat, None, "該手不收 SB");
    assert!(!p.dead_button, "座位 0 仍有人，按鈕不為 dead");
    assert_eq!(p.button, 0);
    assert_eq!(
        p.labels.iter().filter(|l| **l == Some(Sb)).count(),
        0,
        "dead small blind 時無任何玩家標 SB"
    );
    assert_eq!(p.labels[2], Some(Bb), "BB 標籤恆存在");
}

// ── R16 ─────────────────────────────────────────────────────────────────
// 按鈕位玩家於前一手破產離桌 → 該手 dead button。

#[test]
fn r16_按鈕座位無人時為_dead_button() {
    let mut occupied = vec![true; 9];
    occupied[0] = false; // 按鈕位無人

    let p = resolve(&occupied, 2);
    assert!(p.dead_button, "按鈕座位無人時必須為 dead button");
    assert_eq!(p.button, 0, "按鈕位置仍為座位 0，只是無人持有");
    assert_eq!(
        p.labels.iter().filter(|l| **l == Some(Btn)).count(),
        0,
        "dead button 時無任何玩家標 BTN"
    );
    assert_eq!(p.small_blind_seat, Some(1), "SB 不受影響");

    // 規則細則 8.4.1：dead button 使 k 增加，7 人在桌時 k=5
    let in_order: Vec<PositionLabel> = (1..=9)
        .map(|offset| (2 + offset) % 9)
        .filter_map(|seat| p.labels[seat])
        .collect();
    assert_eq!(in_order, vec![Utg, Utg1, Utg2, Lj, Hj, Co, Sb, Bb]);
}

// ── R17 ─────────────────────────────────────────────────────────────────
// 連續多人離桌 → 無任何玩家連續兩手付 BB；BB 不被跳過。

#[test]
fn r17_bb_嚴格前進且不重複落在同一玩家() {
    let mut occupied = vec![true; 9];
    occupied[3] = false;
    occupied[4] = false;
    occupied[5] = false;

    let mut bb = 2;
    let mut seen = Vec::new();
    for _ in 0..6 {
        bb = advance_big_blind(&occupied, bb);
        seen.push(bb);
    }
    // 座位 3、4、5 已離桌，BB 應直接跳到 6
    assert_eq!(seen[0], 6, "BB 前進到下一個在桌玩家，略過空位");

    for pair in seen.windows(2) {
        assert_ne!(pair[0], pair[1], "同一玩家不得連續兩手付 BB");
    }
}

#[test]
fn r17_六人在桌時每人依序輪到_bb() {
    let occupied = vec![true; 6];
    let mut bb = 2;
    let mut order = vec![bb];
    for _ in 0..5 {
        bb = advance_big_blind(&occupied, bb);
        order.push(bb);
    }
    order.sort_unstable();
    order.dedup();
    assert_eq!(order.len(), 6, "六手之內每個座位各付一次 BB，無人被跳過");
}

// ── R18 ─────────────────────────────────────────────────────────────────
// 自動補位關閉，在桌人數降到 6 以下 → 桌次立即結束，不得打成 5-max。

#[test]
fn r18_補位關閉時人數不足六人即結束桌次() {
    let mut config = base_config(6);
    // 讓短碼快速破產
    config.starting_stacks = vec![c(400), c(6), c(6), c(400), c(400), c(400)];
    config.auto_refill = None;
    config.hand_limit = 200;

    let mut min_seated = usize::MAX;
    let summary = run_session(&config, &mut CallingStation, |played| {
        min_seated = min_seated.min(played.seated);
    });

    assert!(
        min_seated >= MIN_PLAYERS,
        "任何一手開始時在桌人數必須 ≥ 6，實際最低 {min_seated}"
    );
    assert!(
        summary
            .instances
            .iter()
            .any(|i| i.end == InstanceEnd::NotEnoughPlayers),
        "應有桌次因人數不足而結束"
    );
}

// ── R19 ─────────────────────────────────────────────────────────────────
// 使用者座位破產 → 桌次立即結束，即使 Bot 尚有籌碼。

#[test]
fn r19_使用者破產即結束桌次() {
    let mut config = base_config(6);
    config.hero_seat = 1;
    config.starting_stacks = vec![c(400), c(4), c(400), c(400), c(400), c(400)];
    config.auto_refill = Some(6);
    config.hand_limit = 300;

    let summary = run_session(&config, &mut CallingStation, |_| {});

    assert!(
        summary
            .instances
            .iter()
            .any(|i| i.end == InstanceEnd::HeroBusted),
        "使用者破產時桌次必須結束"
    );
}

// ── R20 ─────────────────────────────────────────────────────────────────
// 桌次全生命週期的籌碼守恆。

#[test]
fn r20_桌次全生命週期的籌碼守恆() {
    let mut config = base_config(9);
    config.starting_stacks = vec![
        c(400),
        c(30),
        c(400),
        c(80),
        c(400),
        c(15),
        c(400),
        c(400),
        c(400),
    ];
    config.auto_refill = Some(7);
    config.hand_limit = 300;

    // 逐手驗守恆：每手的結束總額 + rake 必須等於開始總額
    run_session(&config, &mut CallingStation, |played| {
        let before: Chips = played.result.total_contributions.iter().copied().sum();
        let payouts: Chips = played.result.distribution.payouts.iter().copied().sum();
        let refunds: Chips = played.result.distribution.refunds.iter().copied().sum();
        assert_eq!(
            before,
            payouts + refunds + played.result.distribution.rake,
            "第 {} 手守恆破壞",
            played.hand_index
        );
    });
}

// ── R21 / R22 ───────────────────────────────────────────────────────────
// 自動補位：Bot 離桌後於下一手前補入，人數回到 targetPlayers。

#[test]
fn r21_自動補位使人數回到目標且事件寫入摘要() {
    let mut config = base_config(9);
    config.starting_stacks = vec![
        c(400),
        c(5),
        c(5),
        c(400),
        c(400),
        c(400),
        c(400),
        c(400),
        c(400),
    ];
    config.auto_refill = Some(9);
    config.hand_limit = 120;

    let mut seen_seated = Vec::new();
    let summary = run_session(&config, &mut CallingStation, |played| {
        seen_seated.push(played.seated);
    });

    let refill_count: usize = summary.instances.iter().map(|i| i.refills.len()).sum();
    assert!(refill_count > 0, "短碼破產後應觸發補位");
    assert!(
        seen_seated.iter().all(|&s| s >= MIN_PLAYERS),
        "補位開啟時每手人數都不得低於下限"
    );

    for instance in &summary.instances {
        for refill in &instance.refills {
            assert_eq!(
                refill.buy_in, config.starting_stacks[refill.seat],
                "補位者以該座設定的起始深度買入"
            );
        }
    }
}

#[test]
fn r22_同手多人破產時一次補足至目標人數() {
    let mut config = base_config(9);
    // 三個座位同時極短，容易在同一手一起破產
    config.starting_stacks = vec![
        c(400),
        c(3),
        c(3),
        c(3),
        c(400),
        c(400),
        c(400),
        c(400),
        c(400),
    ];
    config.auto_refill = Some(9);
    config.hand_limit = 60;

    run_session(&config, &mut CallingStation, |played| {
        assert_eq!(
            played.seated, 9,
            "補位目標為 9 時，每手開始都應是滿桌"
        );
    });
}

// ── R23 ─────────────────────────────────────────────────────────────────
// 補位後重播：依 log 還原補位時點與新 Bot 設定，逐事件與原執行一致。

#[test]
fn r23_含補位的_run_可完整重現() {
    let mut config = base_config(9);
    config.starting_stacks = vec![
        c(400),
        c(8),
        c(400),
        c(12),
        c(400),
        c(400),
        c(400),
        c(400),
        c(400),
    ];
    config.auto_refill = Some(8);
    config.hand_limit = 150;

    let mut first_boards = Vec::new();
    let first = run_session(&config, &mut CallingStation, |p| {
        first_boards.push((p.hand_index, p.result.board.clone(), p.seated));
    });

    let mut second_boards = Vec::new();
    let second = run_session(&config, &mut CallingStation, |p| {
        second_boards.push((p.hand_index, p.result.board.clone(), p.seated));
    });

    assert_eq!(first.hands_played, second.hands_played);
    assert_eq!(first_boards, second_boards, "逐手公共牌與人數必須一致");
    for (a, b) in first.instances.iter().zip(&second.instances) {
        assert_eq!(a.hands, b.hands, "桌次存活手數必須一致");
        assert_eq!(a.end, b.end, "桌次結束原因必須一致");
        assert_eq!(a.refills, b.refills, "補位事件必須逐筆一致");
    }
}

// ── 座位數不變量（核心規格 2.1）────────────────────────────────────────

#[test]
fn 任一手開始時在桌人數恆不低於六人() {
    for players in MIN_PLAYERS..=9 {
        for refill in [None, Some(MIN_PLAYERS), Some(players)] {
            let mut config = base_config(players);
            config.auto_refill = refill;
            config.hand_limit = 80;
            // 讓多數座位短碼，逼出破產與補位路徑
            config.starting_stacks = (0..players)
                .map(|seat| if seat % 2 == 0 { c(400) } else { c(9) })
                .collect();

            run_session(&config, &mut CallingStation, |played| {
                assert!(
                    played.seated >= MIN_PLAYERS,
                    "players={players} refill={refill:?} 出現 {} 人的手牌",
                    played.seated
                );
            });
        }
    }
}

#[test]
fn 設定驗證攔下越界值() {
    let mut config = base_config(6);
    config.players = 5;
    config.starting_stacks = vec![c(400); 5];
    assert!(config.validate().is_err(), "6-max 以下不得通過驗證");

    let mut config = base_config(9);
    config.auto_refill = Some(5);
    assert!(config.validate().is_err(), "補位目標低於 6 不得通過");

    let mut config = base_config(9);
    config.auto_refill = Some(10);
    assert!(config.validate().is_err(), "補位目標不得大於開桌人數");
}

// ── 位置標籤（8.4.1）────────────────────────────────────────────────────

#[test]
fn 標籤在有_dead_位時仍每人唯一() {
    for missing in [vec![0], vec![1], vec![0, 1], vec![4], vec![0, 4]] {
        let mut occupied = vec![true; 9];
        for &seat in &missing {
            occupied[seat] = false;
        }
        // BB 位必須有人
        let bb = (2..9).find(|&s| occupied[s]).expect("必有在座者");
        let p = resolve(&occupied, bb);

        let labels: Vec<PositionLabel> = p.labels.iter().flatten().copied().collect();
        let seated = occupied.iter().filter(|&&o| o).count();
        assert_eq!(labels.len(), seated, "每位在桌玩家都必須取得標籤");

        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "標籤不得重複：{labels:?}");
    }
}
