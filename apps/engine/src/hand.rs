//! 一手牌的完整循環：發牌 → 強制下注 → 逐街下注 → 攤牌 → 結算。
//!
//! 規格來源：`德州撲克規則細則.md` 第一～六章、核心規格 2.x。
//!
//! 本模組是牌局的唯一權威。行動由 [`ActionProvider`] 提供，但**合法性一律
//! 由引擎判定**——provider 交回的行動若不合法即 panic，不做寬容修正，
//! 免得錯誤策略靜默產生不合法牌局（核心規格 2.2）。

use crate::betting::{Action, BettingRound, LegalActions, SeatState};
use crate::card::Card;
use crate::chips::Chips;
use crate::eval::{evaluate, HandRank};
use crate::pot::{settle, Distribution};
use crate::position::{resolve, PositionLabel};
use crate::rng::Rng;
use crate::strategy::decision::{OpponentPublic, PublicAction, StackBucket};
use crate::strategy::DecisionView;
use crate::table::{AnteMode, MuckPolicy, TableConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Street {
    Preflop,
    Flop,
    Turn,
    River,
}

/// 一手牌的座位配置。
///
/// 位置由呼叫端（桌次層）依規則細則 8.4 算好後傳入，包含 dead button 與
/// dead small blind 的情形，因此這裡不重新推導位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandSetup {
    pub stacks: Vec<Chips>,
    /// 該座位是否有玩家。dead button 時按鈕位置為 `false`
    pub occupied: Vec<bool>,
    /// 按鈕**位置**索引（可能無人）
    pub button: usize,
    /// SB 位的玩家；`None` 表示 dead small blind，該手不收 SB
    pub small_blind_seat: Option<usize>,
    /// BB 位的玩家。此位恆存在（BB 位定義為下一個在桌玩家）
    pub big_blind_seat: usize,
}

/// 行動來源。
///
/// 核心規格 2.4：決策端**只能**收到 [`DecisionView`]，不得接觸完整
/// `GameState`。這個簽章就是該約束的落實——實作者拿不到牌堆與他人底牌，
/// 因為那些資訊在 `DecisionView` 的型別上不存在。
pub trait ActionProvider {
    fn choose(&mut self, view: &DecisionView) -> Action;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandEvent {
    PostAnte { seat: usize, amount: Chips },
    PostSmallBlind { seat: usize, amount: Chips },
    PostBigBlind { seat: usize, amount: Chips },
    PostStraddle { seat: usize, amount: Chips },
    Acted { street: Street, seat: usize, action: Action, committed_to: Chips },
    BoardDealt { street: Street, cards: Vec<Card> },
    Revealed { seat: usize, rank: HandRank },
    Mucked { seat: usize },
}

#[derive(Debug, Clone)]
pub struct HandResult {
    /// 每座本手總投入（ante + 各街投入）
    pub total_contributions: Vec<Chips>,
    pub final_stacks: Vec<Chips>,
    pub distribution: Distribution,
    pub board: Vec<Card>,
    pub hole_cards: Vec<Option<[Card; 2]>>,
    /// 依 muck 政策實際亮牌者。`DecisionView` 與對手模型只能看到這些底牌
    pub revealed: Vec<bool>,
    pub folded: Vec<bool>,
    pub flop_dealt: bool,
    pub events: Vec<HandEvent>,
}

/// 預先指定的發牌。
///
/// 供測試構造確定情境使用——例如核心規格 2.4 要求的隔離測試，需要
/// 「英雄底牌與公共牌相同、但他人底牌不同」的兩手，靠隨機碰撞不可行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDeal {
    /// 逐座底牌；未入座者為 `None`
    pub hole_cards: Vec<Option<[Card; 2]>>,
    /// 五張公共牌，依 flop/turn/river 順序取用
    pub board: [Card; 5],
}

/// 打完一手牌，牌由 `rng` 洗出。
///
/// # Panics
/// `provider` 交回不合法行動時 panic。
#[must_use]
pub fn play_hand(
    config: &TableConfig,
    setup: &HandSetup,
    rng: &mut Rng,
    provider: &mut dyn ActionProvider,
) -> HandResult {
    // 規則細則第七章：不實作 burn card（牌堆均勻隨機時與燒牌等價）
    let mut deck = crate::card::full_deck();
    rng.shuffle(&mut deck);
    let mut next_card = 0usize;

    let mut hole_cards: Vec<Option<[Card; 2]>> = vec![None; setup.stacks.len()];
    for (seat, slot) in hole_cards.iter_mut().enumerate() {
        if setup.occupied[seat] {
            *slot = Some([deck[next_card], deck[next_card + 1]]);
            next_card += 2;
        }
    }
    let board = [
        deck[next_card],
        deck[next_card + 1],
        deck[next_card + 2],
        deck[next_card + 3],
        deck[next_card + 4],
    ];

    play_hand_with_deal(config, setup, &PreparedDeal { hole_cards, board }, provider)
}

/// 打完一手牌，使用指定的發牌。
///
/// # Panics
/// `provider` 交回不合法行動，或 `deal` 的座位數與 `setup` 不符時 panic。
#[must_use]
pub fn play_hand_with_deal(
    config: &TableConfig,
    setup: &HandSetup,
    deal: &PreparedDeal,
    provider: &mut dyn ActionProvider,
) -> HandResult {
    let n = setup.stacks.len();
    assert_eq!(deal.hole_cards.len(), n, "發牌的座位數與桌型不符");
    let mut stacks = setup.stacks.clone();
    let mut contributions = vec![Chips::ZERO; n];
    let mut events = Vec::new();

    let hole_cards = deal.hole_cards.clone();
    let mut next_board = 0usize;

    // ── 強制下注 ────────────────────────────────────────────────────
    // 規則細則 2.2：ante 為 dead money，不計入當街投入，因此與盲注分開累計
    post_antes(config, setup, &mut stacks, &mut contributions, &mut events);

    let mut street_committed = vec![Chips::ZERO; n];
    post_blinds_and_straddles(
        config,
        setup,
        &mut stacks,
        &mut street_committed,
        &mut events,
    );

    let mut state: Vec<SeatState> = (0..n)
        .map(|seat| {
            if !setup.occupied[seat] {
                SeatState::Folded
            } else if stacks[seat].is_zero() {
                SeatState::AllIn
            } else {
                SeatState::Active
            }
        })
        .collect();

    // ── 翻前 ────────────────────────────────────────────────────────
    // 規則細則 2.3：最大 straddle 取得 option，行動從其左邊第一位起
    let (option_seat, largest_forced) = match config.straddle.largest() {
        Some((seat, amount)) if setup.occupied[seat] => (seat, amount.max(config.big_blind)),
        _ => (setup.big_blind_seat, config.big_blind),
    };
    let first_to_act = next_occupied(setup, option_seat);

    let labels = resolve(&setup.occupied, setup.big_blind_seat).labels;
    let mut history: Vec<PublicAction> = Vec::new();
    let seated = setup.occupied.iter().filter(|&&o| o).count();
    // ante 已進底池但不屬當街投入，因此列入先前底池
    let ante_pot: Chips = contributions.iter().copied().sum();

    let mut round = BettingRound::new_preflop(
        stacks.clone(),
        state.clone(),
        street_committed.clone(),
        first_to_act,
        largest_forced,
    );
    let mut final_aggressor = {
        let mut ctx = ViewContext {
            labels: &labels,
            hole_cards: &hole_cards,
            board: &[],
            prior_pot: ante_pot,
            seated,
            big_blind: config.big_blind,
            history: &mut history,
        };
        run_round(&mut round, Street::Preflop, provider, &mut events, &mut ctx)
    };
    absorb(&round, &mut stacks, &mut state, &mut contributions, n);

    // ── 翻後各街 ────────────────────────────────────────────────────
    let mut board: Vec<Card> = Vec::with_capacity(5);
    let mut flop_dealt = false;

    for street in [Street::Flop, Street::Turn, Street::River] {
        if contenders(&state) < 2 {
            break;
        }
        let count = if street == Street::Flop { 3 } else { 1 };
        let dealt: Vec<Card> = deal.board[next_board..next_board + count].to_vec();
        next_board += count;
        board.extend_from_slice(&dealt);
        if street == Street::Flop {
            flop_dealt = true;
        }
        events.push(HandEvent::BoardDealt {
            street,
            cards: dealt,
        });

        // 規則細則 1.5：僅剩一名未全下玩家時不再下注，公共牌直接發完
        if actives(&state) < 2 {
            continue;
        }

        let first = next_active_from(&state, setup.button);
        let mut round =
            BettingRound::new_postflop(stacks.clone(), state.clone(), first, config.big_blind);
        let aggressor = {
            let prior_pot: Chips = contributions.iter().copied().sum();
            let mut ctx = ViewContext {
                labels: &labels,
                hole_cards: &hole_cards,
                board: &board,
                prior_pot,
                seated,
                big_blind: config.big_blind,
                history: &mut history,
            };
            run_round(&mut round, street, provider, &mut events, &mut ctx)
        };
        final_aggressor = aggressor;
        absorb(&round, &mut stacks, &mut state, &mut contributions, n);
    }

    // ── 攤牌 ────────────────────────────────────────────────────────
    let folded: Vec<bool> = state
        .iter()
        .map(|s| matches!(s, SeatState::Folded))
        .collect();

    let ranks: Vec<Option<u32>> = (0..n)
        .map(|seat| {
            if folded[seat] || board.len() < 5 {
                return None;
            }
            let hole = hole_cards[seat]?;
            let mut cards = board.clone();
            cards.extend_from_slice(&hole);
            Some(evaluate(&cards).value())
        })
        .collect();

    let revealed = resolve_showdown(
        config,
        setup,
        &state,
        &ranks,
        final_aggressor,
        &mut events,
    );

    // ── 結算 ────────────────────────────────────────────────────────
    // 只剩一名未棄牌者時無需攤牌，該座直接取得底池
    let allocation_ranks: Vec<Option<u32>> = if contenders(&state) < 2 {
        (0..n)
            .map(|seat| (!folded[seat]).then_some(0u32))
            .collect()
    } else {
        ranks.clone()
    };

    let distribution = settle(
        &contributions,
        &folded,
        &allocation_ranks,
        setup.button,
        config.rake,
        flop_dealt,
    );

    let final_stacks: Vec<Chips> = (0..n)
        .map(|seat| stacks[seat] + distribution.payouts[seat] + distribution.refunds[seat])
        .collect();

    HandResult {
        total_contributions: contributions,
        final_stacks,
        distribution,
        board,
        hole_cards,
        revealed,
        folded,
        flop_dealt,
        events,
    }
}

/// 收取 ante。
///
/// 規則細則 2.2：ante 為 dead money，不計入當街投入，因此不影響任何人的
/// call 金額或最小加注額；籌碼不足者全下付 ante 仍然發牌。
/// `bbAnte`／`btnAnte` 的付款順序為 **ante 先、blind 後**，本函式在
/// 收盲注之前呼叫即滿足。
fn post_antes(
    config: &TableConfig,
    setup: &HandSetup,
    stacks: &mut [Chips],
    contributions: &mut [Chips],
    events: &mut Vec<HandEvent>,
) {
    let amount = config.ante.amount;
    if amount.is_zero() || config.ante.mode == AnteMode::None {
        return;
    }
    let occupied_count = u64::try_from(setup.occupied.iter().filter(|&&o| o).count())
        .expect("座位數必在 u64 範圍");
    let table_total = Chips::new(amount.units() * occupied_count);

    let mut pay = |seat: usize, want: Chips, stacks: &mut [Chips], events: &mut Vec<HandEvent>| {
        let paid = want.min_of(stacks[seat]);
        if paid.is_zero() {
            return;
        }
        stacks[seat] -= paid;
        contributions[seat] += paid;
        events.push(HandEvent::PostAnte { seat, amount: paid });
    };

    match config.ante.mode {
        AnteMode::None => {}
        AnteMode::PerPlayer => {
            for seat in 0..stacks.len() {
                if setup.occupied[seat] {
                    pay(seat, amount, stacks, events);
                }
            }
        }
        AnteMode::BbAnte => pay(setup.big_blind_seat, table_total, stacks, events),
        AnteMode::BtnAnte => {
            // dead button 時按鈕位置無人，該手不收 btnAnte
            if setup.occupied[setup.button] {
                pay(setup.button, table_total, stacks, events);
            }
        }
    }
}

/// 收取盲注與 straddle。
///
/// 籌碼不足者全下付出可付部分；**當前下注額仍以名目金額計**，
/// 因此其他玩家仍須跟到完整的 BB／straddle。
fn post_blinds_and_straddles(
    config: &TableConfig,
    setup: &HandSetup,
    stacks: &mut [Chips],
    committed: &mut [Chips],
    events: &mut Vec<HandEvent>,
) {
    if let Some(sb) = setup.small_blind_seat {
        let paid = config.small_blind.min_of(stacks[sb]);
        stacks[sb] -= paid;
        committed[sb] += paid;
        events.push(HandEvent::PostSmallBlind {
            seat: sb,
            amount: paid,
        });
    }

    let bb = setup.big_blind_seat;
    let paid = config.big_blind.min_of(stacks[bb]);
    stacks[bb] -= paid;
    committed[bb] += paid;
    events.push(HandEvent::PostBigBlind {
        seat: bb,
        amount: paid,
    });

    for (&seat, &amount) in config.straddle.seats.iter().zip(&config.straddle.amounts) {
        // straddle 須全額投入才成立，籌碼不足即不視為 straddle
        if setup.occupied[seat] && stacks[seat] >= amount {
            stacks[seat] -= amount;
            committed[seat] += amount;
            events.push(HandEvent::PostStraddle { seat, amount });
        }
    }
}

/// 建構 `DecisionView` 所需的脈絡。
///
/// 刻意只收公開資訊：這個結構裡沒有牌堆，也沒有他人底牌。
struct ViewContext<'a> {
    labels: &'a [Option<PositionLabel>],
    hole_cards: &'a [Option<[Card; 2]>],
    board: &'a [Card],
    /// 先前各街已進入底池的總額
    prior_pot: Chips,
    seated: usize,
    big_blind: Chips,
    history: &'a mut Vec<PublicAction>,
}

/// 由引擎內部狀態萃取該座位可見的資訊（核心規格 2.4）。
///
/// **這是隱藏資訊的唯一出口**。`hole_cards` 雖是完整陣列，但只取用
/// `seat` 自己的那一筆；其餘座位只轉出公開籌碼與狀態。
fn build_view(
    round: &BettingRound,
    seat: usize,
    street: Street,
    legal: &LegalActions,
    ctx: &ViewContext<'_>,
) -> DecisionView {
    let committed_this_street: Chips = ctx
        .hole_cards
        .iter()
        .enumerate()
        .map(|(s, _)| round.committed(s))
        .sum();
    let pot = ctx.prior_pot + committed_this_street;

    let opponents: Vec<OpponentPublic> = (0..ctx.hole_cards.len())
        .filter(|&s| s != seat && ctx.hole_cards[s].is_some())
        .map(|s| OpponentPublic {
            seat: s,
            position: ctx.labels[s].unwrap_or(PositionLabel::Bb),
            stack: round.stack(s),
            committed: round.committed(s),
            folded: matches!(round.state(s), SeatState::Folded),
            all_in: matches!(round.state(s), SeatState::AllIn),
        })
        .collect();

    // 有效籌碼＝英雄與仍在牌局對手中的較小者，換算為百分之一 BB 後判檔
    // （規則細則 8.5：不得先取整再判定）
    let hero_effective = round.stack(seat) + round.committed(seat);
    let opponent_max = opponents
        .iter()
        .filter(|o| !o.folded)
        .map(|o| o.stack + o.committed)
        .max()
        .unwrap_or(hero_effective);
    let effective = hero_effective.min_of(opponent_max);
    let centi_bb = effective.units() * 100 / ctx.big_blind.units().max(1);

    DecisionView {
        seat,
        position: ctx.labels[seat].unwrap_or(PositionLabel::Bb),
        street,
        hole_cards: ctx.hole_cards[seat].expect("行動者必然已發牌"),
        board: ctx.board.to_vec(),
        seated: ctx.seated,
        effective_stack_bucket: StackBucket::from_centi_bb(centi_bb),
        pot,
        to_call: legal.call_to.unwrap_or(round.current_bet()).saturating_sub(round.committed(seat)),
        big_blind: ctx.big_blind,
        legal: legal.clone(),
        history: ctx.history.clone(),
        opponents,
    }
}

/// 跑完一條街的下注，回傳本街最後一位主動下注／加注者。
fn run_round(
    round: &mut BettingRound,
    street: Street,
    provider: &mut dyn ActionProvider,
    events: &mut Vec<HandEvent>,
    ctx: &mut ViewContext<'_>,
) -> Option<usize> {
    let mut aggressor = None;
    while let Some(legal) = round.legal_actions() {
        let seat = legal.seat;
        let before = round.current_bet();
        let view = build_view(round, seat, street, &legal, ctx);
        let action = provider.choose(&view);
        round
            .apply(action)
            .unwrap_or_else(|e| panic!("provider 在座位 {seat} 交回不合法行動 {action:?}：{e:?}"));
        // 先套用再記錄：「是不是加注」要看注額有沒有被推高，
        // 那是套用之後才知道的事
        let raised = round.current_bet() > before;
        ctx.history.push(PublicAction {
            street,
            seat,
            position: ctx.labels[seat].unwrap_or(PositionLabel::Bb),
            action,
            raised,
            committed_to: round.committed(seat),
        });
        if raised {
            aggressor = Some(seat);
        }
        events.push(HandEvent::Acted {
            street,
            seat,
            action,
            committed_to: round.committed(seat),
        });
    }
    aggressor
}

/// 把一條街的結果吸收回手層狀態。
fn absorb(
    round: &BettingRound,
    stacks: &mut [Chips],
    state: &mut [SeatState],
    contributions: &mut [Chips],
    n: usize,
) {
    for seat in 0..n {
        stacks[seat] = round.stack(seat);
        state[seat] = round.state(seat);
        contributions[seat] += round.committed(seat);
    }
}

/// 決定攤牌亮牌與 muck（規則細則 4.2）。
fn resolve_showdown(
    config: &TableConfig,
    setup: &HandSetup,
    state: &[SeatState],
    ranks: &[Option<u32>],
    final_aggressor: Option<usize>,
    events: &mut Vec<HandEvent>,
) -> Vec<bool> {
    let n = state.len();
    let mut revealed = vec![false; n];

    if contenders(state) < 2 {
        // 無人攤牌，未棄牌者不必亮牌
        return revealed;
    }

    // 亮牌順序：最後主動下注／加注者先亮；該街無人下注則自按鈕左側起
    let start = final_aggressor.unwrap_or_else(|| next_occupied(setup, setup.button));
    let order: Vec<usize> = (0..n)
        .map(|offset| (start + offset) % n)
        .filter(|&seat| !matches!(state[seat], SeatState::Folded))
        .collect();

    // 現實規則：有人全下且下注結束時，所有未棄牌者的底牌一律攤開。
    // side pot 只在有人全下時才存在，因此有 side pot 的手必然全部亮牌，
    // muck 判定只作用於單一底池的攤牌。
    let any_all_in = state.iter().any(|s| matches!(s, SeatState::AllIn));
    let force_show = any_all_in || config.muck == MuckPolicy::AlwaysShow;

    let mut best_shown: Option<u32> = None;
    for seat in order {
        let Some(rank) = ranks[seat] else { continue };
        let show = force_show || best_shown.is_none_or(|best| rank >= best);
        if show {
            revealed[seat] = true;
            best_shown = Some(best_shown.map_or(rank, |b| b.max(rank)));
            events.push(HandEvent::Revealed {
                seat,
                rank: crate::eval::HandRank::from_value(rank),
            });
        } else {
            events.push(HandEvent::Mucked { seat });
        }
    }
    revealed
}

fn contenders(state: &[SeatState]) -> usize {
    state
        .iter()
        .filter(|s| !matches!(s, SeatState::Folded))
        .count()
}

fn actives(state: &[SeatState]) -> usize {
    state
        .iter()
        .filter(|s| matches!(s, SeatState::Active))
        .count()
}

/// 自 `from` 左側第一位起的第一個有玩家座位。
fn next_occupied(setup: &HandSetup, from: usize) -> usize {
    let n = setup.occupied.len();
    (1..=n)
        .map(|offset| (from + offset) % n)
        .find(|&seat| setup.occupied[seat])
        .expect("桌上至少有一名玩家")
}

/// 自 `from` 左側第一位起的第一個仍可行動座位。
fn next_active_from(state: &[SeatState], from: usize) -> usize {
    let n = state.len();
    (1..=n)
        .map(|offset| (from + offset) % n)
        .find(|&seat| matches!(state[seat], SeatState::Active))
        .expect("至少有一名可行動玩家")
}
