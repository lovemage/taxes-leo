//! 翻前**預設組合表**：顧問給的 9MAX 逐格手牌清單。
//!
//! # 這份內容與 `baseline` 的關係
//!
//! [`crate::strategy::baseline`] 是把數十個參數展開成六十幾萬格的產生器，
//! 用途是「顧問還沒給表之前先有東西可跑」。這個模組相反：內容就是顧問
//! **逐格寫死的手牌清單**，沒有 equity 排序、沒有內插、沒有混合帶。
//!
//! 兩者的優先序是 覆寫 → 預設組合表 → 參數產生器。預設組合表涵蓋不到的
//! 節點（面對跛入）才會落回參數，見 [`DefaultChart::lookup`]。
//!
//! # 為什麼是離線資產而不是執行期讀 Excel
//!
//! 引擎沒有任何相依（連 serde 都沒有），而 Tauri 打包後的工作目錄不由
//! 我們決定——執行期讀檔會在使用者的機器上失敗而在開發機上永遠成功。
//! 因此 `.xlsx` 由 `tools/preflop_chart_from_xlsx.py` 離線轉成純文字，
//! 以 `include_str!` 編進二進位檔。
//!
//! # 6–8 人桌不另立內容
//!
//! 來源表只有 9MAX。少人數桌**刪位置**而不重新產表：8 人刪 UTG+2、
//! 7 人再刪 UTG+1、6 人再刪 LJ。這與規則細則 8.4.1 的位置序列一致，
//! [`chart_positions`] 有測試把兩者釘在一起。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::betting::Action;
use crate::chips::Chips;
use crate::position::PositionLabel;
use crate::strategy::baseline::centi_bb_to_chips;
use crate::strategy::decision::StackBucket;
use crate::strategy::distribution::{ActionDistribution, Myriad, FULL};
use crate::strategy::hand_class::HandClass;
use crate::strategy::preflop::{PreflopNode, PreflopScenario};
use crate::strategy::ranking::EquityRanking;

/// 資產格式版本。欄位語意改變時遞增，舊資產會在載入時被擋下。
pub const ASSET_FORMAT: u32 = 1;

/// 資產路徑（相對於 `apps/engine`）。
pub const ASSET_PATH: &str = "assets/preflop-default-chart-v1.txt";

/// 內容版本字串。面板必須說得出自己畫的是哪一份表。
pub const CHART_VERSION: &str = "9MAX手牌組合_6/v1";

const EMBEDDED: &str = include_str!("../../assets/preflop-default-chart-v1.txt");

/// 全部 1,326 個 combo。範圍寬度的分母。
const TOTAL_COMBOS: u32 = 1_326;

// ── 維度 ────────────────────────────────────────────────────────────────

/// 來源表的四檔有效籌碼深度。
///
/// 與引擎的九檔 [`StackBucket`] 是兩套刻度：bucket 是引擎判定用的細分，
/// 這四檔是顧問實際寫表的深度。對應關係見 [`ChartDepth::from_bucket`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChartDepth {
    /// 0–15BB：推入或棄牌
    Short,
    /// 35–50BB
    Medium,
    /// 100BB
    Standard,
    /// 200–250BB
    Deep,
}

impl ChartDepth {
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Short, Self::Medium, Self::Standard, Self::Deep]
    }

    /// 資產的欄位鍵。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Short => "0-15",
            Self::Medium => "35-50",
            Self::Standard => "100",
            Self::Deep => "200-250",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Short => "0–15BB",
            Self::Medium => "35–50BB",
            Self::Standard => "100BB",
            Self::Deep => "200–250BB",
        }
    }

    #[must_use]
    pub fn from_key(text: &str) -> Option<Self> {
        Self::all().into_iter().find(|d| d.as_str() == text)
    }

    /// 引擎的九檔 bucket 對應到哪一檔顧問深度。
    ///
    /// 取**代表深度最接近**的一檔：顧問的四檔代表值分別是 7.5／42.5／
    /// 100／225 BB，引擎九檔取各自區間的中點去比。寫成明表而不是執行期
    /// 算距離，是因為這個對應本身要能被顧問審查——「40–70BB 的節點用的
    /// 是 35–50BB 那一欄」必須看得見，而不是藏在一段算式裡。
    #[must_use]
    pub const fn from_bucket(bucket: StackBucket) -> Self {
        match bucket {
            // 中點 7.5、20 → 7.5 那一檔
            StackBucket::VeryShort | StackBucket::Short => Self::Short,
            // 中點 32.5、55 → 42.5 那一檔
            StackBucket::Medium | StackBucket::Deep => Self::Medium,
            // 中點 90、135 → 100 那一檔
            StackBucket::Deeper | StackBucket::Deepest => Self::Standard,
            // 中點 200、320 與 400+ → 225 那一檔
            StackBucket::VeryDeep | StackBucket::UltraDeep | StackBucket::Unbounded => Self::Deep,
        }
    }
}

/// 來源表的五種情境。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChartScenario {
    /// 前面無人加注（RFI）
    Unopened,
    /// 前方有 1 人開牌加注
    Open,
    /// 前方有人開牌「又被另一人加注」→ 冷 4-bet／冷跟／棄
    OpenRaise,
    /// 你開牌後被 3-bet
    ThreeBet,
    /// 你 3-bet 後被 4-bet
    FourBet,
}

impl ChartScenario {
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [
            Self::Unopened,
            Self::Open,
            Self::OpenRaise,
            Self::ThreeBet,
            Self::FourBet,
        ]
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unopened => "unopened",
            Self::Open => "open",
            Self::OpenRaise => "open-raise",
            Self::ThreeBet => "3bet",
            Self::FourBet => "4bet",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unopened => "前面無人加注",
            Self::Open => "OPEN（前方 1 人開牌）",
            Self::OpenRaise => "OPEN-RAISE（開牌＋再加注）",
            Self::ThreeBet => "3B（你開牌後被 3-bet）",
            Self::FourBet => "4B（你 3-bet 後被 4-bet）",
        }
    }

    #[must_use]
    pub fn from_key(text: &str) -> Option<Self> {
        Self::all().into_iter().find(|s| s.as_str() == text)
    }

    /// 引擎的節點情境對應到表上的哪一欄。
    ///
    /// `VsSqueeze` 併入 `3B`：擠壓對英雄而言就是「開牌後被再加注」，
    /// 差別只在中間那個跟注者，而來源表沒有分這一格。併過去而不是落回
    /// 參數產生器，是因為 3B 那一欄至少是顧問寫的內容。
    ///
    /// `VsLimp` 回傳 `None`——來源表沒有面對跛入的欄位（使用說明【7】只
    /// 提到隔離跛入可用 8 倍尺寸，沒有給範圍），那些節點仍走參數產生器。
    #[must_use]
    pub const fn from_preflop(scenario: PreflopScenario) -> Option<Self> {
        match scenario {
            PreflopScenario::Unopened => Some(Self::Unopened),
            PreflopScenario::VsOpen { .. } => Some(Self::Open),
            PreflopScenario::VsOpenRaise { .. } => Some(Self::OpenRaise),
            PreflopScenario::VsThreeBet { .. } | PreflopScenario::VsSqueeze { .. } => {
                Some(Self::ThreeBet)
            }
            PreflopScenario::VsFourBet { .. } => Some(Self::FourBet),
            PreflopScenario::VsLimp { .. } => None,
        }
    }
}

/// 來源表的五種動作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChartAction {
    Fold,
    /// 跟到前方下注額。無人加注時大盲不需跟注，語意為過牌
    Call,
    /// 加注到前方最大下注額的 2.5 倍（標準尺寸）
    RaiseSmall,
    /// 加注到前方最大下注額的 8 倍（只用於擠壓或隔離跛入）
    RaiseLarge,
    AllIn,
}

impl ChartAction {
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [
            Self::Fold,
            Self::Call,
            Self::RaiseSmall,
            Self::RaiseLarge,
            Self::AllIn,
        ]
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fold => "fold",
            Self::Call => "call",
            Self::RaiseSmall => "raise-2.5x",
            Self::RaiseLarge => "raise-8x",
            Self::AllIn => "allin",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fold => "蓋牌",
            Self::Call => "跟注",
            Self::RaiseSmall => "加注（前方 2.5 倍）",
            Self::RaiseLarge => "加注（前方 8 倍）",
            Self::AllIn => "ALL IN",
        }
    }

    /// 是否為主動行動（加注或推入）。
    #[must_use]
    pub const fn is_aggressive(self) -> bool {
        matches!(self, Self::RaiseSmall | Self::RaiseLarge | Self::AllIn)
    }

    #[must_use]
    pub fn from_key(text: &str) -> Option<Self> {
        Self::all().into_iter().find(|a| a.as_str() == text)
    }
}

/// 各情境的兩種加注尺度，以 BB 的百分之一表示。
///
/// 來源表使用說明【2】：倍數乘的是**前方最大下注額**，不是大盲。
/// 前方額度依情境固定為 1／2.5／7.5／9／22 BB，因此展開後如下。
/// 把它寫死在內容層而不是由引擎現算，是因為這些倍數是內容的一部分：
/// 顧問改尺寸時改的是這張表，不是引擎的算式。
#[must_use]
pub const fn raise_size_centi_bb(scenario: ChartScenario, action: ChartAction) -> Option<u32> {
    let (small, large) = match scenario {
        // 前方 1BB
        ChartScenario::Unopened => (250, 800),
        // 前方 2.5BB
        ChartScenario::Open => (625, 2_000),
        // 前方 7.5BB
        ChartScenario::OpenRaise => (1_875, 6_000),
        // 前方約 9BB
        ChartScenario::ThreeBet => (2_250, 7_200),
        // 前方約 22BB
        ChartScenario::FourBet => (5_500, 17_600),
    };
    match action {
        ChartAction::RaiseSmall => Some(small),
        ChartAction::RaiseLarge => Some(large),
        _ => None,
    }
}

/// 某桌型在這份內容裡用到的位置序列。
///
/// 來源表只有 9MAX；少人數桌**刪位置**：8 人刪 UTG+2，7 人再刪 UTG+1，
/// 6 人再刪 LJ。刪掉的是最早的幾個位置，剩下的位置沿用 9MAX 那一欄的
/// 內容，不重新產表。
///
/// 結果必須與 [`crate::strategy::preflop::positions_for`] 完全一致——
/// 那是引擎決策時用的位置序列，兩者漂移的話面板畫的與 Bot 打的就不是
/// 同一個節點。`兩套位置序列必須一致` 測試把它們釘在一起。
#[must_use]
pub fn chart_positions(seated: u8) -> Vec<PositionLabel> {
    use PositionLabel::{Bb, Btn, Co, Hj, Lj, Sb, Utg, Utg1, Utg2};
    const NINE_MAX: [PositionLabel; 9] = [Utg, Utg1, Utg2, Lj, Hj, Co, Btn, Sb, Bb];
    let removed: &[PositionLabel] = match seated {
        9 => &[],
        8 => &[Utg2],
        7 => &[Utg1, Utg2],
        6 => &[Utg1, Utg2, Lj],
        _ => return Vec::new(),
    };
    NINE_MAX
        .into_iter()
        .filter(|p| !removed.contains(p))
        .collect()
}


// ── 人格位移 ────────────────────────────────────────────────────────────

/// 以表為基準的邊界位移。
///
/// # 為什麼需要這一層
///
/// 這張表是**純策略**：每一類恰好落在一個動作上（來源表使用說明【4】）。
/// Bot 的人格參數卻是「權重縮放」——把 100% 的加注乘上 1.5 再正規化仍然
/// 是 100%，參數等於沒有作用。內容換成這張表之後，`preflopAggression`、
/// `callPersistence`、`foldDiscipline` 三支滑桿會一起變成裝飾品。
///
/// 因此人格在**內容層**作用：把 equity 排序上最邊緣的幾手牌搬到隔壁的
/// 動作，而不是去乘一個純策略的權重。
///
/// # 不會重複套用
///
/// 管線第 2 步的人格階段仍然存在，但它作用在**權重**上：表產生的分佈
/// 只有一個項目，乘上倍率再正規化不變，那一步自然是空操作。表涵蓋不到
/// 的節點（面對跛入）反過來——位移不作用，管線照舊。兩條路徑都恰好
/// 套用一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChartShift {
    /// 進池範圍（主動＋被動）的寬度倍率
    pub range_width: Myriad,
    /// 主動帶的倍率：把跟注邊緣的牌改成加注
    pub aggression: Myriad,
    /// 被動帶的倍率：越高越黏
    pub call_persistence: Myriad,
    /// 被動帶的除數：越高越常棄牌（與跟注黏著度反向）
    pub fold_discipline: Myriad,
}

impl ChartShift {
    /// 完全照表。
    pub const NEUTRAL: Self = Self {
        range_width: FULL,
        aggression: FULL,
        call_persistence: FULL,
        fold_discipline: FULL,
    };

    /// 是否為「照表打」——四個倍率都是 100%。
    #[must_use]
    pub const fn is_neutral(self) -> bool {
        self.range_width == FULL
            && self.aggression == FULL
            && self.call_persistence == FULL
            && self.fold_discipline == FULL
    }
}

impl Default for ChartShift {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

fn scale(value: u32, multiplier: Myriad) -> u32 {
    u32::try_from(u64::from(value) * u64::from(multiplier) / u64::from(FULL)).unwrap_or(u32::MAX)
}

// ── 內容 ────────────────────────────────────────────────────────────────

/// 一列的手牌選擇方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selector {
    /// 「無」——此動作在這一格不使用
    None,
    /// 「其餘手牌」——同組其他四個動作沒收走的
    Rest,
    /// 「全部手牌」
    All,
    /// 逐手列出
    List,
}

/// 一格中的一個動作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartRow {
    pub action: ChartAction,
    selector: Selector,
    classes: Vec<HandClass>,
    /// 來源表 H 欄的原文。面板逐格顯示，因此不得在載入時丟掉
    pub note: String,
}

impl ChartRow {
    /// 這一列涵蓋的牌類。「其餘手牌」已在載入時展開，因此這裡恆為實值。
    #[must_use]
    pub fn classes(&self) -> &[HandClass] {
        &self.classes
    }

    /// 這一列涵蓋的 combo 數。
    #[must_use]
    pub fn combos(&self) -> u32 {
        self.classes.iter().map(|c| u32::from(c.combos())).sum()
    }

    /// 佔 1,326 個 combo 的比例（萬分比）。
    #[must_use]
    pub fn share_myriad(&self) -> u32 {
        self.combos() * FULL / TOTAL_COMBOS
    }

    /// 這一列在來源表上是不是「無」。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.selector, Selector::None)
    }

    /// 來源表怎麼寫這一列：`-` 無、`*` 其餘手牌、`+` 全部手牌、`list` 逐手列出。
    ///
    /// 快照要用得到：`*` 與 `+` 那兩列展開後動輒上百個牌類，逐一存進每個
    /// run 的 manifest 會讓紀錄膨脹好幾倍，而它們本來就是可由其餘四列
    /// 推回來的餘數。
    #[must_use]
    pub const fn selector_key(&self) -> &'static str {
        match self.selector {
            Selector::None => "-",
            Selector::Rest => "*",
            Selector::All => "+",
            Selector::List => "list",
        }
    }
}

/// 一格 =（深度 × 位置 × 情境），底下五個動作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartEntry {
    pub depth: ChartDepth,
    pub position: PositionLabel,
    pub scenario: ChartScenario,
    rows: Vec<ChartRow>,
    /// 每一類對應到哪個動作。`None` 代表這一格在牌桌上不可能發生
    by_class: Vec<Option<ChartAction>>,
}

impl ChartEntry {
    /// 這一格在牌桌上是否可能發生。
    ///
    /// 來源表使用說明【6】列出 16 組不可能的組合（UTG 的 OPEN／
    /// OPEN-RAISE／4B 與 UTG+1 的 OPEN-RAISE），五個動作全為「無」。
    #[must_use]
    pub fn reachable(&self) -> bool {
        self.by_class.iter().any(Option::is_some)
    }

    /// 這一類該打什麼。不可能發生的格回傳 `None`。
    #[must_use]
    pub fn action_of(&self, class: HandClass) -> Option<ChartAction> {
        self.by_class.get(class.index()).copied().flatten()
    }

    #[must_use]
    pub fn rows(&self) -> &[ChartRow] {
        &self.rows
    }

    #[must_use]
    pub fn row(&self, action: ChartAction) -> &ChartRow {
        self.rows
            .iter()
            .find(|r| r.action == action)
            .expect("每一格恆有五個動作")
    }

    /// 主動範圍寬度：以 combo 加權（萬分比）。
    #[must_use]
    pub fn aggressive_share_myriad(&self) -> u32 {
        self.combos_where(ChartAction::is_aggressive) * FULL / TOTAL_COMBOS
    }

    /// 進池範圍寬度（主動＋被動）：以 combo 加權（萬分比）。
    #[must_use]
    pub fn entering_share_myriad(&self) -> u32 {
        self.combos_where(|a| a.is_aggressive() || matches!(a, ChartAction::Call)) * FULL
            / TOTAL_COMBOS
    }

    fn combos_where(&self, keep: impl Fn(ChartAction) -> bool) -> u32 {
        self.rows
            .iter()
            .filter(|r| keep(r.action))
            .map(ChartRow::combos)
            .sum()
    }

    /// 這一格的主要主動動作：combo 數最多的那一個。
    ///
    /// 人格位移把牌搬進主動帶時要指定動作。取「表上用得最多的那一個」
    /// 而不是固定用加注：0–15BB 的內容整欄是 ALL IN，搬進去卻標成加注
    /// 的話，那個位移產生的行動根本不在該深度的內容裡。
    #[must_use]
    pub fn primary_aggressive(&self) -> Option<ChartAction> {
        self.rows
            .iter()
            .filter(|r| r.action.is_aggressive() && !r.classes.is_empty())
            .max_by_key(|r| r.combos())
            .map(|r| r.action)
    }
}

/// 一份載入完成的預設組合表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultChart {
    pub format: u32,
    /// 來源檔名，供面板標示內容出處
    pub source: String,
    entries: BTreeMap<(ChartDepth, PositionLabel, ChartScenario), ChartEntry>,
}

impl DefaultChart {
    /// 隨程式編譯進去的那一份。
    ///
    /// 解析結果快取：面板 D 的矩陣請求跑在主執行緒上，每次重解 900 列
    /// 會讓拖動滑桿變成一連串卡頓。
    ///
    /// # Errors
    /// 資產解析失敗時回傳原因。
    pub fn embedded() -> Result<&'static Self, &'static ChartError> {
        static LOADED: OnceLock<Result<DefaultChart, ChartError>> = OnceLock::new();
        LOADED.get_or_init(|| Self::decode(EMBEDDED)).as_ref()
    }

    /// 全部格子，依（深度 × 位置 × 情境）遞增。
    #[must_use]
    pub fn entries(&self) -> Vec<&ChartEntry> {
        self.entries.values().collect()
    }

    #[must_use]
    pub fn entry(
        &self,
        depth: ChartDepth,
        position: PositionLabel,
        scenario: ChartScenario,
    ) -> Option<&ChartEntry> {
        self.entries.get(&(depth, position, scenario))
    }

    /// 引擎節點 → 表上的一格。
    ///
    /// 回傳 `None` 有三種情形，呼叫端一律落回參數產生器：
    /// 1. 該情境不在表上（面對跛入）；
    /// 2. 該位置不屬於這個桌型；
    /// 3. 該格在來源表上五個動作全為「無」（不可能發生的組合）。
    #[must_use]
    pub fn lookup(&self, node: &PreflopNode) -> Option<&ChartEntry> {
        let scenario = ChartScenario::from_preflop(node.scenario)?;
        if !chart_positions(node.seated).contains(&node.hero) {
            return None;
        }
        let entry = self.entry(ChartDepth::from_bucket(node.bucket), node.hero, scenario)?;
        entry.reachable().then_some(entry)
    }

    /// 這個節點的內容是不是由表提供。
    ///
    /// 校準工具用得到：參數推不動由表提供的格子，顧問的意見在這種節點上
    /// 只能落成逐格覆寫，或回頭改表本身。
    #[must_use]
    pub fn covers(&self, node: &PreflopNode) -> bool {
        self.lookup(node).is_some()
    }

    /// 某節點某牌類該打什麼，已套用人格位移。
    ///
    /// `shift` 為 [`ChartShift::NEUTRAL`] 時逐格照表回傳，不經任何重排——
    /// 表上的三個帶本來就不是 equity 排序的連續區段，照排序重建會在
    /// **參數全為預設**的情況下就改掉顧問的內容。
    ///
    /// 非中性時只搬邊界：先把主動帶調到目標寬度（不足時從被動帶最強的
    /// 幾手補、超出時把最弱的幾手退到被動帶），再同樣調被動帶。
    #[must_use]
    pub fn action_of(
        &self,
        node: &PreflopNode,
        class: HandClass,
        ranking: &EquityRanking,
        shift: ChartShift,
    ) -> Option<ChartAction> {
        let entry = self.lookup(node)?;
        let base = entry.action_of(class)?;
        if shift.is_neutral() {
            return Some(base);
        }
        Some(shifted_action(entry, class, ranking, shift).unwrap_or(base))
    }

    /// 某節點某牌類的行動分佈。
    ///
    /// 表是**純策略**：每一類恰好落在一個動作上，因此分佈永遠是單一項目
    /// 的 100%。這是內容的性質而不是簡化——來源表使用說明【4】明講
    /// 「五個動作的手牌互不重疊，同一手牌只會出現在一個動作裡」。
    #[must_use]
    pub fn distribution_for(
        &self,
        node: &PreflopNode,
        class: HandClass,
        ranking: &EquityRanking,
        shift: ChartShift,
        big_blind: Chips,
    ) -> Option<ActionDistribution> {
        let scenario = self.lookup(node)?.scenario;
        let action = self.action_of(node, class, ranking, shift)?;
        ActionDistribution::new(vec![(to_engine_action(scenario, action, big_blind), FULL)]).ok()
    }
}

/// 表上的動作 → 引擎的行動。
///
/// 「跟注」在**無人加注**的情境是過牌：大盲已投入，前方沒有下注可跟。
/// 送出 `Call` 的話會在 legal mask 被整段清掉（`call_to` 為 `None`），
/// 分佈歸零、Bot 掉進 fallback——那不是策略，是接線失誤。
#[must_use]
pub fn to_engine_action(
    scenario: ChartScenario,
    action: ChartAction,
    big_blind: Chips,
) -> Action {
    match action {
        ChartAction::Fold => Action::Fold,
        ChartAction::Call if matches!(scenario, ChartScenario::Unopened) => Action::Check,
        ChartAction::Call => Action::Call,
        ChartAction::AllIn => Action::AllIn,
        ChartAction::RaiseSmall | ChartAction::RaiseLarge => {
            let centi = raise_size_centi_bb(scenario, action).unwrap_or(250);
            Action::RaiseTo(centi_bb_to_chips(centi, big_blind))
        }
    }
}


/// 把一格的三個帶調到人格指定的寬度，回傳指定牌類調整後的動作。
///
/// 演算法只搬**邊界**：以 equity 排序為序，主動帶不足就從被動帶最強的
/// 幾手補上、超出就把最弱的幾手退回被動帶；被動帶再對棄牌帶做同樣的事。
/// 整份重排是行不通的——顧問的三個帶不是 equity 排序的連續區段，重排會
/// 把「表上有但排序偏後」的牌整批丟掉。
fn shifted_action(
    entry: &ChartEntry,
    target: HandClass,
    ranking: &EquityRanking,
    shift: ChartShift,
) -> Option<ChartAction> {
    // 由強到弱。equity 百分位越小越強
    let mut order = HandClass::all();
    order.sort_by_key(|&c| ranking.percentile_myriad(c));

    let band_of = |class: HandClass| -> u8 {
        match entry.action_of(class) {
            Some(action) if action.is_aggressive() => 0,
            Some(ChartAction::Call) => 1,
            _ => 2,
        }
    };
    let mut bands: Vec<u8> = HandClass::all().iter().map(|&c| band_of(c)).collect();
    let combos = |bands: &[u8], band: u8| -> u32 {
        HandClass::all()
            .iter()
            .filter(|c| bands[c.index()] == band)
            .map(|c| u32::from(c.combos()))
            .sum()
    };

    let aggressive_now = combos(&bands, 0);
    let passive_now = combos(&bands, 1);
    let entering = aggressive_now + passive_now;

    // 進池總寬先套 rangeWidth，主動帶再套侵略性；侵略性推的是主動與被動
    // 的界，不是把整個範圍撐大——「把跟注權重移往加注」就是這個意思
    let entering_target = scale(entering, shift.range_width).min(TOTAL_COMBOS);
    let aggressive_target = scale(aggressive_now, shift.aggression).min(entering_target);
    // 跟注黏著度與棄牌紀律反向作用在被動帶上
    let passive_base = entering_target.saturating_sub(aggressive_target);
    let passive_target = u32::try_from(
        u64::from(scale(passive_base, shift.call_persistence))
            * u64::from(FULL)
            / u64::from(shift.fold_discipline.max(1)),
    )
    .unwrap_or(u32::MAX)
    .min(TOTAL_COMBOS.saturating_sub(aggressive_target));

    rebalance(&mut bands, &order, 0, 1, aggressive_target);
    rebalance(&mut bands, &order, 1, 2, passive_target);

    match bands[target.index()] {
        0 => entry.primary_aggressive().or(Some(ChartAction::RaiseSmall)),
        1 => Some(ChartAction::Call),
        _ => Some(ChartAction::Fold),
    }
}

/// 把 `band` 調到 `target` 個 combo，多退少補都只動與 `neighbour` 相鄰的邊界。
///
/// `order` 為由強到弱；補牌取最強的、退牌退最弱的。
fn rebalance(bands: &mut [u8], order: &[HandClass], band: u8, neighbour: u8, target: u32) {
    let width = |bands: &[u8]| -> u32 {
        order
            .iter()
            .filter(|c| bands[c.index()] == band)
            .map(|c| u32::from(c.combos()))
            .sum()
    };

    // 不足：由強到弱從鄰帶補進來
    let mut current = width(bands);
    if current < target {
        for &class in order {
            if current >= target {
                break;
            }
            if bands[class.index()] == neighbour {
                bands[class.index()] = band;
                current += u32::from(class.combos());
            }
        }
        return;
    }
    // 過寬：由弱到強退到鄰帶。退到剛好不低於目標為止，
    // 退過頭會讓 70% 的設定實際打成 50%
    for &class in order.iter().rev() {
        if current <= target {
            break;
        }
        if bands[class.index()] == band {
            let after = current - u32::from(class.combos());
            if after < target {
                break;
            }
            bands[class.index()] = neighbour;
            current = after;
        }
    }
}

// ── 解析 ────────────────────────────────────────────────────────────────

/// 載入資產時的失敗原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartError {
    Empty,
    Malformed { line: usize, reason: String },
    MissingHeader(&'static str),
    UnsupportedFormat { found: u32, expected: u32 },
    Checksum { found: u64, expected: u64 },
    /// 某一格的五個動作不齊，或同一動作出現兩次
    IncompleteCell { key: String, reason: String },
    /// 同一格的兩個動作收了同一手牌
    Overlap { key: String, class: String },
    /// 「其餘手牌」補完後合計不是 1,326 個 combo
    NotExhaustive { key: String, combos: u32 },
}

impl std::fmt::Display for ChartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "預設組合表是空的"),
            Self::Malformed { line, reason } => write!(f, "第 {line} 行無法解析：{reason}"),
            Self::MissingHeader(name) => write!(f, "缺少表頭欄位 {name}"),
            Self::UnsupportedFormat { found, expected } => {
                write!(f, "格式版本 {found} 不是本版支援的 {expected}")
            }
            Self::Checksum { found, expected } => {
                write!(f, "校驗碼 {found:#018x} 與內容算出的 {expected:#018x} 不符")
            }
            Self::IncompleteCell { key, reason } => write!(f, "{key} 不完整：{reason}"),
            Self::Overlap { key, class } => write!(f, "{key} 的 {class} 落在兩個動作上"),
            Self::NotExhaustive { key, combos } => {
                write!(f, "{key} 補完後只有 {combos} 個 combo，不是 1326")
            }
        }
    }
}

type RawCell = Vec<(ChartAction, Selector, Vec<HandClass>, String)>;

impl DefaultChart {
    /// 解析資產文字。
    ///
    /// # Errors
    /// 見 [`ChartError`]。
    pub fn decode(text: &str) -> Result<Self, ChartError> {
        let mut format: Option<u32> = None;
        let mut source: Option<String> = None;
        let mut declared: Option<u64> = None;
        // 依出現順序累積；同一格的五列必須連在一起才算完整，因此先收再組
        let mut raw: Vec<((ChartDepth, PositionLabel, ChartScenario), RawCell)> = Vec::new();

        for (offset, line_text) in text.lines().enumerate() {
            let line = offset + 1;
            let content = line_text.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            if !content.contains('|') {
                let (key, value) = content
                    .split_once(char::is_whitespace)
                    .ok_or_else(|| ChartError::Malformed {
                        line,
                        reason: format!("「{content}」不是「鍵 值」的形狀"),
                    })?;
                match key {
                    "format" => format = Some(parse_u32(value.trim(), line)?),
                    "source" => source = Some(value.trim().to_owned()),
                    "checksum" => declared = Some(parse_hex(value.trim(), line)?),
                    other => {
                        return Err(ChartError::Malformed {
                            line,
                            reason: format!("未知的表頭欄位：{other}"),
                        })
                    }
                }
                continue;
            }

            let fields: Vec<&str> = content.splitn(6, '|').collect();
            if fields.len() != 6 {
                return Err(ChartError::Malformed {
                    line,
                    reason: format!("應有 6 個欄位，實得 {}", fields.len()),
                });
            }
            let depth = ChartDepth::from_key(fields[0]).ok_or_else(|| ChartError::Malformed {
                line,
                reason: format!("未知的深度：{}", fields[0]),
            })?;
            let position = parse_position(fields[1]).ok_or_else(|| ChartError::Malformed {
                line,
                reason: format!("未知的位置：{}", fields[1]),
            })?;
            let scenario =
                ChartScenario::from_key(fields[2]).ok_or_else(|| ChartError::Malformed {
                    line,
                    reason: format!("未知的情境：{}", fields[2]),
                })?;
            let action = ChartAction::from_key(fields[3]).ok_or_else(|| ChartError::Malformed {
                line,
                reason: format!("未知的動作：{}", fields[3]),
            })?;
            let (selector, classes) = parse_hands(fields[4], line)?;
            let note = fields[5].trim().to_owned();

            let key = (depth, position, scenario);
            match raw.last_mut() {
                Some((existing, cell)) if *existing == key => {
                    cell.push((action, selector, classes, note));
                }
                _ => raw.push((key, vec![(action, selector, classes, note)])),
            }
        }

        let format = format.ok_or(ChartError::MissingHeader("format"))?;
        if format != ASSET_FORMAT {
            return Err(ChartError::UnsupportedFormat {
                found: format,
                expected: ASSET_FORMAT,
            });
        }
        let source = source.ok_or(ChartError::MissingHeader("source"))?;
        let declared = declared.ok_or(ChartError::MissingHeader("checksum"))?;
        if raw.is_empty() {
            return Err(ChartError::Empty);
        }

        let mut entries = BTreeMap::new();
        for (key, cell) in raw {
            let entry = build_entry(key, cell)?;
            entries.insert(key, entry);
        }

        let chart = Self {
            format,
            source,
            entries,
        };
        let actual = chart.checksum();
        if actual != declared {
            return Err(ChartError::Checksum {
                found: declared,
                expected: actual,
            });
        }
        Ok(chart)
    }

    /// FNV-1a 走過解析後的內容。
    ///
    /// 算在數值與說明文字上而不是原始位元組：註解、欄位間距與換行因此
    /// 都不參與，Windows 端 checkout 出 CRLF 也不會讓資產突然載不進來。
    /// 必須與 `tools/preflop_chart_from_xlsx.py` 的同名函式逐位相同。
    #[must_use]
    fn checksum(&self) -> u64 {
        let mut hash = feed(FNV_OFFSET, u64::from(ASSET_FORMAT));
        for depth in ChartDepth::all() {
            for (position_index, position) in chart_positions(9).into_iter().enumerate() {
                for scenario in ChartScenario::all() {
                    let Some(entry) = self.entry(depth, position, scenario) else {
                        continue;
                    };
                    for (action_index, action) in ChartAction::all().into_iter().enumerate() {
                        let row = entry.row(action);
                        hash = feed(hash, depth_index(depth));
                        hash = feed(hash, u64::try_from(position_index).unwrap_or(0));
                        hash = feed(hash, scenario_index(scenario));
                        hash = feed(hash, u64::try_from(action_index).unwrap_or(0));
                        hash = feed(hash, selector_code(row.selector));
                        // 「其餘手牌」在載入時已展開，校驗碼只走原始清單，
                        // 否則 Python 端與這裡算的東西不是同一份
                        if matches!(row.selector, Selector::List) {
                            let mut indices: Vec<u64> = row
                                .classes
                                .iter()
                                .map(|c| u64::try_from(c.index()).unwrap_or(0))
                                .collect();
                            indices.sort_unstable();
                            for index in indices {
                                hash = feed(hash, index);
                            }
                        }
                        hash = feed_bytes(hash, row.note.as_bytes());
                    }
                }
            }
        }
        hash
    }
}

fn build_entry(
    key: (ChartDepth, PositionLabel, ChartScenario),
    cell: RawCell,
) -> Result<ChartEntry, ChartError> {
    let (depth, position, scenario) = key;
    let label = format!("{}/{}/{}", depth.as_str(), position.as_str(), scenario.as_str());

    if cell.len() != ChartAction::all().len() {
        return Err(ChartError::IncompleteCell {
            key: label,
            reason: format!("應有 5 個動作，實得 {}", cell.len()),
        });
    }
    for action in ChartAction::all() {
        if cell.iter().filter(|(a, ..)| *a == action).count() != 1 {
            return Err(ChartError::IncompleteCell {
                key: label,
                reason: format!("「{}」不是恰好出現一次", action.label()),
            });
        }
    }

    // 先把逐手列出的四個動作攤到 169 格，「其餘手牌」與「全部手牌」
    // 才知道自己要收哪些
    let mut by_class: Vec<Option<ChartAction>> = vec![None; 169];
    for (action, selector, classes, _) in &cell {
        if !matches!(selector, Selector::List) {
            continue;
        }
        for &class in classes {
            if by_class[class.index()].is_some() {
                return Err(ChartError::Overlap {
                    key: label,
                    class: class.label(),
                });
            }
            by_class[class.index()] = Some(*action);
        }
    }

    let fills = cell
        .iter()
        .filter(|(_, selector, ..)| matches!(selector, Selector::Rest | Selector::All))
        .count();
    if fills > 1 {
        return Err(ChartError::IncompleteCell {
            key: label,
            reason: "同一格有兩個補滿用的選擇子".to_owned(),
        });
    }

    let listed = by_class.iter().filter(|slot| slot.is_some()).count();
    let mut rows = Vec::with_capacity(5);
    for action in ChartAction::all() {
        let (_, selector, classes, note) = cell
            .iter()
            .find(|(a, ..)| *a == action)
            .expect("已驗證五個動作齊全");
        let expanded: Vec<HandClass> = match selector {
            Selector::None => Vec::new(),
            Selector::List => classes.clone(),
            // 「其餘手牌」「全部手牌」收走所有還沒被認領的格子。
            // 兩者的差別只在來源表怎麼寫，展開結果都是補到滿
            Selector::Rest | Selector::All => {
                if matches!(selector, Selector::All) && listed > 0 {
                    return Err(ChartError::IncompleteCell {
                        key: label,
                        reason: "同時有「全部手牌」與逐手清單".to_owned(),
                    });
                }
                HandClass::all()
                    .into_iter()
                    .filter(|c| by_class[c.index()].is_none())
                    .collect()
            }
        };
        if matches!(selector, Selector::Rest | Selector::All) {
            for &class in &expanded {
                by_class[class.index()] = Some(action);
            }
        }
        rows.push(ChartRow {
            action,
            selector: *selector,
            classes: expanded,
            note: note.clone(),
        });
    }

    // 不可能發生的格子（五個動作全為「無」）例外：那 16 組本來就不該有牌
    let covered: u32 = rows.iter().map(ChartRow::combos).sum();
    let reachable = by_class.iter().any(Option::is_some);
    if reachable && covered != TOTAL_COMBOS {
        return Err(ChartError::NotExhaustive {
            key: label,
            combos: covered,
        });
    }

    Ok(ChartEntry {
        depth,
        position,
        scenario,
        rows,
        by_class,
    })
}

fn parse_hands(text: &str, line: usize) -> Result<(Selector, Vec<HandClass>), ChartError> {
    match text.trim() {
        "-" => Ok((Selector::None, Vec::new())),
        "*" => Ok((Selector::Rest, Vec::new())),
        "+" => Ok((Selector::All, Vec::new())),
        list => {
            let mut classes = Vec::new();
            for token in list.split(',') {
                let token = token.trim();
                if token.is_empty() {
                    continue;
                }
                let class = HandClass::from_label(token).ok_or_else(|| ChartError::Malformed {
                    line,
                    reason: format!("未知的牌類：{token}"),
                })?;
                classes.push(class);
            }
            if classes.is_empty() {
                return Err(ChartError::Malformed {
                    line,
                    reason: format!("「{list}」既不是選擇子也不是牌類清單"),
                });
            }
            Ok((Selector::List, classes))
        }
    }
}

fn parse_position(text: &str) -> Option<PositionLabel> {
    chart_positions(9).into_iter().find(|p| p.as_str() == text)
}

const fn depth_index(depth: ChartDepth) -> u64 {
    match depth {
        ChartDepth::Short => 0,
        ChartDepth::Medium => 1,
        ChartDepth::Standard => 2,
        ChartDepth::Deep => 3,
    }
}

const fn scenario_index(scenario: ChartScenario) -> u64 {
    match scenario {
        ChartScenario::Unopened => 0,
        ChartScenario::Open => 1,
        ChartScenario::OpenRaise => 2,
        ChartScenario::ThreeBet => 3,
        ChartScenario::FourBet => 4,
    }
}

const fn selector_code(selector: Selector) -> u64 {
    match selector {
        Selector::None => 0,
        Selector::Rest => 1,
        Selector::All => 2,
        Selector::List => 3,
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn feed(seed: u64, value: u64) -> u64 {
    feed_bytes(seed, &value.to_le_bytes())
}

fn feed_bytes(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn parse_u32(value: &str, line: usize) -> Result<u32, ChartError> {
    value.parse().map_err(|_| ChartError::Malformed {
        line,
        reason: format!("「{value}」不是整數"),
    })
}

fn parse_hex(value: &str, line: usize) -> Result<u64, ChartError> {
    let digits = value.strip_prefix("0x").unwrap_or(value);
    u64::from_str_radix(digits, 16).map_err(|_| ChartError::Malformed {
        line,
        reason: format!("「{value}」不是十六進位整數"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::preflop::{positions_for, scenarios_for};
    use crate::strategy::ranking::EquityRanking;

    fn chart() -> &'static DefaultChart {
        DefaultChart::embedded().expect("內建預設組合表必須載得進來")
    }

    fn ranking() -> EquityRanking {
        EquityRanking::compute(2, 800)
    }

    fn class(label: &str) -> HandClass {
        HandClass::from_label(label).expect("牌類存在")
    }

    fn node(seated: u8, hero: PositionLabel, bucket: StackBucket, scenario: PreflopScenario) -> PreflopNode {
        PreflopNode {
            seated,
            hero,
            bucket,
            scenario,
        }
    }

    #[test]
    fn 內建資產載得進來且格數與來源表相符() {
        let chart = chart();
        assert_eq!(chart.format, ASSET_FORMAT);
        assert_eq!(
            chart.entries().len(),
            4 * 9 * 5,
            "四檔深度 × 九個位置 × 五種情境"
        );
        assert!(chart.source.contains("9MAX"), "資產要說得出自己的來源");
    }

    /// 來源表使用說明【4】：五個動作的手牌互不重疊，合計 1,326 個 combo。
    ///
    /// 這是內容自洽的底線。任何一格漏牌或重複，Bot 在那個節點就會查到
    /// 空的分佈而掉進 fallback——那不是策略，是內容有洞。
    #[test]
    fn 每個可到達的格子都恰好涵蓋一千三百二十六個_combo() {
        for entry in chart().entries() {
            if !entry.reachable() {
                continue;
            }
            let covered: u32 = entry.rows().iter().map(ChartRow::combos).sum();
            assert_eq!(
                covered,
                1_326,
                "{}/{}/{} 的 combo 合計不是 1326",
                entry.depth.as_str(),
                entry.position.as_str(),
                entry.scenario.as_str()
            );
            for class in HandClass::all() {
                let hits = entry
                    .rows()
                    .iter()
                    .filter(|r| r.classes().contains(&class))
                    .count();
                assert_eq!(
                    hits,
                    1,
                    "{} 在 {}/{}/{} 落在 {hits} 個動作上",
                    class.label(),
                    entry.depth.as_str(),
                    entry.position.as_str(),
                    entry.scenario.as_str()
                );
            }
        }
    }

    /// 來源表使用說明【6】：16 組在牌桌上不可能發生的組合。
    #[test]
    fn 不可能發生的十六組全部標為不可到達() {
        let unreachable: Vec<_> = chart()
            .entries()
            .into_iter()
            .filter(|e| !e.reachable())
            .map(|e| (e.position, e.scenario))
            .collect();
        assert_eq!(unreachable.len(), 16, "四檔深度 × 四組不可能的組合");

        for depth in ChartDepth::all() {
            for scenario in [
                ChartScenario::Open,
                ChartScenario::OpenRaise,
                ChartScenario::FourBet,
            ] {
                assert!(
                    !chart()
                        .entry(depth, PositionLabel::Utg, scenario)
                        .expect("格子存在")
                        .reachable(),
                    "UTG 前方無人行動，{} 不可能發生",
                    scenario.as_str()
                );
            }
            assert!(
                !chart()
                    .entry(depth, PositionLabel::Utg1, ChartScenario::OpenRaise)
                    .expect("格子存在")
                    .reachable(),
                "UTG+1 前方只有一人，湊不出開牌＋再加注"
            );
        }
    }

    /// 少人數桌的位置序列必須與引擎決策時用的那一份完全相同。
    ///
    /// 顧問給的規則是「從 9MAX 刪位置」，引擎的規則是規則細則 8.4.1 的
    /// 標籤表。兩者現在恰好一致；漂移的話面板畫的與 Bot 打的就不是同一
    /// 個節點，而且不會有任何徵兆。
    #[test]
    fn 兩套位置序列必須一致() {
        for seated in 6u8..=9 {
            assert_eq!(
                chart_positions(seated),
                positions_for(seated),
                "{seated} 人桌的位置序列與引擎不一致"
            );
        }
        assert_eq!(chart_positions(5), Vec::new(), "桌型超出範圍時不得亂猜");
    }

    /// 九檔 bucket 都要對得到一檔深度，且越深對到的欄位不得倒退。
    #[test]
    fn 籌碼分檔對應到深度且單調不遞減() {
        let buckets = [
            StackBucket::VeryShort,
            StackBucket::Short,
            StackBucket::Medium,
            StackBucket::Deep,
            StackBucket::Deeper,
            StackBucket::Deepest,
            StackBucket::VeryDeep,
            StackBucket::UltraDeep,
            StackBucket::Unbounded,
        ];
        let mapped: Vec<ChartDepth> = buckets.into_iter().map(ChartDepth::from_bucket).collect();
        assert_eq!(mapped[0], ChartDepth::Short);
        assert_eq!(mapped[8], ChartDepth::Deep);
        for pair in mapped.windows(2) {
            assert!(pair[0] <= pair[1], "籌碼越深不得對應到更淺的欄位");
        }
    }

    /// 抽樣核對一格與來源表逐字相符。
    #[test]
    fn utg_短碼推入範圍與來源表相符() {
        let entry = chart()
            .entry(ChartDepth::Short, PositionLabel::Utg, ChartScenario::Unopened)
            .expect("格子存在");
        let shove = entry.row(ChartAction::AllIn);
        let labels: Vec<String> = shove.classes().iter().map(|c| c.label()).collect();
        assert_eq!(
            labels,
            vec![
                "AA", "KK", "QQ", "JJ", "TT", "99", "88", "77", "AKs", "AQs", "AJs", "ATs", "A9s",
                "KQs", "AKo", "AQo", "AJo"
            ]
        );
        // 其餘手牌全部棄牌，且百分比與來源表 G 欄一致（8.1%）
        assert_eq!(entry.action_of(class("66")), Some(ChartAction::Fold));
        // 108 個 combo，來源表 G 欄記為 8.1%
        assert_eq!(shove.combos(), 108);
        assert_eq!(shove.share_myriad(), 814);
        assert_eq!(entry.primary_aggressive(), Some(ChartAction::AllIn));
    }

    /// 大盲在無人加注時是**過牌**，不是跟注也不是棄牌。
    ///
    /// 送出 `Call` 會在 legal mask 被整段清掉（無需跟注時 `call_to` 為
    /// `None`），分佈歸零、Bot 掉進 fallback。
    #[test]
    fn 大盲無人加注時過牌而不是棄牌() {
        for depth in ChartDepth::all() {
            let entry = chart()
                .entry(depth, PositionLabel::Bb, ChartScenario::Unopened)
                .expect("格子存在");
            for class in HandClass::all() {
                assert_eq!(
                    entry.action_of(class),
                    Some(ChartAction::Call),
                    "{} 在大盲無人加注時應留下看翻牌",
                    class.label()
                );
            }
            assert_eq!(
                to_engine_action(ChartScenario::Unopened, ChartAction::Call, Chips::new(2)),
                Action::Check
            );
        }
    }

    /// 中性位移必須逐格照表，不得經過任何重排。
    #[test]
    fn 中性位移逐格照表() {
        let ranking = ranking();
        for seated in 6u8..=9 {
            for hero in positions_for(seated) {
                for scenario in scenarios_for(seated, hero) {
                    for bucket in [StackBucket::VeryShort, StackBucket::Deeper, StackBucket::Unbounded] {
                        let node = node(seated, hero, bucket, scenario);
                        let Some(entry) = chart().lookup(&node) else {
                            continue;
                        };
                        for class in HandClass::all() {
                            assert_eq!(
                                chart().action_of(&node, class, &ranking, ChartShift::NEUTRAL),
                                entry.action_of(class),
                                "{} 在 {} 被中性位移改掉了",
                                class.label(),
                                node.key()
                            );
                        }
                    }
                }
            }
        }
    }

    /// 範圍寬度必須真的改變進池範圍——這是人格層最重要的參數。
    #[test]
    fn 範圍寬度位移改變進池範圍() {
        let ranking = ranking();
        let node = node(
            9,
            PositionLabel::Btn,
            StackBucket::Deeper,
            PreflopScenario::Unopened,
        );
        let entering = |width: Myriad| {
            let shift = ChartShift {
                range_width: width,
                ..ChartShift::NEUTRAL
            };
            HandClass::all()
                .into_iter()
                .filter(|&c| {
                    !matches!(
                        chart().action_of(&node, c, &ranking, shift),
                        Some(ChartAction::Fold) | None
                    )
                })
                .map(|c| u32::from(c.combos()))
                .sum::<u32>()
        };
        let narrow = entering(7_000);
        let base = entering(FULL);
        let wide = entering(13_000);
        assert!(narrow < base, "70% 的進池範圍 {narrow} 應窄於基準 {base}");
        assert!(wide > base, "130% 的進池範圍 {wide} 應寬於基準 {base}");
    }

    /// 侵略性推的是主動與被動的界，不是把整個範圍撐大。
    #[test]
    fn 侵略性位移把跟注邊緣改成加注() {
        let ranking = ranking();
        // 這一格表上同時有加注帶與跟注帶
        let node = node(
            9,
            PositionLabel::Btn,
            StackBucket::Deeper,
            PreflopScenario::VsOpen {
                opener: PositionLabel::Co,
            },
        );
        let entry = chart().lookup(&node).expect("此節點由表提供");
        assert!(
            entry.row(ChartAction::Call).combos() > 0,
            "這一格必須有跟注帶，否則測不到界的移動"
        );

        let aggressive = |value: Myriad| {
            let shift = ChartShift {
                aggression: value,
                ..ChartShift::NEUTRAL
            };
            HandClass::all()
                .into_iter()
                .filter(|&c| {
                    chart()
                        .action_of(&node, c, &ranking, shift)
                        .is_some_and(ChartAction::is_aggressive)
                })
                .map(|c| u32::from(c.combos()))
                .sum::<u32>()
        };
        assert!(
            aggressive(15_000) > aggressive(FULL),
            "侵略性 150% 應把跟注邊緣的牌改成加注"
        );
    }

    /// 棄牌紀律與跟注黏著度反向作用在被動帶上。
    #[test]
    fn 棄牌紀律與跟注黏著度反向() {
        let ranking = ranking();
        let node = node(
            9,
            PositionLabel::Btn,
            StackBucket::Deeper,
            PreflopScenario::VsOpen {
                opener: PositionLabel::Co,
            },
        );
        let calls = |shift: ChartShift| {
            HandClass::all()
                .into_iter()
                .filter(|&c| {
                    matches!(
                        chart().action_of(&node, c, &ranking, shift),
                        Some(ChartAction::Call)
                    )
                })
                .map(|c| u32::from(c.combos()))
                .sum::<u32>()
        };
        let base = calls(ChartShift::NEUTRAL);
        let sticky = calls(ChartShift {
            call_persistence: 15_000,
            ..ChartShift::NEUTRAL
        });
        let disciplined = calls(ChartShift {
            fold_discipline: 15_000,
            ..ChartShift::NEUTRAL
        });
        assert!(sticky > base, "跟注黏著度越高，跟注帶越寬");
        assert!(disciplined < base, "棄牌紀律越高，跟注帶越窄");
    }

    /// 引擎列舉得出來的節點，除了「面對跛入」之外都要查得到內容。
    #[test]
    fn 除了面對跛入之外的節點都由表提供() {
        let mut uncovered = Vec::new();
        for seated in 6u8..=9 {
            for hero in positions_for(seated) {
                for scenario in scenarios_for(seated, hero) {
                    let node = node(seated, hero, StackBucket::Deeper, scenario);
                    if chart().covers(&node) {
                        continue;
                    }
                    uncovered.push((node.key(), scenario));
                }
            }
        }
        assert!(
            uncovered
                .iter()
                .all(|(_, s)| matches!(s, PreflopScenario::VsLimp { .. })),
            "表未涵蓋的節點只應是面對跛入：{uncovered:?}"
        );
        assert!(!uncovered.is_empty(), "面對跛入確實沒有內容，這一點要如實反映");
    }

    /// 校驗碼算在解析後的內容上，因此註解與換行不影響載入。
    #[test]
    fn 註解與_crlf_換行不影響載入() {
        let text = EMBEDDED.replace('\n', "\r\n");
        assert_eq!(DefaultChart::decode(&text).expect("CRLF 仍可載入"), *chart());

        let with_comment = format!("# 額外註解\n{EMBEDDED}");
        assert_eq!(DefaultChart::decode(&with_comment).expect("註解不影響"), *chart());
    }

    /// 手改一個牌類就要被校驗碼擋下。
    #[test]
    fn 改過一格就被校驗碼擋下() {
        let tampered = EMBEDDED.replacen("AJs, ATs, A9s, KQs,", "AJs, ATs, A8s, KQs,", 1);
        assert_ne!(tampered, EMBEDDED, "測試本身要真的改到東西");
        assert!(
            matches!(
                DefaultChart::decode(&tampered),
                Err(ChartError::Checksum { .. })
            ),
            "手改牌類必須被擋下"
        );
    }

    /// 說明文字也在校驗碼裡：那是面板逐格顯示的內容，被改掉一樣是內容損壞。
    #[test]
    fn 改過說明文字也被校驗碼擋下() {
        let tampered = EMBEDDED.replacen("此情境不跟注", "此情境可以跟注", 1);
        assert_ne!(tampered, EMBEDDED, "測試本身要真的改到東西");
        assert!(matches!(
            DefaultChart::decode(&tampered),
            Err(ChartError::Checksum { .. })
        ));
    }

    #[test]
    fn 格式版本不符時拒絕載入() {
        let bumped = EMBEDDED.replacen("format 1", "format 2", 1);
        assert!(matches!(
            DefaultChart::decode(&bumped),
            Err(ChartError::UnsupportedFormat { found: 2, .. })
        ));
    }

    #[test]
    fn 空檔案回報為空而不是恐慌() {
        assert_eq!(DefaultChart::decode(""), Err(ChartError::MissingHeader("format")));
    }

    /// 加注尺度以「前方最大下注額 × 倍數」展開（來源表使用說明【2】）。
    #[test]
    fn 加注尺度與來源表的倍數說明一致() {
        let cases = [
            (ChartScenario::Unopened, 250, 800),
            (ChartScenario::Open, 625, 2_000),
            (ChartScenario::OpenRaise, 1_875, 6_000),
            (ChartScenario::ThreeBet, 2_250, 7_200),
            (ChartScenario::FourBet, 5_500, 17_600),
        ];
        for (scenario, small, large) in cases {
            assert_eq!(
                raise_size_centi_bb(scenario, ChartAction::RaiseSmall),
                Some(small)
            );
            assert_eq!(
                raise_size_centi_bb(scenario, ChartAction::RaiseLarge),
                Some(large)
            );
            assert_eq!(raise_size_centi_bb(scenario, ChartAction::Fold), None);
        }
    }
}
