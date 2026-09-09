//! Postflop 規則清單：條件比對、第一條命中、規則衝突偵測。
//!
//! 規格來源：核心規格 4.2、UI 規格 D.5。
//!
//! # 規則採第一條命中
//!
//! 規則依優先序排列，第一條條件全部滿足者生效。這讓「先寫特例、後寫通則」
//! 的編輯方式可行，但也帶來三種必須偵測的病狀（核心規格 4.2 明訂
//! 「編輯器必須偵測重疊、被遮蔽與永遠不會命中的規則」）：
//!
//! - **被遮蔽**：某規則的條件被更早的規則完全涵蓋，因此永遠輪不到它。
//!   這一定是錯的，列為 error。
//! - **不可能成立**：條件本身矛盾（例如人數範圍 5..3）。列為 error。
//! - **部分重疊**：兩條規則有交集但互不涵蓋。這在「先特例後通則」的寫法
//!   下是正常的，列為 warning 並寫入驗證摘要。
//!
//! # 多人底池的位置
//!
//! 核心規格 4.2：「多人底池**不得只用單一 IP／OOP 表達位置**。」
//! 因此條件同時保留英雄的位置標籤與「身後仍有幾個對手」，
//! 後者才是多人底池真正影響決策的量。

use std::ops::RangeInclusive;

use crate::betting::Action;
use crate::card::{Card, Rank};
use crate::chips::Chips;
use crate::hand::Street;
use crate::position::PositionLabel;
use crate::strategy::decision::StackBucket;
use crate::strategy::distribution::{ActionDistribution, DistributionError, Myriad, FULL};

/// 牌面外觀（六選一，UI 規格 D.5）。
///
/// # 為什麼外觀與順子結構是兩個欄位
///
/// 牌面不是八個互斥類別。「彩虹面」講的是花色與公對，「濕潤面」講的是
/// 順子結構，同一個牌面會同時是彩虹面與濕潤面。壓成單一欄位之後，
/// 「彩虹乾燥面」與「彩虹濕潤面」只能二選一寫進規則，另一半就永遠
/// 落到通則去——而這兩者的正確打法差很多。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoardSurface {
    /// 已有至少三張同花色公共牌，牌面可形成同花
    Flush,
    /// 尚未形成同花，但有兩張同花色公共牌
    FlushDraw,
    /// 無公對且沒有同花結構
    Rainbow,
    /// 有公對且沒有同花結構
    RainbowPaired,
    /// 有公對且有兩張同花色公共牌
    FlushDrawPaired,
    /// 公共牌本身有三張同點數
    Trips,
}

impl BoardSurface {
    pub const ALL: [Self; 6] = [
        Self::Flush,
        Self::FlushDraw,
        Self::Rainbow,
        Self::RainbowPaired,
        Self::FlushDrawPaired,
        Self::Trips,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Flush => "flush",
            Self::FlushDraw => "flush-draw",
            Self::Rainbow => "rainbow",
            Self::RainbowPaired => "rainbow-paired",
            Self::FlushDrawPaired => "flush-draw-paired",
            Self::Trips => "trips",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Flush => "花面",
            Self::FlushDraw => "聽花面",
            Self::Rainbow => "彩虹面",
            Self::RainbowPaired => "彩虹公對面",
            Self::FlushDrawPaired => "聽花公對面",
            Self::Trips => "三條面",
        }
    }

    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Flush => "公共牌已有至少三張同花色",
            Self::FlushDraw => "公共牌有兩張同花色，且沒有公對",
            Self::Rainbow => "公共牌沒有同花結構，也沒有公對",
            Self::RainbowPaired => "公共牌有公對，且沒有同花結構",
            Self::FlushDrawPaired => "公共牌同時有公對與兩張同花色",
            Self::Trips => "公共牌已有三張同點數",
        }
    }
}

/// 牌面的順子結構（二選一）。與 [`BoardSurface`] 是互相獨立的一軸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoardConnectivity {
    /// 沒有明顯順子結構
    Dry,
    /// 有順子或聽順結構
    Wet,
}

impl BoardConnectivity {
    pub const ALL: [Self; 2] = [Self::Dry, Self::Wet];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Dry => "dry",
            Self::Wet => "wet",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dry => "乾燥面",
            Self::Wet => "濕潤面",
        }
    }

    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Dry => "任一五點數視窗內少於三種公共牌點數",
            Self::Wet => "任一五點數視窗內至少有三種公共牌點數",
        }
    }
}

/// 一個牌面同時具有的外觀與順子結構。兩軸各取一個值，不是八選一。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoardTextures {
    surface: BoardSurface,
    connectivity: BoardConnectivity,
}

impl BoardTextures {
    #[must_use]
    pub const fn new(surface: BoardSurface, connectivity: BoardConnectivity) -> Self {
        Self {
            surface,
            connectivity,
        }
    }

    #[must_use]
    pub const fn surface(self) -> BoardSurface {
        self.surface
    }

    #[must_use]
    pub const fn connectivity(self) -> BoardConnectivity {
        self.connectivity
    }
}

/// 由當下已公開的 3～5 張公共牌判定牌面質地。
///
/// 外觀採優先序：三條面 → 花面 → 公對複合面 → 聽花面 → 彩虹面。
/// 使用優先序是因為需求沒有「花面公對」這個獨立類別；一旦牌面已有三張
/// 同花色，同花威脅比兩張同花色的公對類別更具辨識力。
#[must_use]
pub fn classify_board(board: &[Card]) -> Option<BoardTextures> {
    if !(3..=5).contains(&board.len()) {
        return None;
    }

    let mut suits = [0u8; 4];
    let mut ranks = [0u8; 15];
    for card in board {
        suits[card.suit.index()] = suits[card.suit.index()].saturating_add(1);
        let rank = usize::from(card.rank.value());
        ranks[rank] = ranks[rank].saturating_add(1);
    }

    let max_suit = suits.into_iter().max().unwrap_or(0);
    let max_rank = ranks.into_iter().max().unwrap_or(0);
    let paired = max_rank >= 2;
    let surface = if max_rank >= 3 {
        BoardSurface::Trips
    } else if max_suit >= 3 {
        BoardSurface::Flush
    } else if paired && max_suit >= 2 {
        BoardSurface::FlushDrawPaired
    } else if paired {
        BoardSurface::RainbowPaired
    } else if max_suit >= 2 {
        BoardSurface::FlushDraw
    } else {
        BoardSurface::Rainbow
    };
    let connectivity = if has_straight_structure(board) {
        BoardConnectivity::Wet
    } else {
        BoardConnectivity::Dry
    };

    Some(BoardTextures {
        surface,
        connectivity,
    })
}

fn has_straight_structure(board: &[Card]) -> bool {
    let mut present = [false; 15];
    for card in board {
        let rank = usize::from(card.rank.value());
        present[rank] = true;
        if card.rank == Rank::Ace {
            present[1] = true;
        }
    }

    (1usize..=10).any(|low| (low..=low + 4).filter(|rank| present[*rank]).count() >= 3)
}

/// 當前節點有沒有需要跟注的下注。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PostflopSituation {
    NoBet,
    FacingBet,
}

impl PostflopSituation {
    pub const ALL: [Self; 2] = [Self::NoBet, Self::FacingBet];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::NoBet => "no-bet",
            Self::FacingBet => "facing-bet",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoBet => "無人下注",
            Self::FacingBet => "面對下注",
        }
    }
}

/// 業主指定的六個翻後動作欄位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PostflopActionKind {
    Check,
    Call,
    ThirdPot,
    TwoThirdsPot,
    Pot,
    Fold,
}

impl PostflopActionKind {
    pub const ALL: [Self; 6] = [
        Self::Check,
        Self::Call,
        Self::ThirdPot,
        Self::TwoThirdsPot,
        Self::Pot,
        Self::Fold,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Call => "call",
            Self::ThirdPot => "third-pot",
            Self::TwoThirdsPot => "two-thirds-pot",
            Self::Pot => "pot",
            Self::Fold => "fold",
        }
    }

    #[must_use]
    pub const fn label(self, situation: PostflopSituation) -> &'static str {
        match self {
            Self::Check => "過牌",
            Self::Call => "跟注",
            Self::ThirdPot => match situation {
                PostflopSituation::NoBet => "下注 1/3 底池",
                PostflopSituation::FacingBet => "加注 1/3 底池",
            },
            Self::TwoThirdsPot => match situation {
                PostflopSituation::NoBet => "下注 2/3 底池",
                PostflopSituation::FacingBet => "加注 2/3 底池",
            },
            Self::Pot => match situation {
                PostflopSituation::NoBet => "下注 1 個底池",
                PostflopSituation::FacingBet => "加注 1 個底池",
            },
            Self::Fold => "蓋牌",
        }
    }

    #[must_use]
    pub const fn is_available(self, situation: PostflopSituation) -> bool {
        match situation {
            PostflopSituation::NoBet => !matches!(self, Self::Call | Self::Fold),
            PostflopSituation::FacingBet => !matches!(self, Self::Check),
        }
    }

    #[must_use]
    pub const fn pot_fraction(self) -> Option<(u64, u64)> {
        match self {
            Self::ThirdPot => Some((1, 3)),
            Self::TwoThirdsPot => Some((2, 3)),
            Self::Pot => Some((1, 1)),
            Self::Check | Self::Call | Self::Fold => None,
        }
    }
}

/// 牌力組（計劃 §2.1，八組）。
///
/// 舊的五組把 `Bluff` 放在裡面，但那是**動作意圖**不是手牌強度：同一手
/// A 高在不同節點可以是詐唬也可以是抓詐，用它當分組鍵等於把「打算怎麼打」
/// 混進「手上有什麼」。八組全部只描述手牌相對於牌面的強度。
///
/// 宣告順序與計劃 §2.3 的判定樹一致，但 **[`Self::priority`] 只供顯示
/// 排序**；分類判定走的是判定樹，不是這個順序，也不是 derive 的 `Ord`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HandStrength {
    /// 堅果／超強成牌
    Nuts,
    /// 成牌＋強聽牌
    MadePlusDraw,
    /// 強成牌
    StrongMade,
    /// 中等成牌
    MediumMade,
    /// 弱成牌／抓詐
    BluffCatcher,
    /// 強聽牌
    StrongDraw,
    /// 弱聽牌
    WeakDraw,
    /// 空氣牌
    Air,
}

impl HandStrength {
    pub const ALL: [Self; 8] = [
        Self::Nuts,
        Self::MadePlusDraw,
        Self::StrongMade,
        Self::MediumMade,
        Self::BluffCatcher,
        Self::StrongDraw,
        Self::WeakDraw,
        Self::Air,
    ];

    /// 河牌仍可能出現的五組。
    ///
    /// 河牌沒有未來補牌，因此三個帶聽牌的組別在該街**不可能成立**。
    /// 這件事在程式層封死，不只靠列舉器過濾（計劃 §2.3）。
    pub const RIVER: [Self; 5] = [
        Self::Nuts,
        Self::StrongMade,
        Self::MediumMade,
        Self::BluffCatcher,
        Self::Air,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Nuts => "nuts",
            Self::MadePlusDraw => "made-plus-draw",
            Self::StrongMade => "strong-made",
            Self::MediumMade => "medium-made",
            Self::BluffCatcher => "bluff-catcher",
            Self::StrongDraw => "strong-draw",
            Self::WeakDraw => "weak-draw",
            Self::Air => "air",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Nuts => "堅果／超強成牌",
            Self::MadePlusDraw => "成牌＋強聽牌",
            Self::StrongMade => "強成牌",
            Self::MediumMade => "中等成牌",
            Self::BluffCatcher => "弱成牌／抓詐",
            Self::StrongDraw => "強聽牌",
            Self::WeakDraw => "弱聽牌",
            Self::Air => "空氣牌",
        }
    }

    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Nuts => "接近目前牌面可形成的最強範圍",
            Self::MadePlusDraw => "已有攤牌價值，同時有大量有效補牌",
            Self::StrongMade => "可持續價值下注或加注",
            Self::MediumMade => "有攤牌價值，但通常不適合打大底池",
            Self::BluffCatcher => "主要用於控池或抓詐唬",
            Self::StrongDraw => "約八張以上有效補牌，或複合聽牌",
            Self::WeakDraw => "補牌較少、非堅果或只有後門潛力",
            Self::Air => "沒有足夠攤牌價值，也沒有有效聽牌",
        }
    }

    /// 顯示與排序用的序位。
    ///
    /// **這不是分類判定順序。** 分類走計劃 §2.3 的判定樹，其中
    /// `bluffCatcher` 必須向後檢查該手是否已符合強聽牌——那種例外
    /// 無法用「依序取第一個命中」表達。把這個序位當成分類流程的規格，
    /// 會鎖住一個判定流程實際上並不遵守的順序。
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::Nuts => 0,
            Self::MadePlusDraw => 1,
            Self::StrongMade => 2,
            Self::MediumMade => 3,
            Self::BluffCatcher => 4,
            Self::StrongDraw => 5,
            Self::WeakDraw => 6,
            Self::Air => 7,
        }
    }

    /// 這一組在該街是否可能成立。
    #[must_use]
    pub const fn reachable_on(self, street: Street) -> bool {
        match street {
            Street::River => !matches!(
                self,
                Self::MadePlusDraw | Self::StrongDraw | Self::WeakDraw
            ),
            _ => true,
        }
    }

    /// 該街可能出現的牌力組。
    #[must_use]
    pub fn for_street(street: Street) -> &'static [Self] {
        match street {
            Street::River => &Self::RIVER,
            _ => &Self::ALL,
        }
    }
}

/// 底池類型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PotType {
    SingleRaised,
    ThreeBet,
    FourBet,
}

/// 面對的下注尺度級距。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FacingSize {
    /// 無人下注
    None,
    Quarter,
    Third,
    Half,
    TwoThirds,
    ThreeQuarters,
    Pot,
    Overbet,
    AllIn,
}

/// 主動方是誰。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggressorRole {
    Hero,
    Opponent,
    /// 該街沒有人主動下注或加注
    None,
}

impl AggressorRole {
    pub const ALL: [Self; 3] = [Self::Hero, Self::Opponent, Self::None];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Hero => "hero",
            Self::Opponent => "opponent",
            Self::None => "none",
        }
    }
}

/// 英雄本街的行動順序，相對於 [`PostflopLine::last_aggressor_before_current_street`]
/// 所指的那位對手。
///
/// 該主動方是英雄自己或根本沒有主動方時，這個問題沒有意義，一律是
/// [`Self::NotApplicable`]——列舉器不得為不適用的情況產生兩個布林分支
/// （計劃 §4.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelativeAggressorOrder {
    /// 英雄在該對手之前行動
    HeroBefore,
    /// 英雄在該對手之後行動
    HeroAfter,
    NotApplicable,
}

impl RelativeAggressorOrder {
    pub const ALL: [Self; 3] = [Self::HeroBefore, Self::HeroAfter, Self::NotApplicable];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::HeroBefore => "hero-before",
            Self::HeroAfter => "hero-after",
            Self::NotApplicable => "not-applicable",
        }
    }
}

/// 牌局線路：英雄是怎麼走到這個節點的。
///
/// 全部由既有的公開 `PublicAction` history 與座位順序推導，不新增
/// `DecisionView` 欄位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PostflopLine {
    /// 前一街的主動方
    pub previous_street_aggressor: AggressorRole,
    /// 跨街回溯、**不含本街目前下注**的最近主動方。
    ///
    /// 即使上一街 check-through，仍保留更早街的主動方——delayed c-bet
    /// 與 probe 的差別就在這裡，只看前一街的話兩者會撞在同一個節點
    pub last_aggressor_before_current_street: AggressorRole,
    pub relative_aggressor_order: RelativeAggressorOrder,
    pub hero_checked_this_street: bool,
    pub current_street_bet_count: u8,
    pub current_street_raise_count: u8,
}

impl PostflopLine {
    /// 建立線路值，並強制施加不適用不變量。
    ///
    /// `relative_aggressor_order` 只在 `last_aggressor_before_current_street`
    /// 是對手時有意義；其餘情況一律正規化為 `NotApplicable`，
    /// 讓同一個牌局狀態只有一個 canonical 表示。
    #[must_use]
    pub const fn new(
        previous_street_aggressor: AggressorRole,
        last_aggressor_before_current_street: AggressorRole,
        relative_aggressor_order: RelativeAggressorOrder,
        hero_checked_this_street: bool,
        current_street_bet_count: u8,
        current_street_raise_count: u8,
    ) -> Self {
        let relative_aggressor_order = match last_aggressor_before_current_street {
            AggressorRole::Opponent => relative_aggressor_order,
            AggressorRole::Hero | AggressorRole::None => RelativeAggressorOrder::NotApplicable,
        };
        Self {
            previous_street_aggressor,
            last_aggressor_before_current_street,
            relative_aggressor_order,
            hero_checked_this_street,
            current_street_bet_count,
            current_street_raise_count,
        }
    }

    /// 線路值本身是否自洽。
    ///
    /// 兩條不變量：不適用時的行動順序必須是 `NotApplicable`；前一街有
    /// 主動方時，跨街最近主動方必然是同一個角色（本街之前的最近主動方
    /// 不可能比前一街更舊）。
    #[must_use]
    pub fn is_coherent(self) -> bool {
        // 適用時必須有順序、不適用時必須沒有。兩個方向都要守：只擋一邊的話
        // 「對手是主動方但順序不適用」這種空節點會混進列舉結果
        let order_ok = match self.last_aggressor_before_current_street {
            AggressorRole::Opponent => {
                self.relative_aggressor_order != RelativeAggressorOrder::NotApplicable
            }
            AggressorRole::Hero | AggressorRole::None => {
                self.relative_aggressor_order == RelativeAggressorOrder::NotApplicable
            }
        };
        let history_ok = match self.previous_street_aggressor {
            AggressorRole::None => true,
            role => self.last_aggressor_before_current_street == role,
        };
        order_ok && history_ok
    }
}

/// 線路的命名情境（計劃 §4.1）。
///
/// 名稱只描述**英雄當前面對的情境**，不預設他將跟注、加注或蓋牌——
/// 「check-raise」是英雄還沒做的選擇，拿來當節點名稱等於先射箭再畫靶。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PostflopLineName {
    // ── 無人下注 ──────────────────────────────────────────────
    CbetChance,
    DelayedCbetChance,
    DonkChance,
    StabChance,
    ProbeChance,
    DelayedStabChance,
    NoAggressorBetChance,
    // ── 面對下注 ──────────────────────────────────────────────
    FacingCbet,
    FacingDelayedCbet,
    FacingDonk,
    FacingStabAfterCheck,
    FacingProbe,
    FacingNoAggressorBet,
}

impl PostflopLineName {
    pub const NO_BET: [Self; 7] = [
        Self::CbetChance,
        Self::DelayedCbetChance,
        Self::DonkChance,
        Self::StabChance,
        Self::ProbeChance,
        Self::DelayedStabChance,
        Self::NoAggressorBetChance,
    ];

    pub const FACING_BET: [Self; 6] = [
        Self::FacingCbet,
        Self::FacingDelayedCbet,
        Self::FacingDonk,
        Self::FacingStabAfterCheck,
        Self::FacingProbe,
        Self::FacingNoAggressorBet,
    ];

    #[must_use]
    pub const fn situation(self) -> PostflopSituation {
        match self {
            Self::CbetChance
            | Self::DelayedCbetChance
            | Self::DonkChance
            | Self::StabChance
            | Self::ProbeChance
            | Self::DelayedStabChance
            | Self::NoAggressorBetChance => PostflopSituation::NoBet,
            Self::FacingCbet
            | Self::FacingDelayedCbet
            | Self::FacingDonk
            | Self::FacingStabAfterCheck
            | Self::FacingProbe
            | Self::FacingNoAggressorBet => PostflopSituation::FacingBet,
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::CbetChance => "cbet-chance",
            Self::DelayedCbetChance => "delayed-cbet-chance",
            Self::DonkChance => "donk-chance",
            Self::StabChance => "stab-chance",
            Self::ProbeChance => "probe-chance",
            Self::DelayedStabChance => "delayed-stab-chance",
            Self::NoAggressorBetChance => "no-aggressor-bet-chance",
            Self::FacingCbet => "facing-cbet",
            Self::FacingDelayedCbet => "facing-delayed-cbet",
            Self::FacingDonk => "facing-donk",
            Self::FacingStabAfterCheck => "facing-stab-after-check",
            Self::FacingProbe => "facing-probe",
            Self::FacingNoAggressorBet => "facing-no-aggressor-bet",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CbetChance => "c-bet 機會",
            Self::DelayedCbetChance => "delayed c-bet 機會",
            Self::DonkChance => "donk bet 機會",
            Self::StabChance => "stab／float bet 機會",
            Self::ProbeChance => "probe bet 機會",
            Self::DelayedStabChance => "delayed stab 機會",
            Self::NoAggressorBetChance => "無主動方底池下注機會",
            Self::FacingCbet => "面對 c-bet",
            Self::FacingDelayedCbet => "面對 delayed c-bet",
            Self::FacingDonk => "面對 donk bet",
            Self::FacingStabAfterCheck => "過牌後面對 stab／float bet",
            Self::FacingProbe => "面對 probe bet",
            Self::FacingNoAggressorBet => "面對無主動方底池下注",
        }
    }
}

impl PostflopLineName {
    /// 這個命名情境的最小條件（計劃 §4.1 表格）。
    ///
    /// 是 [`classify_line`] 的反向映射：`classify_line` 把具體線路值歸到
    /// 名稱，這個把名稱展開回條件集合。兩者必須對得起來，
    /// 由 `線路命名與最小條件互為反向映射` 測試釘住。
    #[must_use]
    pub fn minimal_condition(self) -> PostflopLineCondition {
        use AggressorRole as Role;
        use RelativeAggressorOrder as Order;

        let base = PostflopLineCondition::default;
        match self {
            Self::CbetChance => PostflopLineCondition {
                previous_street_aggressor: Some(Role::Hero),
                ..base()
            },
            Self::DelayedCbetChance => PostflopLineCondition {
                previous_street_aggressor: Some(Role::None),
                last_aggressor_before_current_street: Some(Role::Hero),
                ..base()
            },
            Self::DonkChance => PostflopLineCondition {
                previous_street_aggressor: Some(Role::Opponent),
                relative_aggressor_order: Some(Order::HeroBefore),
                ..base()
            },
            Self::StabChance => PostflopLineCondition {
                previous_street_aggressor: Some(Role::Opponent),
                relative_aggressor_order: Some(Order::HeroAfter),
                ..base()
            },
            Self::ProbeChance => PostflopLineCondition {
                previous_street_aggressor: Some(Role::None),
                last_aggressor_before_current_street: Some(Role::Opponent),
                relative_aggressor_order: Some(Order::HeroBefore),
                ..base()
            },
            Self::DelayedStabChance => PostflopLineCondition {
                previous_street_aggressor: Some(Role::None),
                last_aggressor_before_current_street: Some(Role::Opponent),
                relative_aggressor_order: Some(Order::HeroAfter),
                ..base()
            },
            Self::NoAggressorBetChance | Self::FacingNoAggressorBet => PostflopLineCondition {
                last_aggressor_before_current_street: Some(Role::None),
                ..base()
            },
            Self::FacingCbet => PostflopLineCondition {
                previous_street_aggressor: Some(Role::Opponent),
                ..base()
            },
            Self::FacingDelayedCbet => PostflopLineCondition {
                previous_street_aggressor: Some(Role::None),
                last_aggressor_before_current_street: Some(Role::Opponent),
                ..base()
            },
            Self::FacingDonk => PostflopLineCondition {
                previous_street_aggressor: Some(Role::Hero),
                hero_checked_this_street: Some(false),
                ..base()
            },
            Self::FacingStabAfterCheck => PostflopLineCondition {
                previous_street_aggressor: Some(Role::Hero),
                hero_checked_this_street: Some(true),
                ..base()
            },
            Self::FacingProbe => PostflopLineCondition {
                previous_street_aggressor: Some(Role::None),
                last_aggressor_before_current_street: Some(Role::Hero),
                ..base()
            },
        }
    }

    /// 一個必然歸入本情境的代表性線路值。
    ///
    /// 列舉器用它產生節點，測試用它驗證與 [`classify_line`] 的往返。
    #[must_use]
    pub fn representative_line(self) -> PostflopLine {
        use AggressorRole as Role;
        use RelativeAggressorOrder as Order;

        let (previous, last, order, hero_checked) = match self {
            Self::CbetChance => (Role::Hero, Role::Hero, Order::NotApplicable, false),
            Self::DelayedCbetChance => (Role::None, Role::Hero, Order::NotApplicable, false),
            Self::DonkChance => (Role::Opponent, Role::Opponent, Order::HeroBefore, false),
            Self::StabChance => (Role::Opponent, Role::Opponent, Order::HeroAfter, false),
            Self::ProbeChance => (Role::None, Role::Opponent, Order::HeroBefore, false),
            Self::DelayedStabChance => (Role::None, Role::Opponent, Order::HeroAfter, false),
            Self::NoAggressorBetChance | Self::FacingNoAggressorBet => {
                (Role::None, Role::None, Order::NotApplicable, false)
            }
            Self::FacingCbet => (Role::Opponent, Role::Opponent, Order::HeroAfter, false),
            Self::FacingDelayedCbet => (Role::None, Role::Opponent, Order::HeroAfter, false),
            Self::FacingDonk => (Role::Hero, Role::Hero, Order::NotApplicable, false),
            Self::FacingStabAfterCheck => (Role::Hero, Role::Hero, Order::NotApplicable, true),
            Self::FacingProbe => (Role::None, Role::Hero, Order::NotApplicable, false),
        };
        PostflopLine::new(previous, last, order, hero_checked, 0, 0)
    }
}

/// 把線路值歸類到唯一的命名情境。
///
/// **列舉器與 UI 共用這一個函式**，不各自複製判斷；兩份判斷遲早會分岔，
/// 而分岔的結果是同一個牌局狀態在導覽上叫一個名字、在規則比對時算另一
/// 個節點。
///
/// 多條命名條件可同時命中時依計劃 §4.1 的表格順序取第一個。回傳
/// `None` 代表這個線路值在該下注狀態下不可達——列舉測試會直接失敗，
/// 而不是偷偷塞進一個共用的「其他」桶。
#[must_use]
pub fn classify_line(situation: PostflopSituation, line: PostflopLine) -> Option<PostflopLineName> {
    use AggressorRole as Role;
    use RelativeAggressorOrder as Order;

    if !line.is_coherent() {
        return None;
    }

    let previous = line.previous_street_aggressor;
    let last = line.last_aggressor_before_current_street;
    let order = line.relative_aggressor_order;

    match situation {
        PostflopSituation::NoBet => Some(match (previous, last, order) {
            (Role::Hero, _, _) => PostflopLineName::CbetChance,
            (Role::None, Role::Hero, _) => PostflopLineName::DelayedCbetChance,
            (Role::Opponent, _, Order::HeroBefore) => PostflopLineName::DonkChance,
            (Role::Opponent, _, Order::HeroAfter) => PostflopLineName::StabChance,
            (Role::None, Role::Opponent, Order::HeroBefore) => PostflopLineName::ProbeChance,
            (Role::None, Role::Opponent, Order::HeroAfter) => PostflopLineName::DelayedStabChance,
            (Role::None, Role::None, _) => PostflopLineName::NoAggressorBetChance,
            // 對手是最近主動方卻沒有行動順序：`is_coherent` 已擋掉，
            // 這裡明列而不用 catch-all，將來新增角色時編譯器才會提醒
            (Role::Opponent, _, Order::NotApplicable)
            | (Role::None, Role::Opponent, Order::NotApplicable) => return None,
        }),
        PostflopSituation::FacingBet => Some(match (previous, last) {
            (Role::Opponent, _) => PostflopLineName::FacingCbet,
            (Role::None, Role::Opponent) => PostflopLineName::FacingDelayedCbet,
            (Role::Hero, _) => {
                if line.hero_checked_this_street {
                    PostflopLineName::FacingStabAfterCheck
                } else {
                    PostflopLineName::FacingDonk
                }
            }
            (Role::None, Role::Hero) => PostflopLineName::FacingProbe,
            (Role::None, Role::None) => PostflopLineName::FacingNoAggressorBet,
        }),
    }
}

/// 規則側的線路條件。`None` 代表不限制該欄位。
///
/// 與 [`PostflopLine`] **刻意不對稱**：那個是牌局的具體值，這個是可選
/// 條件與範圍。用同一個型別表達兩者的話，「至少加注一次」這種條件就
/// 只能寫成相等比較，寫不出來。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostflopLineCondition {
    pub previous_street_aggressor: Option<AggressorRole>,
    pub last_aggressor_before_current_street: Option<AggressorRole>,
    pub relative_aggressor_order: Option<RelativeAggressorOrder>,
    pub hero_checked_this_street: Option<bool>,
    /// 本街下注次數，用範圍才表達得出「至少一次」
    pub current_street_bet_count: Option<RangeInclusive<u8>>,
    pub current_street_raise_count: Option<RangeInclusive<u8>>,
}

impl PostflopLineCondition {
    /// 條件是否成立於該線路值。
    #[must_use]
    pub fn matches(&self, line: PostflopLine) -> bool {
        option_matches(
            self.previous_street_aggressor,
            line.previous_street_aggressor,
        ) && option_matches(
            self.last_aggressor_before_current_street,
            line.last_aggressor_before_current_street,
        ) && option_matches(
            self.relative_aggressor_order,
            line.relative_aggressor_order,
        ) && option_matches(self.hero_checked_this_street, line.hero_checked_this_street)
            && range_matches(
                self.current_street_bet_count.as_ref(),
                line.current_street_bet_count,
            )
            && range_matches(
                self.current_street_raise_count.as_ref(),
                line.current_street_raise_count,
            )
    }

    #[must_use]
    pub fn is_impossible(&self) -> bool {
        range_impossible(self.current_street_bet_count.as_ref())
            || range_impossible(self.current_street_raise_count.as_ref())
    }

    /// 本條件是否完全涵蓋另一條件（集合關係，不是相等）。
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        option_contains(
            self.previous_street_aggressor,
            other.previous_street_aggressor,
        ) && option_contains(
            self.last_aggressor_before_current_street,
            other.last_aggressor_before_current_street,
        ) && option_contains(
            self.relative_aggressor_order,
            other.relative_aggressor_order,
        ) && option_contains(
            self.hero_checked_this_street,
            other.hero_checked_this_street,
        ) && range_contains(
            self.current_street_bet_count.as_ref(),
            other.current_street_bet_count.as_ref(),
        ) && range_contains(
            self.current_street_raise_count.as_ref(),
            other.current_street_raise_count.as_ref(),
        )
    }

    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        option_intersects(
            self.previous_street_aggressor,
            other.previous_street_aggressor,
        ) && option_intersects(
            self.last_aggressor_before_current_street,
            other.last_aggressor_before_current_street,
        ) && option_intersects(
            self.relative_aggressor_order,
            other.relative_aggressor_order,
        ) && option_intersects(
            self.hero_checked_this_street,
            other.hero_checked_this_street,
        ) && range_intersects(
            self.current_street_bet_count.as_ref(),
            other.current_street_bet_count.as_ref(),
        ) && range_intersects(
            self.current_street_raise_count.as_ref(),
            other.current_street_raise_count.as_ref(),
        )
    }
}

impl FacingSize {
    pub const ALL: [Self; 9] = [
        Self::None,
        Self::Quarter,
        Self::Third,
        Self::Half,
        Self::TwoThirds,
        Self::ThreeQuarters,
        Self::Pot,
        Self::Overbet,
        Self::AllIn,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Quarter => "quarter",
            Self::Third => "third",
            Self::Half => "half",
            Self::TwoThirds => "two-thirds",
            Self::ThreeQuarters => "three-quarters",
            Self::Pot => "pot",
            Self::Overbet => "overbet",
            Self::AllIn => "all-in",
        }
    }
}

/// 決策節點的實際狀態，供條件比對。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostflopContext {
    pub street: Street,
    pub board_textures: BoardTextures,
    pub hand_strength: HandStrength,
    /// 本街仍在牌局的人數
    pub active_players: u8,
    pub hero_position: PositionLabel,
    /// 英雄身後仍會行動的對手數。多人底池的位置優勢由此表達，
    /// 而非壓縮成單一 IP／OOP
    pub opponents_behind: u8,
    pub pot_type: PotType,
    pub facing_size: FacingSize,
    /// SPR × 100，避免浮點進入規則比對
    pub spr_centi: u32,
    /// 有效籌碼分檔。沿用翻前既有的 [`StackBucket`] 邊界（規則細則 8.5），
    /// 不另立第二套——同一個牌局狀態在翻前翻後落在不同檔會直接造成誤讀
    pub effective_stack_bucket: StackBucket,
    /// 英雄是怎麼走到這個節點的
    pub line: PostflopLine,
}

impl PostflopContext {
    /// 這個節點是無人下注還是面對下注。
    ///
    /// 由 `facing_size` 推導而不另存一個欄位：兩者若各自可設，就會出現
    /// 「面對下注但尺度是 None」這種對不起來的狀態。
    #[must_use]
    pub const fn situation(&self) -> PostflopSituation {
        match self.facing_size {
            FacingSize::None => PostflopSituation::NoBet,
            _ => PostflopSituation::FacingBet,
        }
    }
}

/// 規則條件。`None` 代表萬用（不限制該欄位）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostflopCondition {
    pub street: Option<Street>,
    /// 無人下注或面對下注。
    ///
    /// 與 `facing_size` 是兩個粒度：`situation` 讓一條規則涵蓋「面對下注
    /// 的所有尺度」，`facing_size` 才指定到某一檔。兩者矛盾（面對下注
    /// 卻指定尺度 `None`）由 [`Self::is_impossible`] 擋掉
    pub situation: Option<PostflopSituation>,
    /// 牌面外觀。與 `board_connectivity` 是獨立兩軸，規則可以只指定其中
    /// 一軸作為廣域條件，也可以同時指定兩軸精確匹配
    pub board_surface: Option<BoardSurface>,
    pub board_connectivity: Option<BoardConnectivity>,
    pub hand_strength: Option<HandStrength>,
    pub active_players: Option<RangeInclusive<u8>>,
    pub hero_position: Option<PositionLabel>,
    pub opponents_behind: Option<RangeInclusive<u8>>,
    pub pot_type: Option<PotType>,
    pub facing_size: Option<FacingSize>,
    pub spr_centi: Option<RangeInclusive<u32>>,
    /// 有效籌碼分檔。與 `spr_centi` 同級：SPR 講的是底池與籌碼的比例，
    /// 這個講的是絕對深度，兩者都會改變正確打法
    pub effective_stack_bucket: Option<StackBucket>,
    pub line: PostflopLineCondition,
}

impl PostflopCondition {
    /// 條件是否成立於該節點。
    #[must_use]
    pub fn matches(&self, context: &PostflopContext) -> bool {
        option_matches(self.street, context.street)
            && option_matches(self.situation, context.situation())
            && option_matches(self.board_surface, context.board_textures.surface())
            && option_matches(
                self.board_connectivity,
                context.board_textures.connectivity(),
            )
            && option_matches(self.hand_strength, context.hand_strength)
            && range_matches(self.active_players.as_ref(), context.active_players)
            && option_matches(self.hero_position, context.hero_position)
            && range_matches(self.opponents_behind.as_ref(), context.opponents_behind)
            && option_matches(self.pot_type, context.pot_type)
            && option_matches(self.facing_size, context.facing_size)
            && range_matches(self.spr_centi.as_ref(), context.spr_centi)
            && option_matches(
                self.effective_stack_bucket,
                context.effective_stack_bucket,
            )
            && self.line.matches(context.line)
    }

    /// 條件本身是否不可能成立（範圍顛倒）。
    #[must_use]
    pub fn is_impossible(&self) -> bool {
        // 下注狀態與面對尺度自相矛盾：無人下注不可能有尺度，
        // 面對下注不可能沒有
        let situation_conflict = match (self.situation, self.facing_size) {
            (Some(PostflopSituation::NoBet), Some(size)) => size != FacingSize::None,
            (Some(PostflopSituation::FacingBet), Some(FacingSize::None)) => true,
            _ => false,
        };

        situation_conflict
            || range_impossible(self.active_players.as_ref())
            || range_impossible(self.opponents_behind.as_ref())
            || range_impossible(self.spr_centi.as_ref())
            || self.line.is_impossible()
    }

    /// 本條件是否**完全涵蓋**另一條件。
    ///
    /// 涵蓋代表：凡是 `other` 會命中的節點，本條件也一定命中。
    /// 若涵蓋者排在前面，被涵蓋者永遠輪不到——這就是遮蔽。
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        option_contains(self.street, other.street)
            && option_contains(self.situation, other.situation)
            && option_contains(self.board_surface, other.board_surface)
            && option_contains(self.board_connectivity, other.board_connectivity)
            && option_contains(self.hand_strength, other.hand_strength)
            && range_contains(self.active_players.as_ref(), other.active_players.as_ref())
            && option_contains(self.hero_position, other.hero_position)
            && range_contains(
                self.opponents_behind.as_ref(),
                other.opponents_behind.as_ref(),
            )
            && option_contains(self.pot_type, other.pot_type)
            && option_contains(self.facing_size, other.facing_size)
            && range_contains(self.spr_centi.as_ref(), other.spr_centi.as_ref())
            && option_contains(
                self.effective_stack_bucket,
                other.effective_stack_bucket,
            )
            && self.line.contains(&other.line)
    }

    /// 兩條件是否有交集。
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        option_intersects(self.street, other.street)
            && option_intersects(self.situation, other.situation)
            && option_intersects(self.board_surface, other.board_surface)
            && option_intersects(self.board_connectivity, other.board_connectivity)
            && option_intersects(self.hand_strength, other.hand_strength)
            && range_intersects(self.active_players.as_ref(), other.active_players.as_ref())
            && option_intersects(self.hero_position, other.hero_position)
            && range_intersects(
                self.opponents_behind.as_ref(),
                other.opponents_behind.as_ref(),
            )
            && option_intersects(self.pot_type, other.pot_type)
            && option_intersects(self.facing_size, other.facing_size)
            && range_intersects(self.spr_centi.as_ref(), other.spr_centi.as_ref())
            && option_intersects(
                self.effective_stack_bucket,
                other.effective_stack_bucket,
            )
            && self.line.intersects(&other.line)
    }
}

fn option_matches<T: PartialEq>(constraint: Option<T>, actual: T) -> bool {
    constraint.is_none_or(|c| c == actual)
}

fn range_matches<T: PartialOrd>(constraint: Option<&RangeInclusive<T>>, actual: T) -> bool {
    constraint.is_none_or(|r| r.contains(&actual))
}

fn range_impossible<T: PartialOrd>(constraint: Option<&RangeInclusive<T>>) -> bool {
    constraint.is_some_and(|r| r.start() > r.end())
}

/// 萬用涵蓋一切；具體值只涵蓋自己。
fn option_contains<T: PartialEq>(outer: Option<T>, inner: Option<T>) -> bool {
    match (outer, inner) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(a), Some(b)) => a == b,
    }
}

fn range_contains<T: PartialOrd>(
    outer: Option<&RangeInclusive<T>>,
    inner: Option<&RangeInclusive<T>>,
) -> bool {
    match (outer, inner) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(a), Some(b)) => a.start() <= b.start() && a.end() >= b.end(),
    }
}

fn option_intersects<T: PartialEq>(a: Option<T>, b: Option<T>) -> bool {
    match (a, b) {
        (None, _) | (_, None) => true,
        (Some(x), Some(y)) => x == y,
    }
}

fn range_intersects<T: PartialOrd>(
    a: Option<&RangeInclusive<T>>,
    b: Option<&RangeInclusive<T>>,
) -> bool {
    match (a, b) {
        (None, _) | (_, None) => true,
        (Some(x), Some(y)) => x.start() <= y.end() && y.start() <= x.end(),
    }
}

/// 換算尺寸意圖時需要的當下金額。
///
/// 規則保存的是「1/3 底池」這種**意圖**，不是具體籌碼數；同一條規則在
/// 不同底池大小下換算出不同的 `Action`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostflopSizing {
    /// 目前底池（不含英雄尚未投入的跟注額）
    pub pot: Chips,
    /// 英雄要跟注的金額。無人下注時為 0
    pub to_call: Chips,
}

/// 把底池比例意圖換算成「加注到多少」。
///
/// - 無人下注：下注額 ＝ 底池 × 比例。
/// - 面對下注：先算跟注後的底池，再乘比例，最後加上跟注額——這是實際
///   桌上「加注到 N」的意思，而不是「在跟注之上再加底池的 N 倍」。
///
/// 換算結果仍會被 `fit_raise_sizes` 夾進引擎給的合法區間；這裡不自行
/// 推導最小加注或全下（核心規格 2.2：策略層不得自行推導合法性）。
#[must_use]
pub fn postflop_raise_to(sizing: PostflopSizing, numerator: u64, denominator: u64) -> Chips {
    let denominator = denominator.max(1);
    let to_call = sizing.to_call.units();
    let pot_after_call = sizing.pot.units().saturating_add(to_call);
    let increment = pot_after_call.saturating_mul(numerator) / denominator;
    Chips::new(to_call.saturating_add(increment))
}

/// 規則保存的尺寸意圖分布。
///
/// **不另立六欄 `u16` 的權重結構**：那會和既有的
/// [`ActionDistribution`] 形成兩份真理，而兩份真理遲早會對不起來。
/// 這裡沿用同一個 [`Myriad`]（`FULL = 10_000`），只是鍵換成尺寸意圖。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostflopIntentDistribution {
    weights: Vec<(PostflopActionKind, Myriad)>,
}

impl PostflopIntentDistribution {
    /// 權重合計必須是 `FULL`。重複的鍵先合併再檢查。
    ///
    /// # Errors
    /// 合計不等於 `FULL` 或清單為空時回傳錯誤。
    pub fn new(weights: Vec<(PostflopActionKind, Myriad)>) -> Result<Self, DistributionError> {
        if weights.is_empty() {
            return Err(DistributionError::Empty);
        }

        let mut merged: Vec<(PostflopActionKind, Myriad)> = Vec::with_capacity(weights.len());
        for (kind, weight) in weights {
            match merged.iter_mut().find(|(other, _)| *other == kind) {
                Some((_, existing)) => *existing = existing.saturating_add(weight),
                None => merged.push((kind, weight)),
            }
        }

        let total: u64 = merged.iter().map(|(_, w)| u64::from(*w)).sum();
        if total != u64::from(FULL) {
            return Err(DistributionError::NotNormalised {
                total: Myriad::try_from(total).unwrap_or(Myriad::MAX),
            });
        }
        Ok(Self { weights: merged })
    }

    #[must_use]
    pub fn weights(&self) -> &[(PostflopActionKind, Myriad)] {
        &self.weights
    }

    /// 某個意圖的權重（未列出即為 0）。
    #[must_use]
    pub fn weight_of(&self, kind: PostflopActionKind) -> Myriad {
        self.weights
            .iter()
            .find(|(other, _)| *other == kind)
            .map_or(0, |(_, weight)| *weight)
    }

    /// 依當下金額換算成具體行動分布。
    ///
    /// 撞在同一個合法尺寸上的意圖會被合併（例如底池很小時 2/3 與 1 倍
    /// 底池可能都被夾到最小加注），因此回傳的分布可能比意圖少幾項。
    ///
    /// # Errors
    /// 換算後為空時回傳錯誤，呼叫端須走 fallback。
    pub fn to_actions(
        &self,
        situation: PostflopSituation,
        sizing: PostflopSizing,
    ) -> Result<ActionDistribution, DistributionError> {
        let mut merged: Vec<(Action, u64)> = Vec::new();
        for (kind, weight) in &self.weights {
            // 與下注狀態矛盾的意圖直接丟掉：無人下注不會有跟注與蓋牌，
            // 面對下注不會有過牌
            if !kind.is_available(situation) || *weight == 0 {
                continue;
            }
            let action = match kind {
                PostflopActionKind::Check => Action::Check,
                PostflopActionKind::Call => Action::Call,
                PostflopActionKind::Fold => Action::Fold,
                sized => {
                    let (numerator, denominator) =
                        sized.pot_fraction().unwrap_or((1, 1));
                    Action::RaiseTo(postflop_raise_to(sizing, numerator, denominator))
                }
            };
            match merged.iter_mut().find(|(existing, _)| *existing == action) {
                Some((_, existing_weight)) => *existing_weight += u64::from(*weight),
                None => merged.push((action, u64::from(*weight))),
            }
        }
        ActionDistribution::from_weights(merged)
    }
}

/// 規則的來源層級（計劃 §4.4）。
///
/// 宣告順序即解析順序：使用者覆寫壓過官方內容，官方壓過同組通則，
/// 通則壓過工程 fallback。`derive` 的 `Ord` 就是這個不變量，
/// [`RuleSet::new`] 會據此檢查規則排列。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleSource {
    /// 使用者自己填的絕對頻率
    UserOverride,
    /// 顧問簽核的官方內容
    Official,
    /// 同牌力組的一般規則
    Generic,
    /// 未簽核的工程基準
    EngineeringFallback,
}

impl RuleSource {
    pub const ALL: [Self; 4] = [
        Self::UserOverride,
        Self::Official,
        Self::Generic,
        Self::EngineeringFallback,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::UserOverride => "user-override",
            Self::Official => "official",
            Self::Generic => "generic",
            Self::EngineeringFallback => "engineering-fallback",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UserOverride => "你的覆寫",
            Self::Official => "官方內容",
            Self::Generic => "同組通則",
            Self::EngineeringFallback => "工程 fallback",
        }
    }

    /// 命中這一層算不算「玩家自己寫的策略」。
    ///
    /// UI 規格 D.7 的完整度分子是**玩家規則**。工程 fallback 本身也是
    /// 規則，把它算進去的話比例會逼近 100%，而那個數字的用途正好是
    /// 告訴使用者「還有多少節點沒寫」。
    #[must_use]
    pub const fn counts_as_player_content(self) -> bool {
        matches!(self, Self::UserOverride)
    }
}

/// 一條 postflop 規則。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostflopRule {
    /// 穩定識別碼，供 UI 顯示「你的規則 R#」與 trace 引用
    pub id: String,
    pub name: String,
    pub condition: PostflopCondition,
    /// 尺寸**意圖**頻率，合計必為 100%。
    ///
    /// 存意圖而不存具體 `Action`：同一條「1/3 底池 45%」的規則，在
    /// 底池 20 與底池 200 時要下的籌碼完全不同，決策當下才換算
    pub intent: PostflopIntentDistribution,
    pub source: RuleSource,
    /// 內容版本字串，隨規則列一起進 manifest
    pub version: String,
    /// 是否經牌手顧問簽核。未簽核內容不得顯示為 GTO 或正式顧問策略
    pub consultant_approved: bool,
}

impl PostflopRule {
    /// 未簽核的工程規則。測試與工程基準用，省去每次重複四個欄位。
    #[must_use]
    pub fn engineering(
        id: impl Into<String>,
        name: impl Into<String>,
        condition: PostflopCondition,
        intent: PostflopIntentDistribution,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            condition,
            intent,
            source: RuleSource::EngineeringFallback,
            version: "v0-unapproved".to_owned(),
            consultant_approved: false,
        }
    }
}

/// 規則清單的問題（計劃 §5.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleIssue {
    /// 同一來源層內，條件被更早的規則完全涵蓋，永遠不會命中。
    ///
    /// **warning，不是 error。** 跨層被壓過是設計本意（使用者覆寫本來
    /// 就該壓過通則），而同層被遮蔽雖然幾乎一定是寫錯，擋住保存卻會讓
    /// 使用者連暫存都做不到
    Shadowed { rule: usize, by: usize },
    /// 條件本身不可能成立（範圍為空、權重全落在該狀態不成立的動作）。
    /// 必須修正
    Impossible { rule: usize },
    /// 同一來源層內與更早的規則部分重疊。「先特例後通則」下屬正常，僅提示
    Overlap { rule: usize, with: usize },
    /// 條件命不中任何可達節點。可能是條件寫錯，也可能是節點集合改版
    Unreachable { rule: usize },
    /// 規則排列違反來源優先序不變量。必須修正——順序錯了的話，
    /// 使用者的覆寫會被官方通則蓋掉，而畫面上完全看不出來
    LayerOrderViolation { rule: usize, after: usize },
}

impl RuleIssue {
    /// 是否為必須處理的錯誤。
    ///
    /// UI 規格 D.5：「使用者必須處理 error，warning 可保留但寫入驗證
    /// 摘要。」計劃 §5.3 把 `Shadowed` 與 `Unreachable` 明訂為 warning，
    /// 只有靜態不可能與來源排序違反擋住保存。
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(
            self,
            Self::Impossible { .. } | Self::LayerOrderViolation { .. }
        )
    }

    /// 這個問題指向哪一條規則。
    #[must_use]
    pub const fn rule(self) -> usize {
        match self {
            Self::Shadowed { rule, .. }
            | Self::Impossible { rule }
            | Self::Overlap { rule, .. }
            | Self::Unreachable { rule }
            | Self::LayerOrderViolation { rule, .. } => rule,
        }
    }
}

/// 比對結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matched {
    /// 命中第 n 條規則。
    ///
    /// `source` 一起回傳，否則完整度統計只知道「命中了規則」，
    /// 而工程 fallback 規則本身也是規則——分不出層的話，比例會
    /// 逼近 100%，UI 規格 D.7 的定義直接被破壞
    Rule { index: usize, source: RuleSource },
    /// 未命中任何規則，或命中後合法行動權重歸零，須走 fallback
    Fallback(FallbackReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// 沒有任何規則的條件成立
    NoRuleMatched,
    /// 規則命中，但其行動在 legal mask 後權重全為 0
    AllWeightsMasked { rule: usize },
}

/// 規則清單。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSet {
    rules: Vec<PostflopRule>,
    /// fallback 基準策略的版本。核心規格 4.2 要求寫入 log
    pub fallback_version: String,
}

impl RuleSet {
    #[must_use]
    pub fn new(rules: Vec<PostflopRule>, fallback_version: impl Into<String>) -> Self {
        Self {
            rules,
            fallback_version: fallback_version.into(),
        }
    }

    #[must_use]
    pub fn rules(&self) -> &[PostflopRule] {
        &self.rules
    }

    /// 找出第一條命中的規則，並套用 legal mask。
    ///
    /// 核心規格 4.2：「不合法行動先 mask，再正規化；若剩餘權重為 0，
    /// 必須進入 fallback，**不得除以 0 或任選行動**。」
    ///
    /// # Errors
    /// 遮蔽與正規化的內部錯誤會轉為 fallback，因此本函式不回傳錯誤；
    /// 呼叫端由 [`Matched`] 判斷是否需要 fallback。
    #[must_use]
    pub fn resolve(
        &self,
        context: &PostflopContext,
        sizing: PostflopSizing,
        is_legal: &impl Fn(Action) -> bool,
    ) -> (Matched, Option<ActionDistribution>) {
        for (index, rule) in self.rules.iter().enumerate() {
            if !rule.condition.matches(context) {
                continue;
            }
            // 意圖換算成具體行動：撞在同一個合法尺寸上的意圖在這裡合併，
            // 因此換算結果可能比規則裡的項目少
            let Ok(actions) = rule.intent.to_actions(context.situation(), sizing) else {
                return (
                    Matched::Fallback(FallbackReason::AllWeightsMasked { rule: index }),
                    None,
                );
            };
            return match actions.mask_and_renormalise(is_legal) {
                Ok(distribution) => (
                    Matched::Rule {
                        index,
                        source: rule.source,
                    },
                    Some(distribution),
                ),
                Err(DistributionError::AllWeightsMasked | DistributionError::Empty) => (
                    Matched::Fallback(FallbackReason::AllWeightsMasked { rule: index }),
                    None,
                ),
                Err(_) => (
                    Matched::Fallback(FallbackReason::AllWeightsMasked { rule: index }),
                    None,
                ),
            };
        }
        (Matched::Fallback(FallbackReason::NoRuleMatched), None)
    }

    /// 檢查規則清單的病狀（計劃 §5.3）。
    ///
    /// 依優先序檢查：只有排在前面的規則才可能遮蔽後面的。
    ///
    /// **遮蔽與重疊只在同一來源層內比較。** 使用者覆寫壓過官方通則是
    /// 設計本意，跨層相交報成警告的話，每寫一條覆寫就多一個假警報，
    /// 真正的問題會被噪音蓋掉。
    ///
    /// 不含 `Unreachable`——那需要可達節點集合，見
    /// [`Self::analyse_reachability`]。
    #[must_use]
    pub fn analyse(&self) -> Vec<RuleIssue> {
        let mut issues = Vec::new();

        for (index, rule) in self.rules.iter().enumerate() {
            // 來源排序不變量：規則列必須依 UserOverride → Official →
            // Generic → EngineeringFallback 排列。順序錯了的話使用者的
            // 覆寫會被官方通則蓋掉，而畫面上完全看不出來
            if let Some(earlier) = self.rules[..index]
                .iter()
                .position(|other| other.source > rule.source)
            {
                issues.push(RuleIssue::LayerOrderViolation {
                    rule: index,
                    after: earlier,
                });
            }

            if rule.condition.is_impossible() {
                issues.push(RuleIssue::Impossible { rule: index });
                continue;
            }

            let mut shadowed = false;
            for (earlier, other) in self.rules.iter().enumerate().take(index) {
                if other.source != rule.source || other.condition.is_impossible() {
                    continue;
                }
                if other.condition.contains(&rule.condition) {
                    issues.push(RuleIssue::Shadowed {
                        rule: index,
                        by: earlier,
                    });
                    shadowed = true;
                    break;
                }
            }
            if shadowed {
                continue;
            }
            for (earlier, other) in self.rules.iter().enumerate().take(index) {
                if other.source == rule.source
                    && !other.condition.is_impossible()
                    && other.condition.intersects(&rule.condition)
                {
                    issues.push(RuleIssue::Overlap {
                        rule: index,
                        with: earlier,
                    });
                    break;
                }
            }
        }
        issues
    }

    /// 找出命不中任何可達節點的規則。
    ///
    /// 成本是 O(規則數 × 節點數)，因此與 [`Self::analyse`] 分開：
    /// 靜態檢查每次編輯都可以跑，這一支留給保存前的完整驗證。
    #[must_use]
    pub fn analyse_reachability(&self, nodes: &[PostflopNode]) -> Vec<RuleIssue> {
        self.rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                // 節點是一組 context 的集合，因此判準是條件**相交**而非
                // 相等：一條只指定牌力組的廣域規則涵蓋很多節點，
                // 用相等比較會把它全部誤判成不可達
                !rule.condition.is_impossible()
                    && !nodes
                        .iter()
                        .any(|node| rule.condition.intersects(&node.condition()))
            })
            .map(|(rule, _)| RuleIssue::Unreachable { rule })
            .collect()
    }
}

/// 節點集合的版本。
///
/// 節點集合會隨階段擴充（facing size、SPR、位置、有效籌碼逐項加入），
/// 同一份覆寫的完整度會在不同版本間暴跌。UI、報表與 manifest 都要帶
/// 這個字串，否則跨 run 比較沒有意義（計劃 §5.3）。
pub const NODE_SET_VERSION: &str = "postflop-nodes/v1";

/// 第一版面對下注時提供的尺度檔位。
///
/// 完整的 [`FacingSize`] 仍保留在引擎條件層——UI 少列幾檔是內容決策，
/// 引擎少支援幾檔就沒辦法描述真實牌局了（計劃 §十 待確認 5）。
pub const FACING_SIZES_V1: [FacingSize; 3] =
    [FacingSize::Third, FacingSize::TwoThirds, FacingSize::Pot];

/// 一個可編輯的翻後策略節點。
///
/// 節點是一組 context 的**集合**，不是單一 context：位置、玩家數、SPR、
/// 有效籌碼分檔留在「更多條件」，由規則條件表達。因此節點對外的形狀是
/// [`Self::condition`]，而不是一個填滿所有欄位的 `PostflopContext`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PostflopNode {
    pub street: Street,
    pub situation: PostflopSituation,
    pub line: PostflopLineName,
    pub surface: BoardSurface,
    pub connectivity: BoardConnectivity,
    pub hand_strength: HandStrength,
    /// 無人下注時恆為 [`FacingSize::None`]
    pub facing_size: FacingSize,
}

impl PostflopNode {
    /// canonical 節點鍵。覆寫 DTO 與 trace 都用這個字串定位節點。
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            street_key(self.street),
            self.situation.key(),
            self.line.key(),
            self.surface.key(),
            self.connectivity.key(),
            self.hand_strength.key(),
            self.facing_size.key(),
        )
    }

    /// 這個節點涵蓋的條件集合。
    #[must_use]
    pub fn condition(&self) -> PostflopCondition {
        PostflopCondition {
            street: Some(self.street),
            situation: Some(self.situation),
            board_surface: Some(self.surface),
            board_connectivity: Some(self.connectivity),
            hand_strength: Some(self.hand_strength),
            facing_size: Some(self.facing_size),
            line: self.line.minimal_condition(),
            ..PostflopCondition::default()
        }
    }
}

#[must_use]
const fn street_key(street: Street) -> &'static str {
    match street {
        Street::Preflop => "preflop",
        Street::Flop => "flop",
        Street::Turn => "turn",
        Street::River => "river",
    }
}

/// 牌面外觀與順子結構的組合在該街是否可能出現。
///
/// 公共牌已有三張同點數時，五點數視窗內最多只有那一個點數加上其餘公共牌
/// 的點數：翻牌三張全同點、轉牌四張裡只有兩種點數，都湊不出「三種點數」
/// 的濕潤條件。河牌五張才可能（三條＋兩張落在同一視窗內的其他點數）。
#[must_use]
pub const fn surface_connectivity_reachable(
    street: Street,
    surface: BoardSurface,
    connectivity: BoardConnectivity,
) -> bool {
    match (surface, connectivity) {
        (BoardSurface::Trips, BoardConnectivity::Wet) => matches!(street, Street::River),
        _ => true,
    }
}

/// 列舉真正可達的翻後節點。
///
/// **不是笛卡兒積。** 排除的組合：
///
/// - 河牌的三個帶聽牌牌力組（[`HandStrength::reachable_on`]）
/// - 翻牌與轉牌的「三條面＋濕潤面」（[`surface_connectivity_reachable`]）
/// - 與下注狀態矛盾的 `facing_size`（無人下注沒有面對尺度可談）
/// - 命不中 §4.1 唯一命名映射的線路（由 [`PostflopLineName`] 的清單保證）
///
/// 排除後仍留在清單裡的每一個節點都必須是可編輯的；列舉器不產生共用的
/// 「其他」桶，出現未覆蓋的可達線路時，測試會直接失敗要求先命名。
#[must_use]
pub fn enumerate_postflop_nodes() -> Vec<PostflopNode> {
    let mut nodes = Vec::new();

    for street in [Street::Flop, Street::Turn, Street::River] {
        for situation in PostflopSituation::ALL {
            let lines: &[PostflopLineName] = match situation {
                PostflopSituation::NoBet => &PostflopLineName::NO_BET,
                PostflopSituation::FacingBet => &PostflopLineName::FACING_BET,
            };
            let facing_sizes: &[FacingSize] = match situation {
                PostflopSituation::NoBet => &[FacingSize::None],
                PostflopSituation::FacingBet => &FACING_SIZES_V1,
            };

            for &line in lines {
                for surface in BoardSurface::ALL {
                    for connectivity in BoardConnectivity::ALL {
                        if !surface_connectivity_reachable(street, surface, connectivity) {
                            continue;
                        }
                        for &hand_strength in HandStrength::for_street(street) {
                            for &facing_size in facing_sizes {
                                nodes.push(PostflopNode {
                                    street,
                                    situation,
                                    line,
                                    surface,
                                    connectivity,
                                    hand_strength,
                                    facing_size,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    nodes
}

/// 節點規模報表（計劃 §4.2）。
///
/// 逐項加維度，讓凍結 DTO 與官方內容之前看得到規模怎麼長出來的——
/// 一次只給總數的話，沒有人說得出是哪一個維度把節點數推爆的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCountReport {
    pub node_set_version: &'static str,
    /// 街別 × 下注狀態 × 線路 × 外觀 × 結構 × 牌力組
    pub core: usize,
    /// 再加面對下注的三檔尺度。這是 [`enumerate_postflop_nodes`] 的實際輸出
    pub with_facing_size: usize,
    /// 再加底池類型（單次加注／3-bet／4-bet）
    pub with_pot_type: usize,
    /// 再加既有的九檔有效籌碼
    pub with_stack_bucket: usize,
    /// 未加維度前，笛卡兒積會有多少——用來看排除規則擋掉了多少不可達組合
    pub cartesian: usize,
}

/// 產生節點規模報表。
#[must_use]
pub fn node_count_report() -> NodeCountReport {
    let with_facing_size = enumerate_postflop_nodes().len();
    let core = enumerate_postflop_nodes()
        .into_iter()
        .map(|node| PostflopNode {
            facing_size: FacingSize::None,
            ..node
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    let streets = 3;
    let situations = PostflopSituation::ALL.len();
    let lines = PostflopLineName::NO_BET.len().max(PostflopLineName::FACING_BET.len());
    let cartesian = streets
        * situations
        * lines
        * BoardSurface::ALL.len()
        * BoardConnectivity::ALL.len()
        * HandStrength::ALL.len()
        * FACING_SIZES_V1.len();

    NodeCountReport {
        node_set_version: NODE_SET_VERSION,
        core,
        with_facing_size,
        with_pot_type: with_facing_size * 3,
        with_stack_bucket: with_facing_size * 3 * 9,
        cartesian,
    }
}

/// 執行期的策略覆蓋率統計，**依來源分層**。
///
/// UI 規格 D.7：「策略完整度 = 命中玩家規則的節點數 ÷ 總決策節點數」。
/// 核心規格 4.2 另要求 fallback 的命中次數寫入 log。
///
/// # 為什麼一定要分層
///
/// 四層規則塞進同一個 [`RuleSet`] 之後，「命中規則」不再等於「命中玩家
/// 寫的規則」——工程 fallback 規則本身也是規則，不分層的話完整度會逼近
/// 100%，而這個數字存在的唯一用途正是告訴使用者還有多少節點沒寫。
///
/// # 分母界定
///
/// 只算英雄座位、只算英雄有實際選擇的節點（已全下、無合法選項不算）。
/// 分母會隨節點集合改版而變，因此比較兩份統計前必須先確認
/// [`NODE_SET_VERSION`] 相同。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageStats {
    pub user_hits: u64,
    pub official_hits: u64,
    pub generic_hits: u64,
    /// 命中工程 fallback**規則**。與 `fallback_no_rule` 不同：
    /// 這是有規則可用，只是那條規則未經簽核
    pub engineering_hits: u64,
    /// 沒有任何規則命中。這是策略缺口
    pub fallback_no_rule: u64,
    /// 規則命中但合法動作全被遮罩。這是籌碼造成的合法性事件，
    /// 不是策略缺口——兩者混在一起會讓使用者以為策略沒寫完
    pub fallback_masked: u64,
}

impl CoverageStats {
    pub fn record(&mut self, matched: &Matched) {
        match matched {
            Matched::Rule { source, .. } => match source {
                RuleSource::UserOverride => self.user_hits += 1,
                RuleSource::Official => self.official_hits += 1,
                RuleSource::Generic => self.generic_hits += 1,
                RuleSource::EngineeringFallback => self.engineering_hits += 1,
            },
            Matched::Fallback(FallbackReason::NoRuleMatched) => self.fallback_no_rule += 1,
            Matched::Fallback(FallbackReason::AllWeightsMasked { .. }) => {
                self.fallback_masked += 1;
            }
        }
    }

    #[must_use]
    pub const fn rule_hits(&self) -> u64 {
        self.user_hits + self.official_hits + self.generic_hits + self.engineering_hits
    }

    #[must_use]
    pub const fn total(&self) -> u64 {
        self.rule_hits() + self.fallback_no_rule + self.fallback_masked
    }

    /// 玩家策略完整度（萬分比）。無決策節點時回傳 `None`。
    ///
    /// 分子只有使用者覆寫。官方內容與同組通則不是使用者寫的，
    /// 工程 fallback 更不是（計劃 §5.3）。
    #[must_use]
    pub fn completeness_myriad(&self) -> Option<u32> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        u32::try_from(self.user_hits * 10_000 / total).ok()
    }
}
