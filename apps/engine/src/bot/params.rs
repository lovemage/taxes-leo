//! Bot 的兩層參數與其 schema。
//!
//! 核心規格 4.3（2026-08-16 移除等級後）：參數為**人格層**（`persona`，11 欄）
//! 與**行為層**（`botBehavior`，10 欄），兩層皆可逐座覆寫。
//!
//! 規格另要求：「每個參數必須在 generated schema 中具有型別、單位、合法範圍、
//! 預設值、顯示名稱、說明與可覆寫層級。」因此每個欄位都有對應的
//! [`ParamSpec`]，UI 的參數面板與驗證一律由它產生，不另外手寫。
//!
//! # 為什麼參數值用整數
//!
//! 全部以萬分比或具名列舉表示，不用浮點。理由與籌碼、頻率相同：
//! 參數會進入 `RunManifest` 的內容快照並參與 hash，浮點的表示差異會讓
//! 同一組設定產生不同 hash，破壞可重現性的比對。

use std::fmt;

/// 參數值。刻意只有四種型別，讓 schema 與 UI 元件的對應保持封閉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamValue {
    /// 萬分比（10000 = 100%）
    Myriad(u32),
    /// 具名列舉的索引
    Enum(u8),
    /// 計數或整數量
    Count(u32),
    Flag(bool),
}

impl ParamValue {
    #[must_use]
    pub const fn as_myriad(self) -> Option<u32> {
        match self {
            Self::Myriad(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_count(self) -> Option<u32> {
        match self {
            Self::Count(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_flag(self) -> Option<bool> {
        match self {
            Self::Flag(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_enum(self) -> Option<u8> {
        match self {
            Self::Enum(v) => Some(v),
            _ => None,
        }
    }
}

/// 參數可被哪一層覆寫。核心規格 4.3 第 4 點：
/// 「逐座覆寫套用在對應欄位；**不得直接注入未登錄參數**。」
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideLevel {
    /// 人格層定義，可逐座覆寫
    PersonaThenSeat,
    /// 行為層定義，可逐座覆寫
    BehaviorThenSeat,
}

/// 單一參數的完整規格。UI 面板與驗證由此產生。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    /// 程式與內容檔共用的欄位鍵
    pub key: &'static str,
    /// UI 顯示名稱
    pub display: &'static str,
    /// 單位。萬分比參數顯示為百分比
    pub unit: &'static str,
    pub description: &'static str,
    pub level: OverrideLevel,
    pub default: ParamValue,
    /// 合法範圍（含端點）。列舉與布林以索引界定
    pub min: u32,
    pub max: u32,
}

impl ParamSpec {
    /// 檢查值是否在合法範圍且型別相符。
    ///
    /// # Errors
    /// 型別不符或越界時回傳說明。
    pub fn validate(&self, value: ParamValue) -> Result<(), ParamError> {
        let numeric = match (self.default, value) {
            (ParamValue::Myriad(_), ParamValue::Myriad(v))
            | (ParamValue::Count(_), ParamValue::Count(v)) => v,
            (ParamValue::Enum(_), ParamValue::Enum(v)) => u32::from(v),
            (ParamValue::Flag(_), ParamValue::Flag(v)) => u32::from(v),
            _ => {
                return Err(ParamError::TypeMismatch {
                    key: self.key,
                    expected: self.default,
                    found: value,
                })
            }
        };
        if numeric < self.min || numeric > self.max {
            return Err(ParamError::OutOfRange {
                key: self.key,
                value: numeric,
                min: self.min,
                max: self.max,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamError {
    UnknownKey(&'static str),
    TypeMismatch {
        key: &'static str,
        expected: ParamValue,
        found: ParamValue,
    },
    OutOfRange {
        key: &'static str,
        value: u32,
        min: u32,
        max: u32,
    },
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(key) => write!(f, "未登錄的參數 {key}"),
            Self::TypeMismatch { key, .. } => write!(f, "{key} 的型別不符"),
            Self::OutOfRange {
                key,
                value,
                min,
                max,
            } => write!(f, "{key} 的值 {value} 超出範圍 [{min}, {max}]"),
        }
    }
}

const PCT: &str = "%";
const NONE: &str = "";

/// 人格層 11 欄（核心規格 4.3）。
pub const PERSONA_SPECS: [ParamSpec; 11] = [
    ParamSpec {
        key: "rangeWidth",
        display: "範圍寬度",
        unit: PCT,
        description: "相對基準策略的進池範圍縮放。100% 為不變，越高越鬆",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 5_000,
        max: 16_000,
    },
    ParamSpec {
        key: "preflopAggression",
        display: "翻前侵略性",
        unit: PCT,
        description: "翻前把跟注權重移往加注的傾向。100% 為不變",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 5_000,
        max: 18_000,
    },
    ParamSpec {
        key: "postflopAggression",
        display: "翻後侵略性",
        unit: PCT,
        description: "翻後把過牌與跟注權重移往下注與加注的傾向",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 5_000,
        max: 18_000,
    },
    ParamSpec {
        key: "callPersistence",
        display: "跟注黏著度",
        unit: PCT,
        description: "面對下注時保留跟注權重的傾向，越高越像跟注站",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 5_000,
        max: 18_000,
    },
    ParamSpec {
        key: "foldDiscipline",
        display: "棄牌紀律",
        unit: PCT,
        description: "在弱勢節點確實棄牌的傾向，與跟注黏著度反向",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 5_000,
        max: 18_000,
    },
    ParamSpec {
        key: "bluffFrequency",
        display: "詐唬頻率",
        unit: PCT,
        description: "以弱牌採取主動行動的頻率縮放",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 0,
        max: 20_000,
    },
    ParamSpec {
        key: "valueThreshold",
        display: "價值門檻",
        unit: PCT,
        description: "採取價值下注所需的牌力門檻。越高越保守",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 5_000,
        max: 16_000,
    },
    ParamSpec {
        key: "betSizePreference",
        display: "尺度偏好",
        unit: PCT,
        description: "在可用尺度中偏好較大或較小的一端。100% 為置中",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 5_000,
        max: 16_000,
    },
    ParamSpec {
        key: "trapFrequency",
        display: "設陷頻率",
        unit: PCT,
        description: "以強牌選擇過牌或跟注以設陷的頻率",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 0,
        max: 20_000,
    },
    ParamSpec {
        key: "tiltResistance",
        display: "抗傾斜",
        unit: PCT,
        description: "近期大額損失後維持原策略的程度。越低越容易偏離",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 0,
        max: 10_000,
    },
    ParamSpec {
        key: "adaptationStyle",
        display: "調整風格",
        unit: NONE,
        description: "對手模型生效時的調整方向：0 不調整／1 針對緊手／2 針對鬆手",
        level: OverrideLevel::PersonaThenSeat,
        default: ParamValue::Enum(0),
        min: 0,
        max: 2,
    },
];

/// 行為層 10 欄（核心規格 4.3，原等級層）。
pub const BEHAVIOR_SPECS: [ParamSpec; 10] = [
    ParamSpec {
        key: "decisionNoisePp",
        display: "決策噪音",
        unit: PCT,
        description: "以具名公式混入均勻分佈的比例。0 為完全依策略",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Myriad(0),
        min: 0,
        max: 3_000,
    },
    ParamSpec {
        key: "preflopCoverage",
        display: "翻前覆蓋率",
        unit: PCT,
        description: "使用完整翻前策略表的比例，其餘走 fallback",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Myriad(10_000),
        min: 0,
        max: 10_000,
    },
    ParamSpec {
        key: "postflopBucketCount",
        display: "翻後分桶數",
        unit: NONE,
        description: "翻後牌力分桶的粒度。越少越粗糙",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Count(8),
        min: 2,
        max: 24,
    },
    ParamSpec {
        key: "allowedBetSizes",
        display: "可用尺度數",
        unit: NONE,
        description: "從尺度樹取用的尺度數量上限",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Count(4),
        min: 1,
        max: 10,
    },
    ParamSpec {
        key: "bluffComplexity",
        display: "詐唬複雜度",
        unit: NONE,
        description: "詐唬選牌的依據層數。0 只看牌力，越高越考慮阻斷與線路",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Count(1),
        min: 0,
        max: 3,
    },
    ParamSpec {
        key: "multiStreetPlanningDepth",
        display: "多街規劃深度",
        unit: NONE,
        description: "決策時往後推演的街數。0 為只看當街",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Count(1),
        min: 0,
        max: 3,
    },
    ParamSpec {
        key: "opponentModelEnabled",
        display: "啟用對手模型",
        unit: NONE,
        description: "是否依觀察到的公開行動調整對手範圍估計",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Flag(false),
        min: 0,
        max: 1,
    },
    ParamSpec {
        key: "opponentModelUpdateHands",
        display: "對手模型更新手數",
        unit: NONE,
        description: "累積多少手後更新一次對手模型",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Count(100),
        min: 10,
        max: 10_000,
    },
    ParamSpec {
        key: "exploitAdjustmentCapPp",
        display: "剝削調整上限",
        unit: PCT,
        description: "對手模型可改變任一行動頻率的上限。引擎強制套用",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Myriad(1_000),
        min: 0,
        max: 3_000,
    },
    ParamSpec {
        key: "explanationDepth",
        display: "解釋深度",
        unit: NONE,
        description: "決策 trace 保留的層數。影響 log 大小與面板 G 的可解釋性",
        level: OverrideLevel::BehaviorThenSeat,
        default: ParamValue::Count(2),
        min: 0,
        max: 3,
    },
];

/// 依欄位鍵查 spec。
#[must_use]
pub fn spec_of(key: &str) -> Option<&'static ParamSpec> {
    PERSONA_SPECS
        .iter()
        .chain(BEHAVIOR_SPECS.iter())
        .find(|spec| spec.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 欄位數符合規格() {
        assert_eq!(PERSONA_SPECS.len(), 11, "核心規格 4.3：人格層 11 欄");
        assert_eq!(BEHAVIOR_SPECS.len(), 10, "核心規格 4.3：行為層 10 欄");
    }

    #[test]
    fn 欄位鍵互異() {
        let mut keys: Vec<&str> = PERSONA_SPECS
            .iter()
            .chain(BEHAVIOR_SPECS.iter())
            .map(|s| s.key)
            .collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(before, keys.len(), "21 個欄位鍵必須互異");
    }

    #[test]
    fn 每欄都有完整的_schema_資訊() {
        // 核心規格 4.3：型別、單位、合法範圍、預設值、顯示名稱、說明、可覆寫層級
        for spec in PERSONA_SPECS.iter().chain(BEHAVIOR_SPECS.iter()) {
            assert!(!spec.key.is_empty());
            assert!(!spec.display.is_empty(), "{} 缺顯示名稱", spec.key);
            assert!(!spec.description.is_empty(), "{} 缺說明", spec.key);
            assert!(spec.min <= spec.max, "{} 的範圍顛倒", spec.key);
        }
    }

    #[test]
    fn 預設值必在合法範圍內() {
        for spec in PERSONA_SPECS.iter().chain(BEHAVIOR_SPECS.iter()) {
            assert_eq!(
                spec.validate(spec.default),
                Ok(()),
                "{} 的預設值不在合法範圍",
                spec.key
            );
        }
    }

    #[test]
    fn 越界值被攔下() {
        let spec = spec_of("rangeWidth").expect("已登錄");
        assert!(spec.validate(ParamValue::Myriad(spec.max + 1)).is_err());
        assert!(spec.validate(ParamValue::Myriad(spec.min - 1)).is_err());
        assert_eq!(spec.validate(ParamValue::Myriad(spec.max)), Ok(()));
    }

    #[test]
    fn 型別不符被攔下() {
        let spec = spec_of("rangeWidth").expect("已登錄");
        assert!(
            matches!(
                spec.validate(ParamValue::Count(1)),
                Err(ParamError::TypeMismatch { .. })
            ),
            "萬分比欄位不接受計數值"
        );
    }

    #[test]
    fn 未登錄的欄位查不到() {
        assert!(spec_of("notARealParameter").is_none(), "未登錄參數不得被接受");
    }
}
