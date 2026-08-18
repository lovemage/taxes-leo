//! 參數化 preflop baseline 產生器。
//!
//! # 這是什麼
//!
//! 把 **727,038 格**的 preflop baseline 由**數十個可讀參數**展開而成。
//! 規則以「取 equity 排序的前 X%」表達，顧問調整的是 [`BaselineRules`]
//! 裡的數十個數字，審查的是規則與抽樣，不是逐格填寫。
//!
//! # 這不是什麼
//!
//! **不是 GTO 解，不是均衡策略。** 它是以 equity 排序為基礎的啟發式，
//! 供工程測試與作為顧問校準的起點。實做計劃強制規則 2：對外不得宣稱
//! 「GTO 正解」；「每批內容由牌手顧問簽核後才進引擎，未簽核不得作為
//! baseline 上線」。因此產生的內容一律標記 [`BaselineRules::version`]
//! 為工程佔位版本，寫入 `RunManifest` 後可與顧問簽核版本區分。
//!
//! # 已知的簡化（顧問校準時的優先修正對象）
//!
//! - **以 equity 排序代替可玩性**：raw equity 低估同花連牌的翻後可玩性、
//!   低估 A5s 這類有阻斷與堅果同花價值的牌、也低估 AKo 的支配優勢。
//! - **每情境單一加注尺度**：核心規格 4.1 的模型是 `action × size`，
//!   這裡每個情境只給一個尺度，尺度混合列為後續。
//! - **不含對手位置的細緻差異**：節點已依「面對誰」區分，但規則目前只用
//!   開牌者的位置早晚，未針對特定配對調整。
//! - **盲注位另立參數**：SB／BB 不套用「位置越晚越寬」，但兩者的實際策略
//!   （SB 的 limp／raise 混合、BB 的防守範圍）遠比單一寬度複雜，
//!   這是顧問校準的優先項目。

use crate::betting::Action;
use crate::chips::Chips;
use crate::strategy::decision::StackBucket;
use crate::strategy::distribution::{ActionDistribution, DistributionError, Myriad, FULL};
use crate::strategy::hand_class::HandClass;
use crate::position::PositionLabel;
use crate::strategy::preflop::{positions_for, PreflopNode, PreflopScenario};
use crate::strategy::ranking::EquityRanking;

/// 情境的範圍寬度參數。寬度以「equity 排序的前 X%」表示（萬分比）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioWidths {
    /// 最早位置的主動行動（開牌／加注）寬度
    pub aggressive_earliest: Myriad,
    /// 最晚位置的主動行動寬度。位置越晚越寬是撲克的基本結構
    pub aggressive_latest: Myriad,
    /// 跟注的追加寬度，接在主動範圍之後
    pub call_extra: Myriad,
    /// 邊界混合帶寬度。帶內的手牌以線性比例混合兩個行動，
    /// 避免產生「多一格就從 100% 加注跳到 100% 棄牌」的硬邊界
    pub mix_band: Myriad,
}

/// 完整規則集。這是顧問實際要調的東西——**數十個數字**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineRules {
    pub name: String,
    /// 內容版本。工程佔位版本必須可與顧問簽核版本區分
    pub version: String,
    /// 是否已由牌手顧問簽核。未簽核者不得作為出貨 baseline
    pub consultant_approved: bool,

    /// 非盲注位的情境寬度。位置越晚越寬只適用於這些位置
    pub unopened: ScenarioWidths,
    pub vs_limp: ScenarioWidths,
    pub vs_open: ScenarioWidths,
    pub vs_three_bet: ScenarioWidths,
    pub vs_four_bet: ScenarioWidths,
    pub vs_squeeze: ScenarioWidths,

    /// 各 bucket 對範圍寬度的乘數（萬分比，10000 = 不變）
    pub bucket_multiplier: [Myriad; 9],
    /// 低於此 bucket 一律推入或棄牌，不做小額加注
    pub push_fold_below: StackBucket,

    /// 盲注位的主動範圍寬度。
    ///
    /// **盲注位不套用「位置越晚越寬」**：SB／BB 在行動順序上最晚，
    /// 但翻後是最不利位置，把它們當成「最晚位置」會產生比 BTN 更寬的
    /// 開牌範圍，與現實相反。因此另立參數。
    pub sb_aggressive: Myriad,
    pub bb_aggressive: Myriad,

    /// 各情境的加注尺度，以 BB 的百分之一表示
    pub open_size_centi_bb: u32,
    pub three_bet_size_centi_bb: u32,
    pub four_bet_size_centi_bb: u32,
}

impl BaselineRules {
    /// 工程佔位版本的預設規則。
    ///
    /// 這些數字的來源是常見的緊兇結構（位置越晚越寬、面對加注需更強），
    /// **不是求解結果**。顧問校準時應逐項檢視。
    #[must_use]
    pub fn engineering_placeholder() -> Self {
        Self {
            name: "工程佔位 baseline（equity 排序）".to_owned(),
            version: "placeholder-v0".to_owned(),
            consultant_approved: false,

            // 開牌：UTG 約前 12%，BTN 約前 45%
            unopened: ScenarioWidths {
                aggressive_earliest: 1_200,
                aggressive_latest: 4_500,
                call_extra: 0,
                mix_band: 400,
            },
            // 面對跛入：可用隔離加注，範圍略寬於開牌
            vs_limp: ScenarioWidths {
                aggressive_earliest: 1_400,
                aggressive_latest: 4_000,
                call_extra: 800,
                mix_band: 400,
            },
            // 面對開牌：3-bet 範圍窄，另有跟注帶
            vs_open: ScenarioWidths {
                aggressive_earliest: 400,
                aggressive_latest: 1_100,
                call_extra: 1_200,
                mix_band: 300,
            },
            // 面對 3-bet：4-bet 更窄
            vs_three_bet: ScenarioWidths {
                aggressive_earliest: 250,
                aggressive_latest: 600,
                call_extra: 900,
                mix_band: 200,
            },
            // 面對 4-bet：只剩最強牌
            vs_four_bet: ScenarioWidths {
                aggressive_earliest: 120,
                aggressive_latest: 300,
                call_extra: 400,
                mix_band: 120,
            },
            // 面對擠壓：比面對 3-bet 更緊
            vs_squeeze: ScenarioWidths {
                aggressive_earliest: 200,
                aggressive_latest: 450,
                call_extra: 700,
                mix_band: 200,
            },

            // 短碼放寬（推入範圍寬），深碼略收（投機牌可玩但不宜過寬開牌）
            bucket_multiplier: [
                18_000, // [0,15)
                14_000, // [15,25)
                11_500, // [25,40)
                10_000, // [40,70)
                10_000, // [70,110)
                9_800,  // [110,160)
                9_500,  // [160,240)
                9_200,  // [240,400)
                9_000,  // [400,∞)
            ],
            push_fold_below: StackBucket::Short,

            // SB 略窄於 BTN；BB 在無人開牌時本就可過牌，主動範圍更窄
            sb_aggressive: 3_800,
            bb_aggressive: 2_000,

            open_size_centi_bb: 250,
            three_bet_size_centi_bb: 900,
            four_bet_size_centi_bb: 2_200,
        }
    }

    /// 取得某情境的寬度參數（校準工具用）。
    #[must_use]
    pub fn widths_of(&self, scenario: PreflopScenario) -> ScenarioWidths {
        self.widths_for(scenario)
    }

    /// 取得某 bucket 的乘數（校準工具用）。
    #[must_use]
    pub fn bucket_multiplier_of(&self, bucket: StackBucket) -> Myriad {
        self.bucket_multiplier[Self::bucket_index(bucket)]
    }

    /// 設定某情境最早位置的主動寬度（校準工具用）。
    pub fn set_aggressive_earliest(&mut self, scenario: PreflopScenario, value: Myriad) {
        self.widths_mut(scenario).aggressive_earliest = value;
    }

    /// 設定某情境最晚位置的主動寬度（校準工具用）。
    pub fn set_aggressive_latest(&mut self, scenario: PreflopScenario, value: Myriad) {
        self.widths_mut(scenario).aggressive_latest = value;
    }

    /// 設定某 bucket 的乘數（校準工具用）。
    pub fn set_bucket_multiplier(&mut self, bucket: StackBucket, value: Myriad) {
        self.bucket_multiplier[Self::bucket_index(bucket)] = value;
    }

    fn widths_mut(&mut self, scenario: PreflopScenario) -> &mut ScenarioWidths {
        match scenario {
            PreflopScenario::Unopened => &mut self.unopened,
            PreflopScenario::VsLimp { .. } => &mut self.vs_limp,
            PreflopScenario::VsOpen { .. } => &mut self.vs_open,
            PreflopScenario::VsThreeBet { .. } => &mut self.vs_three_bet,
            PreflopScenario::VsFourBet { .. } => &mut self.vs_four_bet,
            PreflopScenario::VsSqueeze { .. } => &mut self.vs_squeeze,
        }
    }

    fn widths_for(&self, scenario: PreflopScenario) -> ScenarioWidths {
        match scenario {
            PreflopScenario::Unopened => self.unopened,
            PreflopScenario::VsLimp { .. } => self.vs_limp,
            PreflopScenario::VsOpen { .. } => self.vs_open,
            PreflopScenario::VsThreeBet { .. } => self.vs_three_bet,
            PreflopScenario::VsFourBet { .. } => self.vs_four_bet,
            PreflopScenario::VsSqueeze { .. } => self.vs_squeeze,
        }
    }

    fn bucket_index(bucket: StackBucket) -> usize {
        match bucket {
            StackBucket::VeryShort => 0,
            StackBucket::Short => 1,
            StackBucket::Medium => 2,
            StackBucket::Deep => 3,
            StackBucket::Deeper => 4,
            StackBucket::Deepest => 5,
            StackBucket::VeryDeep => 6,
            StackBucket::UltraDeep => 7,
            StackBucket::Unbounded => 8,
        }
    }
}

/// 節點的「預期對抗人數」，決定該用哪張 equity 排序表。
///
/// # 這個對應為什麼重要
///
/// 直覺上「9-max 開牌 = 對 8 個對手」，但那是錯的：**開牌時不會面對 8 手
/// 隨機牌**，多數人會棄牌，實際對抗的是 1～2 手比隨機強的牌。
///
/// 用 8 人 equity 排序開牌範圍會產生明顯錯誤的結果——同花牌因多人做成
/// 同花的機會而被高估，中小對子因多人底池被稀釋而被低估，於是出現
/// 「UTG 開 K9s 卻棄 88」這種任何牌手都會立刻指出的排序。
///
/// 因此開牌與面對加注一律以少人數排序；只有面對多名跛入者這種確實會
/// 多人看牌的情境才用較高的對手數。
#[must_use]
pub fn expected_opponents(node: &PreflopNode) -> usize {
    match node.scenario {
        // 開牌：預期被 1～2 人跟進
        PreflopScenario::Unopened => 2,
        // 面對跛入：跛入者多半會看牌，人數接近實際
        PreflopScenario::VsLimp { limpers } => usize::from(limpers).clamp(1, 3) + 1,
        // 面對加注：多為單挑或三人底池
        PreflopScenario::VsOpen { .. } | PreflopScenario::VsSqueeze { .. } => 2,
        // 3-bet／4-bet 後幾乎必為單挑
        PreflopScenario::VsThreeBet { .. } | PreflopScenario::VsFourBet { .. } => 1,
    }
}

/// 依節點與規則產生該格的行動分佈。
///
/// # Errors
/// 正規化失敗時回傳錯誤（理論上不會發生，因為必有 fold 保底）。
pub fn distribution_for(
    node: &PreflopNode,
    class: HandClass,
    rules: &BaselineRules,
    ranking: &EquityRanking,
) -> Result<ActionDistribution, DistributionError> {
    let widths = rules.widths_for(node.scenario);
    let order = positions_for(node.seated);
    let position_index = order
        .iter()
        .position(|&p| p == node.hero)
        .unwrap_or(0);

    // 盲注位不套用「位置越晚越寬」：它們行動順序最晚但翻後最不利，
    // 沿用內插會得到比 BTN 更寬的開牌範圍，與現實相反
    let interpolated = match node.hero {
        PositionLabel::Sb => i64::from(rules.sb_aggressive),
        PositionLabel::Bb => i64::from(rules.bb_aggressive),
        _ => {
            // 內插只在非盲注位之間進行（UTG..BTN）
            let non_blind = order.len().saturating_sub(2).max(1);
            let last_index = non_blind.saturating_sub(1).max(1);
            let span =
                i64::from(widths.aggressive_latest) - i64::from(widths.aggressive_earliest);
            i64::from(widths.aggressive_earliest)
                + span * i64::try_from(position_index).unwrap_or(0)
                    / i64::try_from(last_index).unwrap_or(1)
        }
    };

    // 套用 bucket 乘數
    let multiplier = i64::from(rules.bucket_multiplier[BaselineRules::bucket_index(node.bucket)]);
    let clamped = (interpolated * multiplier / 10_000).clamp(0, i64::from(FULL));
    let aggressive_width = Myriad::try_from(clamped).unwrap_or(FULL);
    let call_width = aggressive_width.saturating_add(widths.call_extra).min(FULL);

    let percentile = Myriad::try_from(ranking.percentile_myriad(class)).unwrap_or(FULL);
    let band = widths.mix_band.max(1);

    // 短碼採推入或棄牌，不做小額加注
    let aggressive_action = if node.bucket <= rules.push_fold_below {
        Action::AllIn
    } else {
        Action::RaiseTo(Chips::new(u64::from(raise_size(node.scenario, rules))))
    };

    let weights = if percentile + band <= aggressive_width {
        // 完全落在主動範圍內
        vec![(aggressive_action, 100u64)]
    } else if percentile <= aggressive_width + band {
        // 主動與下一段的混合帶：越靠近邊界，主動比例越低
        let into_band = percentile + band - aggressive_width;
        let aggressive_share = (2 * u64::from(band)).saturating_sub(u64::from(into_band));
        let other = if call_width > aggressive_width {
            Action::Call
        } else {
            Action::Fold
        };
        vec![
            (aggressive_action, aggressive_share.max(1)),
            (other, u64::from(into_band).max(1)),
        ]
    } else if percentile <= call_width {
        vec![(Action::Call, 100)]
    } else {
        vec![(Action::Fold, 100)]
    };

    ActionDistribution::from_weights(weights)
}

fn raise_size(scenario: PreflopScenario, rules: &BaselineRules) -> u32 {
    match scenario {
        PreflopScenario::Unopened | PreflopScenario::VsLimp { .. } => rules.open_size_centi_bb,
        PreflopScenario::VsOpen { .. } => rules.three_bet_size_centi_bb,
        PreflopScenario::VsThreeBet { .. } | PreflopScenario::VsSqueeze { .. } => {
            rules.four_bet_size_centi_bb
        }
        PreflopScenario::VsFourBet { .. } => rules.four_bet_size_centi_bb * 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::ranking::class_of;
    use crate::card::Rank;

    fn ranking() -> EquityRanking {
        EquityRanking::compute(5, 800)
    }

    fn node(hero: PositionLabel, scenario: PreflopScenario, bucket: StackBucket) -> PreflopNode {
        PreflopNode {
            seated: 9,
            hero,
            bucket,
            scenario,
        }
    }

    #[test]
    fn 佔位規則預設為未簽核() {
        let rules = BaselineRules::engineering_placeholder();
        assert!(
            !rules.consultant_approved,
            "工程佔位內容不得預設為已簽核，否則可能誤上線"
        );
        assert!(rules.version.contains("placeholder"));
    }

    #[test]
    fn 每一格的頻率合計皆為百分之百() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        let mut checked = 0;
        for scenario in [
            PreflopScenario::Unopened,
            PreflopScenario::VsOpen {
                opener: PositionLabel::Utg,
            },
            PreflopScenario::VsThreeBet {
                by: PositionLabel::Btn,
            },
        ] {
            for bucket in [StackBucket::VeryShort, StackBucket::Deep, StackBucket::VeryDeep] {
                for class in HandClass::all() {
                    let node = node(PositionLabel::Co, scenario, bucket);
                    let d = distribution_for(&node, class, &rules, &ranking)
                        .expect("必有 fold 保底，不應失敗");
                    let total: Myriad = d.entries().iter().map(|(_, w)| *w).sum();
                    assert_eq!(total, FULL, "{} 的頻率合計必須為 100%", class.label());
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 3 * 3 * 169);
    }

    #[test]
    fn 位置越晚開牌範圍越寬() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        // 取一張中等強度的牌，早位應棄、晚位應開
        let marginal = class_of(Rank::King, Rank::Nine, false); // K9o

        let utg = distribution_for(
            &node(PositionLabel::Utg, PreflopScenario::Unopened, StackBucket::VeryDeep),
            marginal,
            &rules,
            &ranking,
        )
        .expect("產生");
        let btn = distribution_for(
            &node(PositionLabel::Btn, PreflopScenario::Unopened, StackBucket::VeryDeep),
            marginal,
            &rules,
            &ranking,
        )
        .expect("產生");

        let raise_weight = |d: &ActionDistribution| -> Myriad {
            d.entries()
                .iter()
                .filter(|(a, _)| matches!(a, Action::RaiseTo(_) | Action::AllIn))
                .map(|(_, w)| *w)
                .sum()
        };
        assert!(
            raise_weight(&btn) > raise_weight(&utg),
            "BTN 的開牌頻率應高於 UTG"
        );
    }

    /// 回歸測試：盲注位不得比 BTN 更寬。
    ///
    /// 初版把「位置越晚越寬」線性套到全部位置，導致 SB 的開牌範圍比 BTN
    /// 還寬——SB 行動順序最晚但翻後最不利，這與現實相反。
    #[test]
    fn 盲注位的開牌範圍不得寬於按鈕位() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();

        let width = |hero: PositionLabel| -> u64 {
            HandClass::all()
                .into_iter()
                .map(|class| {
                    let d = distribution_for(
                        &node(hero, PreflopScenario::Unopened, StackBucket::VeryDeep),
                        class,
                        &rules,
                        &ranking,
                    )
                    .expect("產生");
                    let raise: Myriad = d
                        .entries()
                        .iter()
                        .filter(|(a, _)| matches!(a, Action::RaiseTo(_) | Action::AllIn))
                        .map(|(_, w)| *w)
                        .sum();
                    u64::from(raise)
                })
                .sum()
        };

        let btn = width(PositionLabel::Btn);
        assert!(
            width(PositionLabel::Sb) < btn,
            "SB 翻後最不利，開牌範圍不得寬於 BTN"
        );
        assert!(
            width(PositionLabel::Bb) < btn,
            "BB 的主動範圍不得寬於 BTN"
        );
        assert!(
            width(PositionLabel::Utg) < width(PositionLabel::Co),
            "非盲注位仍應維持位置越晚越寬"
        );
    }

    /// 回歸測試：開牌情境的排序表必須用少人數。
    ///
    /// 初版把「9-max 開牌」對應成「對 8 個隨機對手」，導致同花牌被多人
    /// equity 高估、中小對子被稀釋低估，產生「UTG 開 K9s 卻棄 88」這種
    /// 任何牌手都會立刻指出的排序。開牌時多數人會棄牌，實際對抗的是
    /// 1～2 手比隨機強的牌，因此排序必須用少人數。
    #[test]
    fn 開牌情境以少人數排序而非全桌人數() {
        for seated in 6u8..=9 {
            let node = PreflopNode {
                seated,
                hero: PositionLabel::Utg,
                bucket: StackBucket::VeryDeep,
                scenario: PreflopScenario::Unopened,
            };
            let opponents = expected_opponents(&node);
            assert!(
                opponents <= 2,
                "{seated}-max 開牌的預期對抗人數為 {opponents}，不應接近全桌人數"
            );
        }
    }

    /// 開牌範圍中，中等對子不應被弱同花牌超越。
    ///
    /// 這是上一則錯誤的直接症狀，用實際產生結果驗證而非只驗參數。
    #[test]
    fn 開牌範圍中中等對子不弱於弱同花牌() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = EquityRanking::compute(2, 3_000);
        let node = node(PositionLabel::Utg, PreflopScenario::Unopened, StackBucket::VeryDeep);

        let aggressive = |class: HandClass| -> Myriad {
            distribution_for(&node, class, &rules, &ranking)
                .expect("產生")
                .entries()
                .iter()
                .filter(|(a, _)| matches!(a, Action::RaiseTo(_) | Action::AllIn))
                .map(|(_, w)| *w)
                .sum()
        };

        let eights = class_of(Rank::Eight, Rank::Eight, false);
        let king_nine_suited = class_of(Rank::King, Rank::Nine, true);
        assert!(
            aggressive(eights) >= aggressive(king_nine_suited),
            "88 的開牌頻率不應低於 K9s"
        );
    }

    #[test]
    fn aa_在任何情境都不會被棄掉() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        let aces = class_of(Rank::Ace, Rank::Ace, false);

        for scenario in [
            PreflopScenario::Unopened,
            PreflopScenario::VsOpen { opener: PositionLabel::Utg },
            PreflopScenario::VsFourBet { by: PositionLabel::Utg },
        ] {
            for hero in [PositionLabel::Utg, PositionLabel::Btn, PositionLabel::Bb] {
                let d = distribution_for(
                    &node(hero, scenario, StackBucket::VeryDeep),
                    aces,
                    &rules,
                    &ranking,
                )
                .expect("產生");
                assert_eq!(
                    d.weight_of(Action::Fold),
                    0,
                    "AA 在 {scenario:?}／{hero:?} 不應有棄牌頻率"
                );
            }
        }
    }

    #[test]
    fn 最弱的牌在早位開牌情境必然棄牌() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        let worst = class_of(Rank::Seven, Rank::Two, false); // 72o

        let d = distribution_for(
            &node(PositionLabel::Utg, PreflopScenario::Unopened, StackBucket::VeryDeep),
            worst,
            &rules,
            &ranking,
        )
        .expect("產生");
        assert_eq!(d.weight_of(Action::Fold), FULL, "72o 在 UTG 應 100% 棄牌");
    }

    #[test]
    fn 短碼採推入或棄牌不做小額加注() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        let strong = class_of(Rank::Ace, Rank::Ace, false);

        let d = distribution_for(
            &node(PositionLabel::Btn, PreflopScenario::Unopened, StackBucket::VeryShort),
            strong,
            &rules,
            &ranking,
        )
        .expect("產生");
        assert!(
            d.entries()
                .iter()
                .all(|(a, _)| !matches!(a, Action::RaiseTo(_))),
            "短碼不得出現小額加注"
        );
        assert!(d.weight_of(Action::AllIn) > 0, "短碼應以推入為主動行動");
    }
}
