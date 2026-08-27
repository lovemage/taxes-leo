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
use poker_engine::strategy::default_chart::{
    raise_size_centi_bb, ChartAction, ChartScenario, DefaultChart, CHART_VERSION,
};
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
        "defaultChart": default_chart(rules),
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

/// 顧問的預設組合表。
///
/// 核心規格 3.3：「內容本身必須保存，只留 hash 不合格。」表是這個 run
/// 翻前實際使用的內容，因此逐格存下手牌清單，而不是只記一個版本字串。
///
/// 「其餘手牌」與「全部手牌」兩種列只記選擇子不展開：它們是其餘四列的
/// 餘數，展開後每一格會多出上百個牌類，紀錄膨脹好幾倍卻沒有多帶資訊。
/// 讀回來時照 `DefaultChart` 的同一條規則補滿即可。
fn default_chart(rules: &BaselineRules) -> Value {
    if !rules.use_default_chart {
        return json!({ "enabled": false });
    }
    let chart = match DefaultChart::embedded() {
        Ok(chart) => chart,
        // 載入失敗時整份內容退回參數產生器。紀錄必須說出這件事，
        // 否則事後看不出這個 run 跑的根本不是顧問的表
        Err(error) => {
            return json!({ "enabled": true, "loaded": false, "error": error.to_string() })
        }
    };

    let cells: Map<String, Value> = chart
        .entries()
        .into_iter()
        .map(|entry| {
            let rows: Map<String, Value> = entry
                .rows()
                .iter()
                .map(|row| {
                    let hands = if row.selector_key() == "list" {
                        row.classes()
                            .iter()
                            .map(|class| class.label())
                            .collect::<Vec<_>>()
                            .join(",")
                    } else {
                        row.selector_key().to_owned()
                    };
                    (row.action.as_str().to_owned(), json!(hands))
                })
                .collect();
            (
                format!(
                    "{}/{}/{}",
                    entry.depth.as_str(),
                    entry.position.as_str(),
                    entry.scenario.as_str()
                ),
                Value::Object(rows),
            )
        })
        .collect();

    let sizes: Map<String, Value> = ChartScenario::all()
        .into_iter()
        .map(|scenario| {
            (
                scenario.as_str().to_owned(),
                json!({
                    "small": raise_size_centi_bb(scenario, ChartAction::RaiseSmall),
                    "large": raise_size_centi_bb(scenario, ChartAction::RaiseLarge),
                }),
            )
        })
        .collect();

    json!({
        "enabled": true,
        "loaded": true,
        "source": chart.source,
        "version": CHART_VERSION,
        "format": chart.format,
        "cells": cells,
        "raiseSizesCentiBb": sizes,
        // 人格對表的邊界位移。使用者座位恆為中性，Bot 逐座的值另存於
        // bot_personas 的人格參數裡
        "shift": {
            "rangeWidth": rules.chart_shift.range_width,
            "aggression": rules.chart_shift.aggression,
            "callPersistence": rules.chart_shift.call_persistence,
            "foldDiscipline": rules.chart_shift.fold_discipline,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 核心規格 3.3：內容本身必須保存。表是翻前實際使用的內容，
    /// 因此快照裡要有逐格的手牌清單，不是只有一個版本字串。
    #[test]
    fn 快照存下預設組合表的逐格內容() {
        let rules = BaselineRules::engineering_placeholder();
        let snapshot = baseline(&rules);
        let chart = &snapshot["defaultChart"];

        assert_eq!(chart["enabled"], json!(true));
        assert_eq!(chart["loaded"], json!(true));
        assert_eq!(chart["version"], json!(CHART_VERSION));
        assert_eq!(
            chart["cells"].as_object().expect("逐格內容").len(),
            180,
            "四檔深度 × 九個位置 × 五種情境"
        );

        let cell = &chart["cells"]["0-15/UTG/unopened"];
        assert_eq!(cell["fold"], json!("*"), "「其餘手牌」記選擇子不展開");
        assert_eq!(cell["call"], json!("-"));
        assert!(
            cell["allin"]
                .as_str()
                .expect("推入清單")
                .starts_with("AA,KK,QQ"),
            "推入範圍要逐手存下來"
        );

        // 尺度也是內容的一部分：表的倍數乘的是前方最大下注額
        assert_eq!(chart["raiseSizesCentiBb"]["open"]["small"], json!(625));
    }

    /// 關掉表時快照要說出來，否則事後看不出這個 run 跑的不是顧問的內容。
    #[test]
    fn 關掉表時快照如實標示() {
        let mut rules = BaselineRules::engineering_placeholder();
        rules.use_default_chart = false;
        assert_eq!(baseline(&rules)["defaultChart"]["enabled"], json!(false));
    }

    /// 快照是每個 run 都要寫一份的，體積要留意。
    #[test]
    fn 快照體積留在可接受範圍內() {
        let rules = BaselineRules::engineering_placeholder();
        let size = baseline(&rules).to_string().len();
        assert!(
            size < 80_000,
            "翻前快照 {size} 位元組偏大——每個 run 都會存一份"
        );
    }
}
