//! Bot 設定的跨 IPC 表示（面板 B／C）。
//!
//! 參數的**權威定義在 Rust**（`poker_engine::bot::params` 的 20 個
//! `ParamSpec`）：鍵、顯示名、單位、說明與上下限都從那裡送到前端，
//! 前端不另外抄一份。抄一份的後果是引擎改了範圍而 UI 不知道，
//! 使用者會拉到一個引擎會拒絕的值。

use std::collections::BTreeMap;

use poker_engine::bot::params::{spec_of, ParamValue, BEHAVIOR_SPECS, PERSONA_SPECS};
use poker_engine::bot::BotConfig;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 一個參數的完整規格，供 UI 渲染欄位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ParamSpecView {
    pub key: String,
    pub display: String,
    pub unit: String,
    pub description: String,
    /// `persona`（人格層）或 `behavior`（行為層）
    pub layer: String,
    /// `myriad`／`count`／`flag`／`enum`。決定 UI 用哪種控制項
    pub kind: String,
    /// 這三個值的量級是萬分比（上限 16000）或小計數，遠在 JS 安全整數之內。
    /// 標成 number 而非 bigint，前端才不必為了顯示一個百分比做型別轉換
    #[ts(type = "number")]
    pub default: i64,
    #[ts(type = "number")]
    pub min: i64,
    #[ts(type = "number")]
    pub max: i64,
    /// 在目前的內容下，調整這個欄位會不會改變決策。
    ///
    /// `false` 的欄位 UI 必須畫成停用並說明原因。畫成可調的話，
    /// 使用者會拉一個不會有事的滑桿，然後以為自己調到了東西
    pub implemented: bool,
}

fn kind_of(value: ParamValue) -> &'static str {
    match value {
        ParamValue::Myriad(_) => "myriad",
        ParamValue::Count(_) => "count",
        ParamValue::Flag(_) => "flag",
        ParamValue::Enum(_) => "enum",
    }
}

fn numeric(value: ParamValue) -> i64 {
    match value {
        ParamValue::Myriad(v) | ParamValue::Count(v) => i64::from(v),
        ParamValue::Enum(v) => i64::from(v),
        ParamValue::Flag(v) => i64::from(v),
    }
}

/// 全部 20 個參數的規格，人格層在前。
#[must_use]
pub fn all_specs() -> Vec<ParamSpecView> {
    PERSONA_SPECS
        .iter()
        .map(|spec| (spec, "persona"))
        .chain(BEHAVIOR_SPECS.iter().map(|spec| (spec, "behavior")))
        .map(|(spec, layer)| ParamSpecView {
            key: spec.key.to_owned(),
            display: spec.display.to_owned(),
            unit: spec.unit.to_owned(),
            description: spec.description.to_owned(),
            layer: layer.to_owned(),
            kind: kind_of(spec.default).to_owned(),
            default: numeric(spec.default),
            min: i64::from(spec.min),
            max: i64::from(spec.max),
            implemented: spec.implemented,
        })
        .collect()
}

/// 一個座位的 Bot 設定。
///
/// `params` 只帶**與預設不同**的欄位。全帶會讓 `RunManifest` 塞滿
/// 一堆沒改過的值，日後要看「這個 run 到底調了什麼」就得逐欄比對。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/poker-types/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BotSeatConfig {
    pub name: String,
    /// 參數鍵 → 數值。型別由 `ParamSpec` 決定，因此這裡統一是整數
    #[ts(type = "Record<string, number>")]
    pub params: BTreeMap<String, i64>,
}

impl BotSeatConfig {
    /// 轉為引擎的 [`BotConfig`]。
    ///
    /// # Errors
    /// 參數未登錄或值越界時回傳說明。**不靜默忽略**：使用者以為調了，
    /// 結果引擎沒收到，跑出來的統計就對不上他看到的設定。
    pub fn to_bot_config(&self) -> Result<BotConfig, String> {
        let mut config = BotConfig::defaults(if self.name.is_empty() {
            "未命名"
        } else {
            &self.name
        });

        for (key, &raw) in &self.params {
            if key.starts_with("openSizeCentiBb") && raw != 0 && !(200..=10000).contains(&raw) {
                return Err(format!("{key} 必須為 0（繼承）或 2～100 BB"));
            }
            // 舊存檔還帶著已移除的欄位。整份拒絕會讓使用者的設定讀不回來，
            // 而那個欄位本來就沒有作用——忽略並記一筆比較誠實
            // （0907 計劃 §6.3）
            if DEPRECATED_KEYS.contains(&key.as_str()) {
                crate::log::info(&format!(
                    "忽略已移除的 Bot 參數 {key}：牌力組固定為八組後它不再有作用"
                ));
                continue;
            }
            let spec = spec_of(key).ok_or_else(|| format!("未登錄的參數：{key}"))?;
            // 值的型別跟著 spec 的預設值走，前端只需傳數字
            let value = match spec.default {
                ParamValue::Myriad(_) => ParamValue::Myriad(clamp_u32(raw)),
                ParamValue::Count(_) => ParamValue::Count(clamp_u32(raw)),
                ParamValue::Enum(_) => {
                    ParamValue::Enum(u8::try_from(raw.clamp(0, 255)).unwrap_or(0))
                }
                ParamValue::Flag(_) => ParamValue::Flag(raw != 0),
            };
            let applied = if PERSONA_SPECS.iter().any(|s| s.key == spec.key) {
                config.set_persona(spec.key, value)
            } else {
                config.set_behavior(spec.key, value)
            };
            applied.map_err(|e| format!("參數 {key} 不合法：{e:?}"))?;
        }
        Ok(config)
    }
}

/// 曾經存在、現已從 schema 移除的參數鍵。
///
/// `postflopBucketCount` 的舊範圍是 2–24，用來調翻後牌力分桶的粒度。
/// 牌力組固定為八組之後它不再是有效的可調參數，因此從表上移除；
/// 但舊存檔仍然帶著它，匯入時接受並忽略。
const DEPRECATED_KEYS: [&str; 1] = ["postflopBucketCount"];

fn clamp_u32(value: i64) -> u32 {
    u32::try_from(value.clamp(0, i64::from(u32::MAX))).unwrap_or(0)
}

/// 把逐座設定展開成引擎需要的清單。
///
/// 座位數不足時以預設補齊——桌上永遠有 `players` 個座位，
/// 少一個就代表那個座位沒有策略可用。
///
/// # Errors
/// 任一座設定不合法時回傳說明。
pub fn to_bot_configs(seats: &[BotSeatConfig], players: usize) -> Result<Vec<BotConfig>, String> {
    let mut configs = Vec::with_capacity(players);
    for index in 0..players {
        match seats.get(index) {
            Some(seat) => configs.push(seat.to_bot_config()?),
            None => configs.push(BotConfig::defaults(format!("座位 {index}"))),
        }
    }
    Ok(configs)
}

/// 工程用示範組合。
///
/// **這不是校準過的人格。** 核心規格的 7 組官方人格由牌手顧問定義，
/// 目前還沒進來。這幾組只是既有參數的組合，用途是讓面板 B／C 有東西
/// 可以指派、讓使用者一眼看出參數確實會改變結果。
#[must_use]
pub fn demo_presets() -> Vec<BotSeatConfig> {
    vec![
        BotSeatConfig {
            name: "標準".to_owned(),
            params: BTreeMap::new(),
        },
        BotSeatConfig {
            name: "緊凶".to_owned(),
            params: [
                ("rangeWidth".to_owned(), 8_000),
                ("preflopAggression".to_owned(), 13_000),
                ("foldDiscipline".to_owned(), 12_000),
                ("callPersistence".to_owned(), 7_000),
            ]
            .into_iter()
            .collect(),
        },
        BotSeatConfig {
            name: "鬆被動".to_owned(),
            params: [
                ("rangeWidth".to_owned(), 14_000),
                ("preflopAggression".to_owned(), 6_000),
                ("callPersistence".to_owned(), 14_000),
                ("foldDiscipline".to_owned(), 7_000),
            ]
            .into_iter()
            .collect(),
        },
        BotSeatConfig {
            name: "鬆凶".to_owned(),
            params: [
                ("rangeWidth".to_owned(), 14_000),
                ("preflopAggression".to_owned(), 14_000),
                ("callPersistence".to_owned(), 11_000),
                ("foldDiscipline".to_owned(), 8_000),
            ]
            .into_iter()
            .collect(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 已移除的參數在舊存檔裡被忽略而不是讓整份設定讀不回來() {
        let seat = BotSeatConfig {
            name: "舊存檔".to_owned(),
            params: [
                ("rangeWidth".to_owned(), 8_000),
                // 牌力組固定為八組後這個欄位不再有作用（0907 計劃 §6.3）
                ("postflopBucketCount".to_owned(), 12),
            ]
            .into_iter()
            .collect(),
        };

        let config = seat.to_bot_config().expect("舊存檔必須仍然讀得回來");
        assert_eq!(
            config
                .effective("rangeWidth")
                .and_then(ParamValue::as_myriad),
            Some(8_000),
            "其他欄位照常生效"
        );
        assert!(
            config.effective("postflopBucketCount").is_none(),
            "已移除的欄位不得偷偷復活"
        );
    }

    #[test]
    fn 真正未登錄的參數仍然被拒絕() {
        let seat = BotSeatConfig {
            name: "亂填".to_owned(),
            params: [("notARealParameter2".to_owned(), 1)].into_iter().collect(),
        };
        assert!(
            seat.to_bot_config().is_err(),
            "核心規格 4.3：不得直接注入未登錄參數"
        );
    }

    #[test]
    fn 已移除的參數不再出現在_schema() {
        assert!(
            !all_specs()
                .iter()
                .any(|spec| spec.key == "postflopBucketCount"),
            "無作用的滑桿不該畫在面板上"
        );
    }
}
