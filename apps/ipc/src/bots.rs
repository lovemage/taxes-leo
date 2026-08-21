//! Bot 設定的跨 IPC 表示（面板 B／C）。
//!
//! 參數的**權威定義在 Rust**（`poker_engine::bot::params` 的 21 個
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

/// 全部 21 個參數的規格，人格層在前。
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
            let spec = spec_of(key).ok_or_else(|| format!("未登錄的參數：{key}"))?;
            // 值的型別跟著 spec 的預設值走，前端只需傳數字
            let value = match spec.default {
                ParamValue::Myriad(_) => ParamValue::Myriad(clamp_u32(raw)),
                ParamValue::Count(_) => ParamValue::Count(clamp_u32(raw)),
                ParamValue::Enum(_) => ParamValue::Enum(u8::try_from(raw.clamp(0, 255)).unwrap_or(0)),
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
