//! 顧問校準工具：13×13 範圍矩陣與參數歸因。
//!
//! # 為什麼需要「歸因」而不只是「畫出來」
//!
//! 顧問看到某格不對（例如「BTN 的 55 不該只開 35%」）之後，真正的問題是
//! **該調哪個參數、調了會連帶影響哪些格**。參數化模型的每個參數都同時
//!管著大量格子，因此顧問的單格意見必然帶有連帶效果。
//!
//! [`attribute`] 回答的就是這件事：要滿足這個意見，某參數必須改到多少，
//! 以及**會被一併拉進或推出範圍的其他手牌**。
//!
//! # 連帶效果不可接受時，代表模型缺參數
//!
//! 若顧問說「55 要開但 K5o 不要」，而兩者在 equity 排序上相鄰，
//! 那就不是調參能解決的——這是 raw equity 排序無法表達「對子的翻後價值」
//! 的證據，模型需要新增具名參數（例如對子加成）。**這種發現越早越好**，
//! 因此本工具刻意把連帶效果攤開，而不是自動找一個最小改動了事。

use crate::betting::Action;
use crate::strategy::baseline::{distribution_for, BaselineRules};
use crate::strategy::distribution::{Myriad, FULL};
use crate::strategy::hand_class::HandClass;
use crate::strategy::preflop::PreflopNode;
use crate::strategy::ranking::EquityRanking;

/// 矩陣中一格的行動頻率彙總。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixCell {
    pub class: HandClass,
    /// 主動行動（加注或推入）的合計頻率
    pub aggressive: Myriad,
    pub call: Myriad,
    pub fold: Myriad,
    /// 該類別在 equity 排序中的百分位（萬分比，0 為最強）
    pub percentile: Myriad,
}

impl MatrixCell {
    /// 是否為混合格（同一手牌有兩種以上行動）。
    #[must_use]
    pub const fn is_mixed(&self) -> bool {
        self.aggressive > 0 && self.aggressive < FULL
    }
}

/// 一個節點的完整 13×13 範圍矩陣。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeMatrix {
    pub node: PreflopNode,
    /// 依 `HandClass::index()` 存放的 169 格
    cells: Vec<MatrixCell>,
}

impl RangeMatrix {
    /// 產生某節點的矩陣。
    #[must_use]
    pub fn build(node: PreflopNode, rules: &BaselineRules, ranking: &EquityRanking) -> Self {
        let mut cells = Vec::with_capacity(169);
        cells.resize(
            169,
            MatrixCell {
                class: HandClass::all()[0],
                aggressive: 0,
                call: 0,
                fold: FULL,
                percentile: 0,
            },
        );

        for class in HandClass::all() {
            let distribution = distribution_for(&node, class, rules, ranking)
                .unwrap_or_else(|e| panic!("產生 {} 的分佈失敗：{e:?}", class.label()));

            let mut aggressive = 0;
            let mut call = 0;
            let mut fold = 0;
            for &(action, weight) in distribution.entries() {
                match action {
                    Action::RaiseTo(_) | Action::AllIn => aggressive += weight,
                    Action::Call => call += weight,
                    Action::Fold | Action::Check => fold += weight,
                }
            }

            cells[class.index()] = MatrixCell {
                class,
                aggressive,
                call,
                fold,
                percentile: Myriad::try_from(ranking.percentile_myriad(class)).unwrap_or(FULL),
            };
        }

        Self { node, cells }
    }

    #[must_use]
    pub fn cell(&self, class: HandClass) -> MatrixCell {
        self.cells[class.index()]
    }

    /// 依 13×13 網格順序（列由 A 到 2）取得全部格子。
    #[must_use]
    pub fn grid(&self) -> Vec<Vec<MatrixCell>> {
        let all = HandClass::all();
        (0..13)
            .map(|row| {
                (0..13)
                    .map(|col| self.cells[all[row * 13 + col].index()])
                    .collect()
            })
            .collect()
    }

    /// 範圍寬度：169 格主動頻率的平均（萬分比）。
    #[must_use]
    pub fn width_myriad(&self) -> Myriad {
        let total: u64 = self.cells.iter().map(|c| u64::from(c.aggressive)).sum();
        Myriad::try_from(total / 169).unwrap_or(FULL)
    }

    /// 混合格清單，供顧問優先檢視邊界。
    #[must_use]
    pub fn mixed_cells(&self) -> Vec<MatrixCell> {
        let mut out: Vec<MatrixCell> = self.cells.iter().copied().filter(MatrixCell::is_mixed).collect();
        out.sort_by_key(|c| std::cmp::Reverse(c.aggressive));
        out
    }
}

/// 顧問對某一格的意見。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 這格應該 100% 主動（開牌／加注／推入）
    ShouldBeAggressive,
    /// 這格不該有任何主動頻率
    ShouldNotBeAggressive,
}

/// 可調整的參數。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterRef {
    /// 該情境最早位置的主動寬度
    AggressiveEarliest,
    /// 該情境最晚（非盲注）位置的主動寬度
    AggressiveLatest,
    /// SB 專用寬度
    SbAggressive,
    /// BB 專用寬度
    BbAggressive,
    /// 該 bucket 的乘數
    BucketMultiplier,
}

impl ParameterRef {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AggressiveEarliest => "aggressive_earliest",
            Self::AggressiveLatest => "aggressive_latest",
            Self::SbAggressive => "sb_aggressive",
            Self::BbAggressive => "bb_aggressive",
            Self::BucketMultiplier => "bucket_multiplier",
        }
    }
}

/// 歸因結果：要滿足顧問的意見，哪個參數要改到多少，代價是什麼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub parameter: ParameterRef,
    pub current: Myriad,
    /// 滿足意見所需的值
    pub required: Myriad,
    /// 改動後會**新增**主動頻率的其他手牌
    pub pulled_in: Vec<HandClass>,
    /// 改動後會**失去**主動頻率的其他手牌
    pub pushed_out: Vec<HandClass>,
}

impl Attribution {
    /// 連帶影響的格數。
    #[must_use]
    pub fn collateral_count(&self) -> usize {
        self.pulled_in.len() + self.pushed_out.len()
    }
}

/// 針對一則顧問意見做參數歸因。
///
/// 回傳可能的調整途徑（通常是位置寬度與 bucket 乘數兩條），
/// 各自附上所需值與連帶影響，由顧問決定走哪一條、或宣告模型需要新參數。
#[must_use]
pub fn attribute(
    node: PreflopNode,
    class: HandClass,
    verdict: Verdict,
    rules: &BaselineRules,
    ranking: &EquityRanking,
) -> Vec<Attribution> {
    let before = RangeMatrix::build(node, rules, ranking);
    let current_cell = before.cell(class);

    // 已經符合意見就不需要調整
    let satisfied = match verdict {
        Verdict::ShouldBeAggressive => current_cell.aggressive == FULL,
        Verdict::ShouldNotBeAggressive => current_cell.aggressive == 0,
    };
    if satisfied {
        return Vec::new();
    }

    // 對每條可調途徑做二分搜尋，找出剛好滿足意見的參數值
    let mut out = Vec::new();
    for parameter in candidate_parameters(&node, rules) {
        let Some(required) = solve(parameter, node, class, verdict, rules, ranking) else {
            continue;
        };
        let adjusted = apply(parameter, &node, rules, required);
        let after = RangeMatrix::build(node, &adjusted, ranking);

        let mut pulled_in = Vec::new();
        let mut pushed_out = Vec::new();
        for other in HandClass::all() {
            if other == class {
                continue;
            }
            let was = before.cell(other).aggressive;
            let now = after.cell(other).aggressive;
            if was == 0 && now > 0 {
                pulled_in.push(other);
            } else if was > 0 && now == 0 {
                pushed_out.push(other);
            }
        }

        out.push(Attribution {
            parameter,
            current: current_value(parameter, &node, rules),
            required,
            pulled_in,
            pushed_out,
        });
    }
    out
}

fn candidate_parameters(node: &PreflopNode, rules: &BaselineRules) -> Vec<ParameterRef> {
    use crate::position::PositionLabel;
    let position_param = match node.hero {
        PositionLabel::Sb => ParameterRef::SbAggressive,
        PositionLabel::Bb => ParameterRef::BbAggressive,
        // UTG 是內插起點，只受 earliest 影響
        PositionLabel::Utg => ParameterRef::AggressiveEarliest,
        _ => ParameterRef::AggressiveLatest,
    };
    let _ = rules;
    vec![position_param, ParameterRef::BucketMultiplier]
}

fn current_value(parameter: ParameterRef, node: &PreflopNode, rules: &BaselineRules) -> Myriad {
    let widths = rules.widths_of(node.scenario);
    match parameter {
        ParameterRef::AggressiveEarliest => widths.aggressive_earliest,
        ParameterRef::AggressiveLatest => widths.aggressive_latest,
        ParameterRef::SbAggressive => rules.sb_aggressive,
        ParameterRef::BbAggressive => rules.bb_aggressive,
        ParameterRef::BucketMultiplier => rules.bucket_multiplier_of(node.bucket),
    }
}

fn apply(
    parameter: ParameterRef,
    node: &PreflopNode,
    rules: &BaselineRules,
    value: Myriad,
) -> BaselineRules {
    let mut out = rules.clone();
    match parameter {
        ParameterRef::AggressiveEarliest => out.set_aggressive_earliest(node.scenario, value),
        ParameterRef::AggressiveLatest => out.set_aggressive_latest(node.scenario, value),
        ParameterRef::SbAggressive => out.sb_aggressive = value,
        ParameterRef::BbAggressive => out.bb_aggressive = value,
        ParameterRef::BucketMultiplier => out.set_bucket_multiplier(node.bucket, value),
    }
    out
}

/// 二分搜尋滿足意見的最小改動值。
///
/// 用搜尋而非解析求逆：產生邏輯含 clamp、整數除法與混合帶，
/// 解析式容易與實作漂移，搜尋則恆與實際產生結果一致。
fn solve(
    parameter: ParameterRef,
    node: PreflopNode,
    class: HandClass,
    verdict: Verdict,
    rules: &BaselineRules,
    ranking: &EquityRanking,
) -> Option<Myriad> {
    let upper = match parameter {
        ParameterRef::BucketMultiplier => 60_000,
        _ => FULL,
    };

    let meets = |value: Myriad| -> bool {
        let adjusted = apply(parameter, &node, rules, value);
        let cell = RangeMatrix::build(node, &adjusted, ranking).cell(class);
        match verdict {
            Verdict::ShouldBeAggressive => cell.aggressive == FULL,
            Verdict::ShouldNotBeAggressive => cell.aggressive == 0,
        }
    };

    // 主動寬度與該格是否主動同向，因此可二分
    let (mut low, mut high) = (0u32, upper);
    if verdict == Verdict::ShouldBeAggressive {
        if !meets(high) {
            return None;
        }
        while low < high {
            let mid = low + (high - low) / 2;
            if meets(mid) {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
    } else {
        if !meets(0) {
            return None;
        }
        while low < high {
            let mid = high - (high - low) / 2;
            if meets(mid) {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
    }
    Some(low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::PositionLabel;
    use crate::strategy::decision::StackBucket;
    use crate::strategy::preflop::PreflopScenario;
    use crate::strategy::ranking::class_of;
    use crate::card::Rank;

    fn ranking() -> EquityRanking {
        EquityRanking::compute(5, 600)
    }

    fn btn_open() -> PreflopNode {
        PreflopNode {
            seated: 9,
            hero: PositionLabel::Btn,
            bucket: StackBucket::VeryDeep,
            scenario: PreflopScenario::Unopened,
        }
    }

    #[test]
    fn 矩陣涵蓋_169_格且為十三乘十三() {
        let matrix = RangeMatrix::build(btn_open(), &BaselineRules::engineering_placeholder(), &ranking());
        let grid = matrix.grid();
        assert_eq!(grid.len(), 13);
        assert!(grid.iter().all(|row| row.len() == 13));

        let mut labels: Vec<String> = grid
            .iter()
            .flatten()
            .map(|c| c.class.label())
            .collect();
        let before = labels.len();
        labels.sort();
        labels.dedup();
        assert_eq!(before, 169);
        assert_eq!(labels.len(), 169, "169 格不得重複");
    }

    #[test]
    fn 每格三種頻率合計為百分之百() {
        let matrix = RangeMatrix::build(btn_open(), &BaselineRules::engineering_placeholder(), &ranking());
        for class in HandClass::all() {
            let cell = matrix.cell(class);
            assert_eq!(
                cell.aggressive + cell.call + cell.fold,
                FULL,
                "{} 的頻率合計錯誤",
                class.label()
            );
        }
    }

    #[test]
    fn 已符合意見時不產生調整建議() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        let aces = class_of(Rank::Ace, Rank::Ace, false);
        let result = attribute(btn_open(), aces, Verdict::ShouldBeAggressive, &rules, &ranking);
        assert!(result.is_empty(), "AA 在 BTN 已是 100% 開牌，無需調整");
    }

    #[test]
    fn 歸因給出所需參數值與連帶影響() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        // 找一個目前完全不開的中等牌
        let matrix = RangeMatrix::build(btn_open(), &rules, &ranking);
        let target = HandClass::all()
            .into_iter()
            .find(|c| matrix.cell(*c).aggressive == 0)
            .expect("應有不開的牌");

        let attributions = attribute(btn_open(), target, Verdict::ShouldBeAggressive, &rules, &ranking);
        assert!(!attributions.is_empty(), "應給出至少一條調整途徑");

        for attribution in &attributions {
            assert!(
                attribution.required > attribution.current,
                "要讓更弱的牌進入範圍，寬度參數必須提高"
            );
            // 調寬必然連帶拉進其他更強但原本在邊界外的牌
            assert!(
                !attribution.pulled_in.is_empty(),
                "{} 的調整應有連帶影響，工具必須攤開讓顧問判斷",
                attribution.parameter.as_str()
            );
        }
    }

    #[test]
    fn 調整後該格確實滿足意見() {
        let rules = BaselineRules::engineering_placeholder();
        let ranking = ranking();
        let matrix = RangeMatrix::build(btn_open(), &rules, &ranking);
        let target = HandClass::all()
            .into_iter()
            .find(|c| matrix.cell(*c).aggressive == 0)
            .expect("應有不開的牌");

        for attribution in attribute(btn_open(), target, Verdict::ShouldBeAggressive, &rules, &ranking) {
            let adjusted = apply(attribution.parameter, &btn_open(), &rules, attribution.required);
            let cell = RangeMatrix::build(btn_open(), &adjusted, &ranking).cell(target);
            assert_eq!(
                cell.aggressive, FULL,
                "依 {} 的建議值調整後，該格應 100% 主動",
                attribution.parameter.as_str()
            );
        }
    }
}

// ── 顧問調整結果的回讀 ──────────────────────────────────────────────────

/// 由校準工作台匯出的 JSON 還原規則集。
///
/// 工作台只負責預覽，**正式的 727,038 格全表一律由引擎展開**，
/// 因此回讀後必須重新驗證每個值是否在合法範圍內——不能假設前端已擋過。
///
/// 刻意手寫解析而不引入 JSON crate：引擎維持零外部相依，而工作台匯出的
/// 格式是我們自己產生的、結構固定的扁平物件，手寫解析的成本低於引入相依。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedRules {
    pub version: String,
    pub values: Vec<(String, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    MissingField(&'static str),
    BadNumber(String),
    OutOfRange { field: String, value: i64 },
}

/// 從工作台匯出的 JSON 取出「路徑 → 數值」清單。
///
/// # Errors
/// 缺少版本欄位或數值無法解析時回傳錯誤。
pub fn parse_workbench_export(json: &str) -> Result<ImportedRules, ImportError> {
    let version = extract_string(json, "version").ok_or(ImportError::MissingField("version"))?;

    let mut values = Vec::new();
    for key in WORKBENCH_FIELDS {
        if let Some(value) = extract_number(json, key) {
            values.push(((*key).to_owned(), value));
        }
    }
    if values.is_empty() {
        return Err(ImportError::MissingField("找不到任何參數欄位"));
    }
    Ok(ImportedRules { version, values })
}

/// 工作台可調整的欄位。回讀時只接受這些鍵，
/// 對應核心規格 4.3「不得直接注入未登錄參數」的精神。
pub const WORKBENCH_FIELDS: &[&str] = &[
    "sbAggressive",
    "bbAggressive",
    "pocketPair",
    "suitedAce",
    "suitedConnector",
    "suitedOneGap",
    "suitedTwoGap",
    "suitedWideGap",
    "offsuitBroadway",
    "offsuitOther",
];

/// 套用回讀的值到規則集，逐項驗證範圍。
///
/// # Errors
/// 任一值超出合法範圍時回傳該欄位與值，**整批不套用**——
/// 部分套用會產生一組沒有人簽核過的混合設定。
pub fn apply_import(
    rules: &BaselineRules,
    imported: &ImportedRules,
) -> Result<BaselineRules, ImportError> {
    use crate::strategy::playability::MAX_SHIFT;

    // 先全部驗證再套用
    for (field, value) in &imported.values {
        let ok = match field.as_str() {
            "sbAggressive" | "bbAggressive" => (0..=i64::from(FULL)).contains(value),
            _ => value.abs() <= i64::from(MAX_SHIFT),
        };
        if !ok {
            return Err(ImportError::OutOfRange {
                field: field.clone(),
                value: *value,
            });
        }
    }

    let mut out = rules.clone();
    for (field, value) in &imported.values {
        let narrow = i32::try_from(*value).unwrap_or(0);
        let myriad = Myriad::try_from(*value).unwrap_or(0);
        match field.as_str() {
            "sbAggressive" => out.sb_aggressive = myriad,
            "bbAggressive" => out.bb_aggressive = myriad,
            "pocketPair" => out.playability.pocket_pair = narrow,
            "suitedAce" => out.playability.suited_ace = narrow,
            "suitedConnector" => out.playability.suited_connector = narrow,
            "suitedOneGap" => out.playability.suited_one_gap = narrow,
            "suitedTwoGap" => out.playability.suited_two_gap = narrow,
            "suitedWideGap" => out.playability.suited_wide_gap = narrow,
            "offsuitBroadway" => out.playability.offsuit_broadway = narrow,
            "offsuitOther" => out.playability.offsuit_other = narrow,
            _ => {}
        }
    }
    // 回讀的內容仍未簽核，除非另行標記
    out.consultant_approved = false;
    out.version = format!("{}+consultant", imported.version);
    Ok(out)
}

fn extract_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let colon = rest.find(':')? + 1;
    let rest = &rest[colon..];
    let open = rest.find('"')? + 1;
    let rest = &rest[open..];
    let close = rest.find('"')?;
    Some(rest[..close].to_owned())
}

fn extract_number(json: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\"");
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let colon = rest.find(':')? + 1;
    let rest = rest[colon..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod import_tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "version": "placeholder-v0",
      "sbAggressive": 3600,
      "bbAggressive": 2200,
      "playability": {
        "pocketPair": 700,
        "suitedAce": 300,
        "suitedConnector": 900,
        "suitedOneGap": 400,
        "suitedTwoGap": 150,
        "suitedWideGap": -700,
        "offsuitBroadway": -200,
        "offsuitOther": -500
      }
    }"#;

    #[test]
    fn 可解析工作台匯出的格式() {
        let imported = parse_workbench_export(SAMPLE).expect("解析");
        assert_eq!(imported.version, "placeholder-v0");
        assert_eq!(imported.values.len(), WORKBENCH_FIELDS.len());
        assert!(imported
            .values
            .iter()
            .any(|(k, v)| k == "suitedConnector" && *v == 900));
        assert!(imported
            .values
            .iter()
            .any(|(k, v)| k == "suitedWideGap" && *v == -700));
    }

    #[test]
    fn 套用後的規則反映顧問調整() {
        let base = BaselineRules::engineering_placeholder();
        let imported = parse_workbench_export(SAMPLE).expect("解析");
        let applied = apply_import(&base, &imported).expect("套用");

        assert_eq!(applied.playability.suited_connector, 900);
        assert_eq!(applied.playability.suited_wide_gap, -700);
        assert_eq!(applied.sb_aggressive, 3_600);
    }

    #[test]
    fn 回讀的內容仍標為未簽核() {
        let base = BaselineRules::engineering_placeholder();
        let imported = parse_workbench_export(SAMPLE).expect("解析");
        let applied = apply_import(&base, &imported).expect("套用");
        assert!(
            !applied.consultant_approved,
            "顧問調過參數不等於已完成簽核，簽核是另一個明確動作"
        );
        assert!(applied.version.contains("consultant"), "版本須可辨識來源");
    }

    #[test]
    fn 越界值整批拒絕而非部分套用() {
        let bad = SAMPLE.replace("\"suitedConnector\": 900", "\"suitedConnector\": 9900");
        let imported = parse_workbench_export(&bad).expect("解析");
        let base = BaselineRules::engineering_placeholder();
        assert!(
            matches!(
                apply_import(&base, &imported),
                Err(ImportError::OutOfRange { .. })
            ),
            "超出上限必須整批拒絕，部分套用會產生沒有人簽核過的混合設定"
        );
    }

    #[test]
    fn 缺少版本欄位時拒絕() {
        assert!(parse_workbench_export(r#"{"sbAggressive":100}"#).is_err());
    }
}
