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
use crate::hand::Street;
use crate::position::PositionLabel;
use crate::strategy::distribution::{ActionDistribution, DistributionError};

/// 公共牌面質地（UI 規格 D.5）。
///
/// 前六項是互斥的牌面外觀；`Dry`／`Wet` 是另一個維度。同一個牌面會同時
/// 命中一個外觀與一個順子結構，例如 `Rainbow + Wet`。若把八項硬做成單選，
/// 彩虹但有順子結構的牌面就一定會失真。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoardTexture {
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
    /// 任一五點數視窗內少於三種公共牌點數
    Dry,
    /// 任一五點數視窗內至少有三種公共牌點數
    Wet,
}

impl BoardTexture {
    pub const ALL: [Self; 8] = [
        Self::Flush,
        Self::FlushDraw,
        Self::Rainbow,
        Self::RainbowPaired,
        Self::FlushDrawPaired,
        Self::Trips,
        Self::Dry,
        Self::Wet,
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
            Self::Dry => "dry",
            Self::Wet => "wet",
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
            Self::Dry => "乾燥面",
            Self::Wet => "濕潤面",
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
            Self::Dry => "沒有明顯順子結構",
            Self::Wet => "有順子或聽順結構",
        }
    }

    #[must_use]
    pub const fn dimension(self) -> &'static str {
        match self {
            Self::Dry | Self::Wet => "順子結構",
            _ => "花色／公對",
        }
    }

    const fn is_connectivity(self) -> bool {
        matches!(self, Self::Dry | Self::Wet)
    }
}

/// 一個牌面同時具有的「花色／公對」與「順子結構」標籤。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoardTextures {
    surface: BoardTexture,
    connectivity: BoardTexture,
}

impl BoardTextures {
    #[must_use]
    pub const fn surface(self) -> BoardTexture {
        self.surface
    }

    #[must_use]
    pub const fn connectivity(self) -> BoardTexture {
        self.connectivity
    }

    #[must_use]
    pub fn contains(self, texture: BoardTexture) -> bool {
        self.surface == texture || self.connectivity == texture
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
        BoardTexture::Trips
    } else if max_suit >= 3 {
        BoardTexture::Flush
    } else if paired && max_suit >= 2 {
        BoardTexture::FlushDrawPaired
    } else if paired {
        BoardTexture::RainbowPaired
    } else if max_suit >= 2 {
        BoardTexture::FlushDraw
    } else {
        BoardTexture::Rainbow
    };
    let connectivity = if has_straight_structure(board) {
        BoardTexture::Wet
    } else {
        BoardTexture::Dry
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

/// 手牌強度分桶。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HandStrength {
    Value,
    Bluff,
    BluffCatcher,
    Draw,
    Air,
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
}

/// 規則條件。`None` 代表萬用（不限制該欄位）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostflopCondition {
    pub street: Option<Street>,
    pub board_texture: Option<BoardTexture>,
    pub hand_strength: Option<HandStrength>,
    pub active_players: Option<RangeInclusive<u8>>,
    pub hero_position: Option<PositionLabel>,
    pub opponents_behind: Option<RangeInclusive<u8>>,
    pub pot_type: Option<PotType>,
    pub facing_size: Option<FacingSize>,
    pub spr_centi: Option<RangeInclusive<u32>>,
}

impl PostflopCondition {
    /// 條件是否成立於該節點。
    #[must_use]
    pub fn matches(&self, context: &PostflopContext) -> bool {
        option_matches(self.street, context.street)
            && self
                .board_texture
                .is_none_or(|texture| context.board_textures.contains(texture))
            && option_matches(self.hand_strength, context.hand_strength)
            && range_matches(self.active_players.as_ref(), context.active_players)
            && option_matches(self.hero_position, context.hero_position)
            && range_matches(self.opponents_behind.as_ref(), context.opponents_behind)
            && option_matches(self.pot_type, context.pot_type)
            && option_matches(self.facing_size, context.facing_size)
            && range_matches(self.spr_centi.as_ref(), context.spr_centi)
    }

    /// 條件本身是否不可能成立（範圍顛倒）。
    #[must_use]
    pub fn is_impossible(&self) -> bool {
        range_impossible(self.active_players.as_ref())
            || range_impossible(self.opponents_behind.as_ref())
            || range_impossible(self.spr_centi.as_ref())
    }

    /// 本條件是否**完全涵蓋**另一條件。
    ///
    /// 涵蓋代表：凡是 `other` 會命中的節點，本條件也一定命中。
    /// 若涵蓋者排在前面，被涵蓋者永遠輪不到——這就是遮蔽。
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        option_contains(self.street, other.street)
            && option_contains(self.board_texture, other.board_texture)
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
    }

    /// 兩條件是否有交集。
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        option_intersects(self.street, other.street)
            && texture_intersects(self.board_texture, other.board_texture)
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
    }
}

fn texture_intersects(a: Option<BoardTexture>, b: Option<BoardTexture>) -> bool {
    match (a, b) {
        (None, _) | (_, None) => true,
        (Some(x), Some(y)) => x == y || x.is_connectivity() != y.is_connectivity(),
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

/// 一條 postflop 規則。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostflopRule {
    pub name: String,
    pub condition: PostflopCondition,
    /// 行動頻率，合計必為 100%（由 `ActionDistribution` 保證）
    pub actions: ActionDistribution,
}

/// 規則清單的問題。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleIssue {
    /// 條件被更早的規則完全涵蓋，永遠不會命中。必須修正
    Shadowed { rule: usize, by: usize },
    /// 條件本身不可能成立。必須修正
    Impossible { rule: usize },
    /// 與更早的規則部分重疊。「先特例後通則」下屬正常，僅提示
    Overlap { rule: usize, with: usize },
}

impl RuleIssue {
    /// 是否為必須處理的錯誤。
    ///
    /// UI 規格 D.5：「使用者必須處理 error，warning 可保留但寫入驗證摘要。」
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Shadowed { .. } | Self::Impossible { .. })
    }
}

/// 比對結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matched {
    /// 命中第 n 條規則
    Rule(usize),
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
        is_legal: &impl Fn(Action) -> bool,
    ) -> (Matched, Option<ActionDistribution>) {
        for (index, rule) in self.rules.iter().enumerate() {
            if !rule.condition.matches(context) {
                continue;
            }
            return match rule.actions.mask_and_renormalise(is_legal) {
                Ok(distribution) => (Matched::Rule(index), Some(distribution)),
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

    /// 檢查規則清單的三種病狀。
    ///
    /// 依優先序檢查：只有排在前面的規則才可能遮蔽後面的。
    #[must_use]
    pub fn analyse(&self) -> Vec<RuleIssue> {
        let mut issues = Vec::new();

        for (index, rule) in self.rules.iter().enumerate() {
            if rule.condition.is_impossible() {
                issues.push(RuleIssue::Impossible { rule: index });
                continue;
            }
            // 只需與更早的規則比較
            let mut shadowed = false;
            for (earlier, other) in self.rules.iter().enumerate().take(index) {
                if other.condition.is_impossible() {
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
                if !other.condition.is_impossible() && other.condition.intersects(&rule.condition) {
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
}

/// 策略覆蓋率統計。
///
/// UI 規格 D.7：「策略完整度 = 命中玩家規則的節點數 ÷ 總決策節點數」。
/// 核心規格 4.2 另要求 fallback 的命中次數寫入 log。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageStats {
    pub rule_hits: u64,
    pub fallback_no_rule: u64,
    pub fallback_masked: u64,
}

impl CoverageStats {
    pub fn record(&mut self, matched: &Matched) {
        match matched {
            Matched::Rule(_) => self.rule_hits += 1,
            Matched::Fallback(FallbackReason::NoRuleMatched) => self.fallback_no_rule += 1,
            Matched::Fallback(FallbackReason::AllWeightsMasked { .. }) => {
                self.fallback_masked += 1;
            }
        }
    }

    #[must_use]
    pub const fn total(&self) -> u64 {
        self.rule_hits + self.fallback_no_rule + self.fallback_masked
    }

    /// 策略完整度（萬分比）。無決策節點時回傳 `None`。
    #[must_use]
    pub fn completeness_myriad(&self) -> Option<u32> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        u32::try_from(self.rule_hits * 10_000 / total).ok()
    }
}
