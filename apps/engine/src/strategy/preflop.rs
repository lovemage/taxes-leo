//! Preflop 策略節點與策略表。
//!
//! 核心規格 4.1：資料模型為 `node × handClass × action × size → frequency`，
//! 且 `node` 至少包含桌型人數、使用者位置、有效籌碼 bucket、ante、rake、
//! straddle 與**完整公開翻前 action history**。
//!
//! 「完整 action history」是內容體積爆炸的來源：`vs Open` 不是一個節點，
//! 而是「被 UTG 開牌」「被 CO 開牌」等多個不同節點。實做計劃 M0 閘門 2
//! 要求精算格數，[`enumerate_nodes`] 即為該精算的依據。

use crate::chips::Chips;
use crate::position::PositionLabel;
use crate::strategy::decision::StackBucket;

/// 對手 open 尺度的區間。
///
/// 面對 2.5BB 與 10BB 的 open 是不同的決策：開得越大，繼續範圍通常越緊。
/// 區間只決定**使用者覆寫落在哪一格**，預設內容（組合表／參數）不因區間
/// 而改變——引擎不自行發明「面對大 open 該收多緊」的內容，那是業主的
/// 策略判斷。未設定某區間時，該區間繼承標準區間的覆寫，再落回預設內容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OpenSizeTier {
    /// 標準 open（預設 ≤ 3BB）。節點鍵不帶後綴，與舊版覆寫相容
    Standard,
    /// 中型 open（預設 > 3BB 且 ≤ 6BB）
    Medium,
    /// 大型 open（預設 > 6BB）
    Large,
}

impl OpenSizeTier {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Medium, Self::Large];

    /// 序列化用的唯一字串。
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    #[must_use]
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.key() == key)
    }

    /// 節點鍵的後綴。標準區間為空字串：0.1.6 以前的 `vs-open-UTG` 覆寫
    /// 因此原封不動地落在標準區間，不會因為新增維度而悄悄失效。
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Standard => "",
            Self::Medium => "@open-medium",
            Self::Large => "@open-large",
        }
    }
}

/// open 尺度區間的邊界，以 BB 的百分之一表示（300 = 3BB）。
///
/// 左開右閉：`size ≤ medium_above` 為標準，`medium_above < size ≤
/// large_above` 為中型，其餘為大型。邊界可由使用者調整，因此存在規則集
/// 裡並隨 run 寫進快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenTierBounds {
    pub medium_above_centi_bb: u32,
    pub large_above_centi_bb: u32,
}

impl OpenTierBounds {
    /// 標準 2～3BB、中型超過 3～6BB、大型超過 6BB。
    pub const DEFAULT: Self = Self {
        medium_above_centi_bb: 300,
        large_above_centi_bb: 600,
    };

    /// 邊界是否合法：兩者都在 2～100BB 之內且嚴格遞增。
    ///
    /// # Errors
    /// 不合法時回傳說明。
    pub fn validate(self) -> Result<(), String> {
        let range = 200..=10_000;
        if !range.contains(&self.medium_above_centi_bb) || !range.contains(&self.large_above_centi_bb)
        {
            return Err("open 尺度區間邊界必須在 2～100 BB 之內".to_owned());
        }
        if self.medium_above_centi_bb >= self.large_above_centi_bb {
            return Err("中型區間的上限必須大於標準區間的上限".to_owned());
        }
        Ok(())
    }

    /// 依實際 open 金額（raise-to 總額）判定區間。
    ///
    /// 以整數交叉相乘比對，不先換算成 BB 再取整：3.004BB 的 open 在
    /// 3BB 邊界上若先取整，會被錯歸到標準區間。
    #[must_use]
    pub fn classify(self, open_to: Chips, big_blind: Chips) -> OpenSizeTier {
        let scaled = u128::from(open_to.units()) * 100;
        let bb = u128::from(big_blind.units().max(1));
        if scaled <= u128::from(self.medium_above_centi_bb) * bb {
            OpenSizeTier::Standard
        } else if scaled <= u128::from(self.large_above_centi_bb) * bb {
            OpenSizeTier::Medium
        } else {
            OpenSizeTier::Large
        }
    }
}

impl Default for OpenTierBounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// 翻前情境。對應 UI 規格 D.4 的節點情境清單。
///
/// 帶位置參數的變體是刻意的：核心規格 4.1 要求節點涵蓋完整公開行動史，
/// 因此「面對誰的開牌」屬於節點識別的一部分，不得合併。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PreflopScenario {
    /// 前面全部棄牌
    Unopened,
    /// 面對 n 名跛入者
    VsLimp { limpers: u8 },
    /// 面對某位置的開牌加注。
    ///
    /// `size` 是開牌尺度的區間（見 [`OpenSizeTier`]）。[`scenarios_for`]
    /// 與 [`enumerate_nodes`] 只列標準區間：其餘區間是標準節點的**切片**，
    /// 不計入節點總數與完整度，未設定時繼承標準區間。
    VsOpen {
        opener: PositionLabel,
        size: OpenSizeTier,
    },
    /// 面對「開牌＋再加注」：前方有人開牌又被另一人加注，英雄尚未下注。
    ///
    /// **與 [`Self::VsThreeBet`] 的差別是英雄自己有沒有下過注。** 這裡的
    /// 選項是冷 4-bet／冷跟／棄牌，底池賠率與所需範圍都與「自己開牌後被
    /// 3-bet」不同；混成同一個節點等於用錯一整欄內容。
    ///
    /// `opener` 是最初的開牌者。再加注者不進節點鍵——兩個位置都放進去會
    /// 讓節點數再乘一個維度，而顧問的表本來就沒有分到那麼細。
    VsOpenRaise { opener: PositionLabel },
    /// 英雄開牌後被某位置 3-bet
    VsThreeBet { by: PositionLabel },
    /// 英雄 3-bet 後被 4-bet
    VsFourBet { by: PositionLabel },
    /// 英雄開牌、有人跟注後被擠壓。
    ///
    /// **與 [`Self::VsThreeBet`] 的差別是中間那個跟注者。** 因此 `by` 必須與
    /// 英雄之間隔著至少一個座位；相鄰時無人能跟注，該情境不存在
    /// （見 [`scenarios_for`]）。
    VsSqueeze { by: PositionLabel },
}

impl PreflopScenario {
    /// 內容表的欄位鍵。
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Unopened => "unopened".to_owned(),
            Self::VsLimp { limpers } => format!("vs-limp-{limpers}"),
            Self::VsOpen { opener, size } => {
                format!("vs-open-{}{}", opener.as_str(), size.suffix())
            }
            Self::VsOpenRaise { opener } => format!("vs-open-raise-{}", opener.as_str()),
            Self::VsThreeBet { by } => format!("vs-3bet-{}", by.as_str()),
            Self::VsFourBet { by } => format!("vs-4bet-{}", by.as_str()),
            Self::VsSqueeze { by } => format!("vs-squeeze-{}", by.as_str()),
        }
    }

    /// 面對某位置的標準區間開牌。
    #[must_use]
    pub const fn vs_open(opener: PositionLabel) -> Self {
        Self::VsOpen {
            opener,
            size: OpenSizeTier::Standard,
        }
    }

    /// 情境種類的鍵，不含位置與區間。執行期命中統計依它分組。
    #[must_use]
    pub const fn kind_key(self) -> &'static str {
        match self {
            Self::Unopened => "unopened",
            Self::VsLimp { .. } => "vs-limp",
            Self::VsOpen { .. } => "vs-open",
            Self::VsOpenRaise { .. } => "vs-open-raise",
            Self::VsThreeBet { .. } => "vs-3bet",
            Self::VsFourBet { .. } => "vs-4bet",
            Self::VsSqueeze { .. } => "vs-squeeze",
        }
    }

    /// 面對開牌時的 open 尺度區間；其餘情境為 `None`。
    #[must_use]
    pub const fn open_tier(self) -> Option<OpenSizeTier> {
        match self {
            Self::VsOpen { size, .. } => Some(size),
            _ => None,
        }
    }

    /// 去掉 open 尺度區間後的基礎情境（區間一律歸回標準）。
    ///
    /// 可達性檢查與預設內容查表都以基礎情境為準：區間只是覆寫的切片。
    #[must_use]
    pub const fn base(self) -> Self {
        match self {
            Self::VsOpen { opener, .. } => Self::VsOpen {
                opener,
                size: OpenSizeTier::Standard,
            },
            other => other,
        }
    }

    /// 換成指定的 open 尺度區間。非面對開牌的情境原樣回傳。
    #[must_use]
    pub const fn with_open_tier(self, tier: OpenSizeTier) -> Self {
        match self {
            Self::VsOpen { opener, .. } => Self::VsOpen { opener, size: tier },
            other => other,
        }
    }
}

/// 一個翻前決策節點。
///
/// ante／rake／straddle 也是核心規格 4.1 的節點要素，但它們是**桌型層級**
/// 的設定而非逐節點變化，因此保存在策略表的 meta 而不重複進每個節點鍵，
/// 避免內容量再乘上一個維度。策略表綁定桌型（UI 規格 D.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreflopNode {
    pub seated: u8,
    pub hero: PositionLabel,
    pub bucket: StackBucket,
    pub scenario: PreflopScenario,
}

impl PreflopNode {
    /// 內容表的唯一鍵。
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{}max/{}/{}/{}",
            self.seated,
            self.hero.as_str(),
            self.bucket.as_str(),
            self.scenario.key()
        )
    }

    /// 去掉 open 尺度區間的基礎節點，見 [`PreflopScenario::base`]。
    #[must_use]
    pub const fn base(&self) -> Self {
        Self {
            scenario: self.scenario.base(),
            ..*self
        }
    }
}

/// 各桌型由早到晚的位置序列（規則細則 8.4.1，無 dead 位的情形）。
#[must_use]
pub fn positions_for(seated: u8) -> Vec<PositionLabel> {
    use PositionLabel::{Bb, Btn, Co, Hj, Lj, Sb, Utg, Utg1, Utg2};
    match seated {
        6 => vec![Utg, Hj, Co, Btn, Sb, Bb],
        7 => vec![Utg, Lj, Hj, Co, Btn, Sb, Bb],
        8 => vec![Utg, Utg1, Lj, Hj, Co, Btn, Sb, Bb],
        9 => vec![Utg, Utg1, Utg2, Lj, Hj, Co, Btn, Sb, Bb],
        _ => Vec::new(),
    }
}

/// 全部 9 個有效籌碼分檔（規則細則 8.5）。
#[must_use]
pub fn all_buckets() -> Vec<StackBucket> {
    vec![
        StackBucket::VeryShort,
        StackBucket::Short,
        StackBucket::Medium,
        StackBucket::Deep,
        StackBucket::Deeper,
        StackBucket::Deepest,
        StackBucket::VeryDeep,
        StackBucket::UltraDeep,
        StackBucket::Unbounded,
    ]
}

/// 列舉某桌型下英雄在某位置的所有合法翻前情境。
///
/// 合法性依行動順序判定，例如 UTG 不可能「面對 UTG 開牌」，
/// BB 不可能被更晚的位置開牌。列舉不合法節點會虛增內容量。
///
/// 擠壓另有一個容易漏掉的前提：擠壓者與英雄之間必須有座位可以先跟注。
/// 「UTG 被 UTG+1 擠壓」不存在，因為兩者之間沒有人能跟注，那個情境就是
/// 「UTG 被 UTG+1 3-bet」。非相鄰時兩者仍是不同節點——中間有人跟注會
/// 改變底池大小、賠率與對手範圍——因此只排除相鄰的情形。
#[must_use]
pub fn scenarios_for(seated: u8, hero: PositionLabel) -> Vec<PreflopScenario> {
    let order = positions_for(seated);
    let Some(hero_index) = order.iter().position(|&p| p == hero) else {
        return Vec::new();
    };
    let earlier = &order[..hero_index];
    let later = &order[hero_index + 1..];

    let mut out = vec![PreflopScenario::Unopened];

    // 面對跛入：需有更早的位置
    if !earlier.is_empty() {
        out.push(PreflopScenario::VsLimp { limpers: 1 });
        if earlier.len() >= 2 {
            out.push(PreflopScenario::VsLimp { limpers: 2 });
        }
    }

    // 面對開牌：開牌者必須在英雄之前
    for (index, &opener) in earlier.iter().enumerate() {
        out.push(PreflopScenario::VsOpen {
            opener,
            size: OpenSizeTier::Standard,
        });
        // 面對「開牌＋再加注」多一個前提：開牌者與英雄之間要有座位，
        // 那個座位才可能再加注。開牌者緊鄰英雄時無人能再加注，該情境
        // 到不了——這正是來源表把 UTG 與 UTG+1 的 OPEN-RAISE 整欄列為
        // 「無此情境」的原因（使用說明【6】）。
        if index + 1 < earlier.len() {
            out.push(PreflopScenario::VsOpenRaise { opener });
        }
    }

    // 英雄開牌後被 3-bet：3-bet 者必須在英雄之後
    for (gap, &by) in later.iter().enumerate() {
        out.push(PreflopScenario::VsThreeBet { by });
        // 擠壓比 3-bet 多一個前提：英雄與加注者之間至少要有一個座位，
        // 那個座位才可能先跟注。相鄰時無人能跟注，該節點到不了。
        // `gap` 為 `later` 內的索引，0 代表緊鄰英雄的下一個位置。
        if gap >= 1 {
            out.push(PreflopScenario::VsSqueeze { by });
        }
    }

    // 英雄 3-bet 後被 4-bet：原開牌者在英雄之前
    for &by in earlier {
        out.push(PreflopScenario::VsFourBet { by });
    }

    out
}

/// 列舉全部翻前節點（6～9 人 × 位置 × bucket × 情境）。
///
/// 這是實做計劃 M0 閘門 2 所需的「格數清單」依據：
/// 節點數 × 169 即為 baseline 的總格數。
#[must_use]
pub fn enumerate_nodes() -> Vec<PreflopNode> {
    let mut out = Vec::new();
    for seated in 6u8..=9 {
        for hero in positions_for(seated) {
            for scenario in scenarios_for(seated, hero) {
                for bucket in all_buckets() {
                    out.push(PreflopNode {
                        seated,
                        hero,
                        bucket,
                        scenario,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 各桌型的位置數正確() {
        for seated in 6u8..=9 {
            assert_eq!(
                positions_for(seated).len(),
                usize::from(seated),
                "{seated} 人桌應有 {seated} 個位置"
            );
        }
    }

    #[test]
    fn utg_不會面對更早位置的開牌() {
        let scenarios = scenarios_for(9, PositionLabel::Utg);
        assert!(
            !scenarios
                .iter()
                .any(|s| matches!(s, PreflopScenario::VsOpen { .. })),
            "UTG 是最早行動者，不可能面對開牌"
        );
        assert!(
            !scenarios
                .iter()
                .any(|s| matches!(s, PreflopScenario::VsLimp { .. })),
            "UTG 之前無人，不可能面對跛入"
        );
    }

    #[test]
    fn bb_不會被更晚位置_3bet() {
        let scenarios = scenarios_for(9, PositionLabel::Bb);
        assert!(
            !scenarios
                .iter()
                .any(|s| matches!(s, PreflopScenario::VsThreeBet { .. })),
            "BB 是最後行動者，其後無人可 3-bet"
        );
    }

    #[test]
    fn 相鄰位置不產生擠壓節點() {
        let scenarios = scenarios_for(9, PositionLabel::Utg);
        assert!(
            !scenarios.contains(&PreflopScenario::VsSqueeze {
                by: PositionLabel::Utg1
            }),
            "UTG 與 UTG+1 之間沒有座位可以跟注，該擠壓情境不存在"
        );
        assert!(
            scenarios.contains(&PreflopScenario::VsThreeBet {
                by: PositionLabel::Utg1
            }),
            "同一個位置的 3-bet 情境仍然成立，不可一併刪掉"
        );
    }

    #[test]
    fn 非相鄰的擠壓與_3bet_仍是兩個節點() {
        let scenarios = scenarios_for(9, PositionLabel::Utg);
        for by in [PositionLabel::Utg2, PositionLabel::Btn] {
            assert!(
                scenarios.contains(&PreflopScenario::VsSqueeze { by }),
                "{} 與 UTG 之間有座位可跟注，擠壓情境必須保留",
                by.as_str()
            );
            assert!(
                scenarios.contains(&PreflopScenario::VsThreeBet { by }),
                "中間有人跟注會改變底池與對手範圍，3-bet 與擠壓不得合併"
            );
        }
    }

    #[test]
    fn 全部桌型的擠壓者都與英雄隔著座位() {
        for seated in 6u8..=9 {
            let order = positions_for(seated);
            for (hero_index, &hero) in order.iter().enumerate() {
                for scenario in scenarios_for(seated, hero) {
                    let PreflopScenario::VsSqueeze { by } = scenario else {
                        continue;
                    };
                    let by_index = order
                        .iter()
                        .position(|&p| p == by)
                        .expect("擠壓者必須是本桌型的位置");
                    assert!(
                        by_index > hero_index + 1,
                        "{seated}max：{} 被 {} 擠壓時中間沒有座位可跟注",
                        hero.as_str(),
                        by.as_str()
                    );
                }
            }
        }
    }

    #[test]
    fn 節點鍵互異() {
        let nodes = enumerate_nodes();
        let mut keys: Vec<String> = nodes.iter().map(PreflopNode::key).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "節點鍵必須互異，否則內容會互相覆蓋");
    }

    #[test]
    fn open_區間邊界以交叉相乘判定且左開右閉() {
        let bounds = OpenTierBounds::DEFAULT;
        let bb = Chips::new(100);
        assert_eq!(bounds.classify(Chips::new(250), bb), OpenSizeTier::Standard);
        assert_eq!(bounds.classify(Chips::new(300), bb), OpenSizeTier::Standard);
        assert_eq!(bounds.classify(Chips::new(301), bb), OpenSizeTier::Medium);
        assert_eq!(bounds.classify(Chips::new(600), bb), OpenSizeTier::Medium);
        assert_eq!(bounds.classify(Chips::new(601), bb), OpenSizeTier::Large);
        assert_eq!(bounds.classify(Chips::new(1_000), bb), OpenSizeTier::Large);
        // BB=2 時 6 單位 = 3BB 恰好在邊界上，7 單位 = 3.5BB 屬中型
        assert_eq!(
            bounds.classify(Chips::new(6), Chips::new(2)),
            OpenSizeTier::Standard
        );
        assert_eq!(
            bounds.classify(Chips::new(7), Chips::new(2)),
            OpenSizeTier::Medium
        );
    }

    #[test]
    fn open_區間邊界必須遞增且在範圍內() {
        assert!(OpenTierBounds::DEFAULT.validate().is_ok());
        for (medium, large) in [(600, 600), (700, 600), (100, 600), (300, 20_000)] {
            assert!(
                OpenTierBounds {
                    medium_above_centi_bb: medium,
                    large_above_centi_bb: large,
                }
                .validate()
                .is_err(),
                "{medium}／{large} 應被拒絕"
            );
        }
    }

    #[test]
    fn 標準區間的鍵與舊版相同_其餘區間帶後綴() {
        let standard = PreflopScenario::VsOpen {
            opener: PositionLabel::Utg,
            size: OpenSizeTier::Standard,
        };
        assert_eq!(standard.key(), "vs-open-UTG");
        let large = standard.with_open_tier(OpenSizeTier::Large);
        assert_eq!(large.key(), "vs-open-UTG@open-large");
        assert_eq!(large.base(), standard);
        assert_eq!(large.open_tier(), Some(OpenSizeTier::Large));
        assert_eq!(
            PreflopScenario::Unopened.with_open_tier(OpenSizeTier::Large),
            PreflopScenario::Unopened
        );
    }

    #[test]
    fn 列舉只含標準區間() {
        assert!(enumerate_nodes()
            .iter()
            .all(|node| node.scenario.open_tier().is_none_or(|t| t == OpenSizeTier::Standard)));
    }

    #[test]
    fn 每個節點都含九個_bucket() {
        let nodes = enumerate_nodes();
        assert_eq!(
            nodes.len() % all_buckets().len(),
            0,
            "節點總數應為 bucket 數的整數倍"
        );
    }
}
