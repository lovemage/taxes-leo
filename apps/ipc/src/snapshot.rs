//! 內容快照（核心規格 3.3）。
//!
//! > 「內容本身必須保存，只留 hash 不合格。」
//!
//! 因此這裡把實際生效的 `BaselineRules` 與逐座 `BotConfig` 攤成 JSON。
//! 不是名稱字串，也不是「與預設的差異」——差異需要對照當時的預設值才
//! 讀得回來，而預設值本身會隨版本改變。留下來的必須是自足的。
//!
//! # 為什麼寫在這裡而不是引擎
//!
//! `poker-engine` 沒有任何外部相依（連 serde 都沒有），那是刻意的：
//! 規則引擎不該因為序列化格式的選擇而綁上第三方套件。序列化是這一層
//! 的職責，因此由這裡走引擎的公開介面把內容讀出來。

use poker_engine::bot::params::{ParamValue, BEHAVIOR_SPECS, PERSONA_SPECS};
use poker_engine::bot::BotConfig;
use poker_engine::strategy::baseline::BaselineRules;
use poker_engine::strategy::playability::PlayabilityCategory;
use poker_engine::strategy::ScenarioWidths;
use serde_json::{json, Map, Value};

fn widths(widths: ScenarioWidths) -> Value {
    json!({
        "aggressiveEarliest": widths.aggressive_earliest,
        "aggressiveLatest": widths.aggressive_latest,
        "callExtra": widths.call_extra,
        "mixBand": widths.mix_band,
    })
}

/// 完整的基準規則內容。
#[must_use]
pub fn baseline(rules: &BaselineRules) -> Value {
    let opening: Map<String, Value> = rules
        .opening
        .entries()
        .into_iter()
        .map(|((seated, position), width)| {
            (format!("{seated}max/{}", position.as_str()), json!(width))
        })
        .collect();

    let vs_open: Map<String, Value> = rules
        .vs_open_width
        .entries()
        .into_iter()
        .map(|((seated, hero, opener), width)| {
            (
                format!("{seated}max/{}/vs-{}", hero.as_str(), opener.as_str()),
                json!(width),
            )
        })
        .collect();

    let playability: Map<String, Value> = PlayabilityCategory::ALL
        .iter()
        .map(|&category| {
            (
                category.as_str().to_owned(),
                json!(rules.playability.of(category)),
            )
        })
        .collect();

    // 逐格覆寫是「參數表達不出來的意見」，屬於內容的一部分，
    // 只記筆數等於把顧問的判斷丟掉
    let overrides: Vec<Value> = rules
        .overrides
        .entries()
        .into_iter()
        .map(|((node, class), cell)| {
            json!({
                "node": node.key(),
                "class": class.label(),
                "aggressive": cell.aggressive(),
                "call": cell.call(),
                "fold": cell.fold(),
            })
        })
        .collect();

    json!({
        "name": rules.name,
        "version": rules.version,
        "consultantApproved": rules.consultant_approved,
        "scenarioWidths": {
            "unopened": widths(rules.unopened),
            "vsLimp": widths(rules.vs_limp),
            "vsOpen": widths(rules.vs_open),
            "vsThreeBet": widths(rules.vs_three_bet),
            "vsFourBet": widths(rules.vs_four_bet),
            "vsSqueeze": widths(rules.vs_squeeze),
        },
        "bucketMultiplier": rules.bucket_multiplier,
        "pushFoldBelow": rules.push_fold_below.as_str(),
        "openingWidths": opening,
        "vsOpenWidths": vs_open,
        "playability": playability,
        "cellOverrides": overrides,
        "raiseSizesCentiBb": {
            "open": rules.open_size_centi_bb,
            "threeBet": rules.three_bet_size_centi_bb,
            "fourBet": rules.four_bet_size_centi_bb,
        },
    })
}

/// 一組 Bot 的**全部 21 個生效值**。
///
/// 記生效值而不是「改過的欄位」：改過的欄位要配上當時的預設值才讀得回
/// 完整設定，而預設值會隨版本改變——那樣的紀錄過幾個版本就再也還原不出
/// 當初到底跑了什麼。
#[must_use]
pub fn bot(config: &BotConfig) -> Value {
    let params: Map<String, Value> = PERSONA_SPECS
        .iter()
        .chain(BEHAVIOR_SPECS.iter())
        .map(|spec| {
            let value = config.effective(spec.key).unwrap_or(spec.default);
            (spec.key.to_owned(), json!(numeric(value)))
        })
        .collect();

    json!({
        "name": config.name,
        "params": params,
    })
}

fn numeric(value: ParamValue) -> i64 {
    match value {
        ParamValue::Myriad(v) | ParamValue::Count(v) => i64::from(v),
        ParamValue::Enum(v) => i64::from(v),
        ParamValue::Flag(v) => i64::from(v),
    }
}
