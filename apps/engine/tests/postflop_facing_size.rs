//! 面對下注尺度的驗收測試（0907 計劃 §4.1）。
//!
//! 這裡的 `DecisionView` **全部由引擎跑真牌局產生**，不是手寫 fixture。
//! 手寫的話很容易把「下注後的底池」填進 `pot` 再拿它當分母，測出來的
//! 就是那個錯誤本身：對手往 100 底池推 100，`to_call / pot` 得到
//! 100/200＝50%，滿池下注被歸成半池，使用者編輯「面對滿池」的覆寫
//! 因此永遠命不中一般的滿池下注。
//!
//! 下注額由下注者**當下的 view** 算出來（那一刻的 `pot` 正是下注前的
//! 底池），因此期望值與引擎看到的是同一個數字，不靠人工對帳。

use std::cell::RefCell;

use poker_engine::betting::Action;
use poker_engine::card::Card;
use poker_engine::chips::Chips;
use poker_engine::hand::{play_hand_with_deal, ActionProvider, HandSetup, PreparedDeal, Street};
use poker_engine::strategy::decision::DecisionView;
use poker_engine::strategy::hand_strength::HandStrengthConfig;
use poker_engine::strategy::postflop::FacingSize;
use poker_engine::strategy::postflop_view::postflop_node;
use poker_engine::table::TableConfig;

const SEATS: usize = 6;
/// 按鈕座位。翻後最後行動，因此一定面對得到別人的下注
const HERO: usize = 0;
const SB: usize = 1;
const BB: usize = 2;
const UTG: usize = 3;
/// 英雄翻前加注到這個數字，把底池撐大到有空間下各種尺度
const HERO_OPEN: u64 = 20;

fn card(text: &str) -> Card {
    Card::parse(text).unwrap_or_else(|| panic!("無法解析牌張 {text}"))
}

fn setup() -> HandSetup {
    HandSetup {
        stacks: vec![Chips::new(400); SEATS],
        occupied: vec![true; SEATS],
        button: HERO,
        small_blind_seat: Some(SB),
        big_blind_seat: BB,
    }
}

/// 固定發牌。牌面內容與尺度分檔無關，指定只是為了讓牌局可重現。
fn deal() -> PreparedDeal {
    let hands = [
        ["As", "Kd"],
        ["7c", "2h"],
        ["Qs", "Jh"],
        ["9c", "9d"],
        ["4s", "4h"],
        ["6c", "5d"],
    ];
    PreparedDeal {
        hole_cards: hands
            .iter()
            .map(|hand| Some([card(hand[0]), card(hand[1])]))
            .collect(),
        board: [card("Ah"), card("7d"), card("3c"), card("Ts"), card("2c")],
    }
}

/// 翻牌怎麼打。三種腳本共用同一份翻前劇本（英雄開池、指定對手跟注）。
#[derive(Clone, Copy)]
enum FlopScript {
    /// 對手直接下注：底池 × `numerator` ÷ `denominator`
    Bet { numerator: u64, denominator: u64 },
    /// 英雄先下注半池，對手再加注到「加注前底池」的指定倍數
    Reraise { numerator: u64, denominator: u64 },
    /// 對手全下，但籌碼不足以構成完整加注
    ShortAllIn,
}

/// 依腳本打完翻牌，並記下英雄面對下注時看到的 view。
struct Table {
    script: FlopScript,
    /// 翻前跟注的對手；`UTG` 也跟的話就是三人底池
    callers: Vec<usize>,
    /// 翻牌的下注者
    bettor: usize,
    /// 下注者行動當下的底池，與他實際推出去的增量
    observed: RefCell<Vec<(u64, u64)>>,
    hero_view: RefCell<Option<DecisionView>>,
}

impl Table {
    fn new(script: FlopScript, callers: Vec<usize>, bettor: usize) -> Self {
        Self {
            script,
            callers,
            bettor,
            observed: RefCell::new(Vec::new()),
            hero_view: RefCell::new(None),
        }
    }

    fn run(self) -> (DecisionView, u64, u64) {
        let mut table = self;
        let _ = play_hand_with_deal(&TableConfig::simple(1, 2), &setup(), &deal(), &mut table);
        let view = table
            .hero_view
            .borrow()
            .clone()
            .expect("英雄應面對過一次下注");
        let (pot_before, bet) = *table.observed.borrow().last().expect("翻牌應有一次進攻");
        (view, pot_before, bet)
    }

    fn preflop(&self, view: &DecisionView) -> Action {
        if view.seat == HERO {
            return Action::RaiseTo(Chips::new(HERO_OPEN));
        }
        if self.callers.contains(&view.seat) {
            return if view.to_call.units() > 0 {
                Action::Call
            } else {
                Action::Check
            };
        }
        Action::Fold
    }

    fn flop(&self, view: &DecisionView) -> Action {
        let facing = view.to_call.units() > 0;
        if view.seat == HERO {
            if facing {
                *self.hero_view.borrow_mut() = Some(view.clone());
                return Action::Fold;
            }
            // 英雄先開一注，對手才有得加。短碼腳本開得小一點，
            // 對手的全下才會是「不足額的加注」而不是部分跟注
            let opening = match self.script {
                FlopScript::Reraise { .. } => Some(view.pot.units() / 2),
                FlopScript::ShortAllIn => Some(view.pot.units() / 4),
                FlopScript::Bet { .. } => None,
            };
            if let Some(bet) = opening {
                self.observed.borrow_mut().push((view.pot.units(), bet));
                return Action::RaiseTo(Chips::new(bet));
            }
            return Action::Check;
        }

        if view.seat == self.bettor {
            match self.script {
                FlopScript::Bet {
                    numerator,
                    denominator,
                } if !facing => {
                    let pot = view.pot.units();
                    let bet = pot * numerator / denominator;
                    self.observed.borrow_mut().push((pot, bet));
                    return Action::RaiseTo(Chips::new(bet));
                }
                FlopScript::Reraise {
                    numerator,
                    denominator,
                } if facing => {
                    // 加注的基準是「他跟平前一手注之後的底池」：
                    // 面對 50 的注、底池 150 時，滿池加注是先跟 50
                    // （底池 200）再加 200
                    let base = view.pot.units() + view.to_call.units();
                    let increment = base * numerator / denominator;
                    let level = view.to_call.units() + view.committed_here();
                    self.observed.borrow_mut().push((base, increment));
                    return Action::RaiseTo(Chips::new(level + increment));
                }
                FlopScript::ShortAllIn if facing => {
                    let all_in = view.legal.all_in_to.expect("短碼應有全下上限");
                    let level = view.to_call.units() + view.committed_here();
                    self.observed.borrow_mut().push((
                        view.pot.units() + view.to_call.units(),
                        all_in.units() - level,
                    ));
                    return Action::AllIn;
                }
                _ => {}
            }
        }

        // 翻前跟進的其他人在翻牌照樣跟：多人底池才有「中途跟注」可測
        if self.callers.contains(&view.seat) && facing {
            return Action::Call;
        }
        if facing {
            Action::Fold
        } else {
            Action::Check
        }
    }
}

/// 這一座在本街已投入多少。`DecisionView` 沒有直接欄位，
/// 但公開歷史裡有它自己的 `committed_to`。
trait CommittedHere {
    fn committed_here(&self) -> u64;
}

impl CommittedHere for DecisionView {
    fn committed_here(&self) -> u64 {
        self.current_street_history()
            .iter()
            .filter(|action| action.seat == self.seat)
            .map(|action| action.committed_to.units())
            .max()
            .unwrap_or(0)
    }
}

impl ActionProvider for Table {
    fn choose(&mut self, view: &DecisionView) -> Action {
        match view.street {
            Street::Preflop => self.preflop(view),
            Street::Flop => self.flop(view),
            // 英雄棄牌之後剩下的人還是會把牌打完。後面幾街不照腳本下注，
            // 否則 `observed` 記下的會是轉牌那一注
            _ => {
                if view.legal.can_check {
                    Action::Check
                } else {
                    Action::Fold
                }
            }
        }
    }
}

fn facing_size_of(view: &DecisionView) -> FacingSize {
    postflop_node(view, &HandStrengthConfig::ENGINEERING)
        .expect("翻後節點")
        .context
        .facing_size
}

// ── 一般下注 ────────────────────────────────────────────────────

/// 分母必須是**下注前**的底池。
///
/// 五個檔位一起測：只測滿池的話，改壞分母的人可以把門檻整排往下挪
/// 讓那一個案例過。
#[test]
fn 下注尺度以下注前的底池分檔() {
    let cases = [
        (1u64, 3u64, FacingSize::UpToThird),
        (1, 2, FacingSize::ThirdToHalf),
        (2, 3, FacingSize::HalfToTwoThirds),
        (1, 1, FacingSize::PotOrMore),
        (3, 2, FacingSize::PotOrMore),
    ];

    for (numerator, denominator, expected) in cases {
        let (view, pot_before, bet) = Table::new(
            FlopScript::Bet {
                numerator,
                denominator,
            },
            vec![BB],
            BB,
        )
        .run();

        assert_eq!(
            facing_size_of(&view),
            expected,
            "對手往 {pot_before} 的底池下 {bet}（{numerator}/{denominator} 底池）"
        );
        // 分母抓錯的具體徵狀：把這一注也算進底池，比例會系統性偏低
        let inflated = bet * 1_000 / (pot_before + bet);
        let honest = bet * 1_000 / pot_before;
        assert!(
            honest > inflated,
            "含了這注的底池必然給出更小的比例，這正是要避免的分母"
        );
    }
}

/// 滿池下注是這個缺陷最直接的受害者：舊分母把它歸成半池。
#[test]
fn 滿池下注歸在滿池而不是半池() {
    let (view, pot_before, bet) = Table::new(
        FlopScript::Bet {
            numerator: 1,
            denominator: 1,
        },
        vec![BB],
        BB,
    )
    .run();

    assert_eq!(bet, pot_before, "腳本應下出剛好一個底池");
    assert_eq!(view.pot.units(), pot_before + bet, "當下底池已含這一注");
    assert_eq!(facing_size_of(&view), FacingSize::PotOrMore);
}

/// 多人底池：在進攻者之後才跟進的籌碼不算進他的基準。
///
/// 那些錢在他決定尺度時還不在池子裡，算進去會讓同一個下注在三人底池
/// 被歸到比較小的一檔。
#[test]
fn 中途跟注的籌碼不算進下注者的基準() {
    let (view, pot_before, bet) = Table::new(
        FlopScript::Bet {
            numerator: 1,
            denominator: 1,
        },
        vec![BB, UTG],
        BB,
    )
    .run();

    assert!(
        view.pot.units() > pot_before + bet,
        "英雄行動時，UTG 的跟注已經進池"
    );
    assert_eq!(
        facing_size_of(&view),
        FacingSize::PotOrMore,
        "下注者推的仍是一個底池，不因為後面有人跟而變小"
    );
}

// ── 再加注與短碼 ────────────────────────────────────────────────

/// 面對加注時，量的是**加注的增量**相對於他跟平前一手注之後的底池。
///
/// 拿「加注到的總額 ÷ 當下底池」會把半池加注算成別的東西。
#[test]
fn 再加注以加注增量對跟平後的底池分檔() {
    let (view, base, increment) = Table::new(
        FlopScript::Reraise {
            numerator: 1,
            denominator: 1,
        },
        vec![BB],
        BB,
    )
    .run();

    assert_eq!(increment, base, "腳本應加出剛好一個底池的增量");
    assert_eq!(facing_size_of(&view), FacingSize::PotOrMore);
    assert!(
        view.pot.units() > base,
        "當下底池已含這次加注，拿它當分母會把滿池加注歸到更小的一檔"
    );
}

/// 籌碼不足的全下只是部分加注，不該被歸成 overbet。
#[test]
fn 短碼全下依實際增量分檔而不是全下就算_overbet() {
    let mut table = Table::new(FlopScript::ShortAllIn, vec![BB], BB);
    table.callers = vec![BB];
    // 對手的籌碼剛好夠跟翻前，翻牌只剩一點點
    let mut short = setup();
    short.stacks[BB] = Chips::new(HERO_OPEN + 12);

    let _ = play_hand_with_deal(&TableConfig::simple(1, 2), &short, &deal(), &mut table);
    let view = table
        .hero_view
        .borrow()
        .clone()
        .expect("英雄應面對過那個全下");

    let size = facing_size_of(&view);
    assert_ne!(
        size,
        FacingSize::PotOrMore,
        "全下的是短碼，推出去的增量遠小於底池"
    );
    assert_eq!(size, FacingSize::UpToThird);
}

// ── 無人下注 ────────────────────────────────────────────────────

#[test]
fn 無人下注時沒有尺度() {
    let (view, _, _) = Table::new(
        FlopScript::Bet {
            numerator: 1,
            denominator: 1,
        },
        vec![BB],
        BB,
    )
    .run();
    let mut checked = view;
    checked.to_call = Chips::ZERO;
    assert_eq!(facing_size_of(&checked), FacingSize::None);
}
