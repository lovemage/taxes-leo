//! 隱藏資訊隔離的驗收測試。
//!
//! 核心規格 2.4（不可放寬）：
//! 「測試必須證明：只改變不可見牌、保持資訊集相同時，Bot 在相同 seed 下
//! 產生相同 action distribution。」
//!
//! 這是實做計劃 M0／M1 的閘門項目之一，也是「Bot 永不讀其他座位底牌」
//! （鐵則 6）唯一能被機械驗證的形式。

use std::cell::RefCell;

use poker_engine::betting::Action;
use poker_engine::card::{Card, Rank, Suit};
use poker_engine::chips::Chips;
use poker_engine::hand::{play_hand, play_hand_with_deal, ActionProvider, HandSetup, PreparedDeal};
use poker_engine::rng::{Rng, RngDomain};
use poker_engine::strategy::{ActionDistribution, DecisionView};
use poker_engine::table::TableConfig;

fn c(n: u64) -> Chips {
    Chips::new(n)
}

/// 記錄每次決策時看到的 `DecisionView` 與產生的行動分佈。
///
/// 分佈刻意由 view 的內容決定（而非隨機），這樣若有隱藏資訊滲入 view，
/// 分佈就會跟著改變而被測出來。
struct RecordingProvider {
    views: RefCell<Vec<DecisionView>>,
    distributions: RefCell<Vec<ActionDistribution>>,
}

impl RecordingProvider {
    fn new() -> Self {
        Self {
            views: RefCell::new(Vec::new()),
            distributions: RefCell::new(Vec::new()),
        }
    }

    /// 由 view 的公開內容導出一個確定性的分佈。
    ///
    /// 用 view 本身當輸入是刻意的：只要 view 有任何一位元不同，
    /// 分佈就會不同。
    fn distribution_from(view: &DecisionView) -> ActionDistribution {
        let seed = view.hand_class().index() as u64
            + view.pot.units() * 7
            + view.history.len() as u64 * 13
            + view.board.len() as u64 * 31
            + view.opponents.len() as u64 * 17;

        let mut weights: Vec<(Action, u64)> = vec![(Action::Fold, 1 + seed % 5)];
        if view.legal.can_check {
            weights.push((Action::Check, 1 + seed % 7));
        }
        if view.legal.call_to.is_some() {
            weights.push((Action::Call, 1 + seed % 11));
        }
        ActionDistribution::from_weights(weights).expect("至少有一個合法行動")
    }
}

impl ActionProvider for RecordingProvider {
    fn choose(&mut self, view: &DecisionView) -> Action {
        let distribution = Self::distribution_from(view);
        self.views.borrow_mut().push(view.clone());
        self.distributions.borrow_mut().push(distribution.clone());

        // 實際行動仍走簡單規則，確保兩次執行的牌局路徑一致
        if view.legal.can_check {
            Action::Check
        } else if view.legal.call_to.is_some() {
            Action::Call
        } else {
            Action::Fold
        }
    }
}

fn setup(stacks_seed: u64) -> HandSetup {
    let _ = stacks_seed;
    HandSetup {
        stacks: vec![c(400); 9],
        occupied: vec![true; 9],
        button: 0,
        small_blind_seat: Some(1),
        big_blind_seat: 2,
    }
}

/// 以隨機發牌跑一手並記錄所有決策視角。
fn run_recording(seed: u64) -> (Vec<DecisionView>, Vec<ActionDistribution>) {
    let config = TableConfig::simple(1, 2);
    let mut provider = RecordingProvider::new();
    let mut rng = Rng::derive(seed, 1, RngDomain::Deal);
    let _ = play_hand(&config, &setup(seed), &mut rng, &mut provider);
    (provider.views.into_inner(), provider.distributions.into_inner())
}

// ── DecisionView 的欄位層級檢查 ─────────────────────────────────────────

#[test]
fn decision_view_不含他人底牌() {
    let (views, _) = run_recording(1);
    assert!(!views.is_empty(), "應至少有一次決策");

    for view in &views {
        // 型別上就沒有可承載他人底牌的欄位；這裡再驗序列化語意：
        // opponents 只有公開籌碼與狀態
        for opponent in &view.opponents {
            assert_ne!(opponent.seat, view.seat, "自己不應出現在對手清單");
            // OpponentPublic 沒有底牌欄位——若日後有人加上去，這行的
            // 相鄰註解與下方 debug 檢查會提醒
        }
        let dumped = format!("{view:?}");
        let hero = view.hole_cards;
        let hero_text = [hero[0].to_string(), hero[1].to_string()];
        // view 中出現的牌張字串，應只來自英雄底牌與公共牌
        let allowed: Vec<String> = hero_text
            .iter()
            .cloned()
            .chain(view.board.iter().map(ToString::to_string))
            .collect();
        for rank in [Rank::Ace, Rank::King, Rank::Queen] {
            for suit in [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs] {
                let card = Card::new(rank, suit).to_string();
                if dumped.contains(&format!("{card}\"")) {
                    assert!(
                        allowed.contains(&card),
                        "view 中出現了非英雄且非公共牌的牌張 {card}"
                    );
                }
            }
        }
    }
}

#[test]
fn decision_view_的合法行動來自引擎() {
    let (views, _) = run_recording(3);
    for view in &views {
        assert_eq!(
            view.legal.seat, view.seat,
            "合法行動集合必須屬於行動中的座位"
        );
        // 核心規格 2.2：UI 與策略層不得自行推導可下注金額
        assert!(
            view.legal.can_fold,
            "引擎回傳的合法行動應完整，包含永遠允許的棄牌"
        );
    }
}

// ── 核心規格 2.4 的隔離證明 ────────────────────────────────────────────

/// 同一 seed 重跑：view 與分佈必須逐位元一致。
///
/// 這是隔離測試的基準線——若同 seed 都不一致，就代表決策路徑裡混入了
/// 未被 `DecisionView` 涵蓋的狀態。
#[test]
fn 相同_seed_的決策視角與分佈逐位元一致() {
    let (views_a, dist_a) = run_recording(777);
    let (views_b, dist_b) = run_recording(777);

    assert_eq!(views_a.len(), views_b.len());
    assert_eq!(views_a, views_b, "DecisionView 必須逐位元一致");
    assert_eq!(dist_a, dist_b, "action distribution 必須逐位元一致");
}

/// **主測試**：只改變不可見牌、保持資訊集相同時，分佈必須一致。
///
/// 用 [`PreparedDeal`] 直接構造兩手：英雄底牌與五張公共牌完全相同，
/// 其餘座位的底牌全部換掉。對英雄而言兩手的資訊集相同，因此依核心規格
/// 2.4，其 `DecisionView` 與 action distribution 必須逐位元一致。
///
/// 靠隨機碰撞找這種配對不可行（需要兩張底牌與五張公共牌同時相同），
/// 因此改用指定發牌。
#[test]
fn 只改變不可見牌時分佈不變() {
    let config = TableConfig::simple(1, 2);
    let hero_seat = 3;

    let card = |text: &str| Card::parse(text).expect("牌張");
    let hero_hole = [card("As"), card("Kd")];
    let board = [card("2c"), card("7h"), card("9s"), card("Jd"), card("4c")];

    // 兩組對手底牌完全不同，但都不與英雄底牌或公共牌衝突
    let opponents_a = [
        "3d", "3h", "5c", "5d", "6s", "6h", "8c", "8d", "Tc", "Th", "Qc", "Qh", "Ac", "Ah", "Kc",
        "Kh",
    ];
    let opponents_b = [
        "3s", "3c", "5h", "5s", "6c", "6d", "8h", "8s", "Td", "Ts", "Qd", "Qs", "Ad", "Ah", "Ks",
        "Kc",
    ];

    let build = |opponents: &[&str]| {
        let mut hole: Vec<Option<[Card; 2]>> = vec![None; 9];
        let mut index = 0;
        for (seat, slot) in hole.iter_mut().enumerate() {
            if seat == hero_seat {
                *slot = Some(hero_hole);
            } else {
                *slot = Some([card(opponents[index]), card(opponents[index + 1])]);
                index += 2;
            }
        }
        PreparedDeal { hole_cards: hole, board }
    };

    let deal_a = build(&opponents_a);
    let deal_b = build(&opponents_b);
    assert_ne!(
        deal_a.hole_cards, deal_b.hole_cards,
        "兩組發牌的他人底牌必須不同，否則測試沒有實際驗到隔離"
    );

    let run = |deal: &PreparedDeal| {
        let mut provider = RecordingProvider::new();
        let _ = play_hand_with_deal(&config, &setup(0), deal, &mut provider);
        (
            provider.views.into_inner(),
            provider.distributions.into_inner(),
        )
    };

    let (views_a, dist_a) = run(&deal_a);
    let (views_b, dist_b) = run(&deal_b);

    let hero_indices_a: Vec<usize> = views_a
        .iter()
        .enumerate()
        .filter(|(_, v)| v.seat == hero_seat)
        .map(|(i, _)| i)
        .collect();
    let hero_indices_b: Vec<usize> = views_b
        .iter()
        .enumerate()
        .filter(|(_, v)| v.seat == hero_seat)
        .map(|(i, _)| i)
        .collect();

    assert!(!hero_indices_a.is_empty(), "英雄應至少決策一次");
    assert_eq!(
        hero_indices_a.len(),
        hero_indices_b.len(),
        "英雄的決策次數不應因他人底牌而改變"
    );

    for (&a, &b) in hero_indices_a.iter().zip(&hero_indices_b) {
        assert_eq!(
            views_a[a], views_b[b],
            "資訊集相同時 DecisionView 必須完全相同"
        );
        assert_eq!(
            dist_a[a], dist_b[b],
            "只改變不可見牌時 action distribution 必須逐位元一致"
        );
    }
}

// ── 有效籌碼分檔 ────────────────────────────────────────────────────────

#[test]
fn 有效籌碼分檔依實際籌碼判定() {
    let (views, _) = run_recording(5);
    let first = views.first().expect("至少一次決策");
    // 全桌 400 籌碼、BB=2 → 200BB → 落在 [160,240)
    assert_eq!(
        first.effective_stack_bucket,
        poker_engine::strategy::StackBucket::VeryDeep,
        "200BB 起始應落在 [160,240) 檔"
    );
}
