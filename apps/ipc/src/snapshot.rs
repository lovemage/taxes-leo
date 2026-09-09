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
use poker_engine::strategy::postflop::{
    PostflopCondition, PostflopIntentDistribution, PostflopLineCondition, RuleSet,
};
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

/// 一組 Bot 的**全部 20 個生效值**。
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

// ── 翻後（0907 計劃 §6.2）────────────────────────────────────────────

/// 完整的翻後規則列。
///
/// 核心規格 3.3 要求 manifest 能在**沒有外部版本表**的情況下獨立重建
/// 當次策略，因此存的是每一條規則的條件與意圖頻率，不是「規則集版本
/// v1」這種指標。版本字串會被重新定義，內容不會。
///
/// 使用者覆寫也在同一份清單裡（來源層 `userOverride`），因此重建時
/// 連解析順序都是完整的。
#[must_use]
pub fn postflop(rules: &RuleSet) -> Value {
    let entries: Vec<Value> = rules
        .rules()
        .iter()
        .map(|rule| {
            json!({
                "id": rule.id,
                "name": rule.name,
                "source": rule.source.key(),
                "version": rule.version,
                "consultantApproved": rule.consultant_approved,
                "condition": condition(&rule.condition),
                "intent": intent(&rule.intent),
            })
        })
        .collect();

    json!({
        "nodeSetVersion": poker_engine::strategy::postflop::NODE_SET_VERSION,
        "fallbackVersion": rules.fallback_version,
        "ruleCount": entries.len(),
        "rules": entries,
    })
}

/// 一條規則的條件。省略的欄位就是萬用，因此只寫出有指定的那些——
/// 全部寫出來的話，一條只指定牌力組的通則會多出十幾個 `null`。
fn condition(condition: &PostflopCondition) -> Value {
    let mut map = Map::new();
    if let Some(street) = condition.street {
        map.insert("street".to_owned(), json!(format!("{street:?}").to_lowercase()));
    }
    if let Some(situation) = condition.situation {
        map.insert("situation".to_owned(), json!(situation.key()));
    }
    if let Some(surface) = condition.board_surface {
        map.insert("boardSurface".to_owned(), json!(surface.key()));
    }
    if let Some(connectivity) = condition.board_connectivity {
        map.insert("boardConnectivity".to_owned(), json!(connectivity.key()));
    }
    if let Some(strength) = condition.hand_strength {
        map.insert("handStrength".to_owned(), json!(strength.key()));
    }
    if let Some(size) = condition.facing_size {
        map.insert("facingSize".to_owned(), json!(size.key()));
    }
    if let Some(bucket) = condition.effective_stack_bucket {
        map.insert("effectiveStackBucket".to_owned(), json!(bucket.as_str()));
    }
    if let Some(pot) = condition.pot_type {
        map.insert("potType".to_owned(), json!(format!("{pot:?}")));
    }
    if let Some(position) = condition.hero_position {
        map.insert("heroPosition".to_owned(), json!(position.as_str()));
    }
    if let Some(range) = condition.active_players.as_ref() {
        map.insert("activePlayers".to_owned(), json!([range.start(), range.end()]));
    }
    if let Some(range) = condition.opponents_behind.as_ref() {
        map.insert("opponentsBehind".to_owned(), json!([range.start(), range.end()]));
    }
    if let Some(range) = condition.spr_centi.as_ref() {
        map.insert("sprCenti".to_owned(), json!([range.start(), range.end()]));
    }
    let line = line_condition(&condition.line);
    if !line.is_empty() {
        map.insert("line".to_owned(), Value::Object(line));
    }
    Value::Object(map)
}

fn line_condition(line: &PostflopLineCondition) -> Map<String, Value> {
    let mut map = Map::new();
    if let Some(role) = line.previous_street_aggressor {
        map.insert("previousStreetAggressor".to_owned(), json!(role.key()));
    }
    if let Some(role) = line.last_aggressor_before_current_street {
        map.insert(
            "lastAggressorBeforeCurrentStreet".to_owned(),
            json!(role.key()),
        );
    }
    if let Some(order) = line.relative_aggressor_order {
        map.insert("relativeAggressorOrder".to_owned(), json!(order.key()));
    }
    if let Some(checked) = line.hero_checked_this_street {
        map.insert("heroCheckedThisStreet".to_owned(), json!(checked));
    }
    if let Some(range) = line.current_street_bet_count.as_ref() {
        map.insert(
            "currentStreetBetCount".to_owned(),
            json!([range.start(), range.end()]),
        );
    }
    if let Some(range) = line.current_street_raise_count.as_ref() {
        map.insert(
            "currentStreetRaiseCount".to_owned(),
            json!([range.start(), range.end()]),
        );
    }
    map
}

/// 尺寸意圖頻率。存意圖而不是換算後的籌碼：同一條規則在不同底池下
/// 要下的注不同，存死了就不是原本那條規則。
fn intent(intent: &PostflopIntentDistribution) -> Value {
    let weights: Map<String, Value> = intent
        .weights()
        .iter()
        .map(|(kind, myriad)| (kind.key().to_owned(), json!(myriad)))
        .collect();
    Value::Object(weights)
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

    // ── 翻後（0907 計劃 §6.2）────────────────────────────────────

    #[test]
    fn 翻後快照存的是完整內容而不是版本指標() {
        let rules = crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default());
        let snapshot = postflop(&rules);

        let entries = snapshot["rules"].as_array().expect("規則列");
        assert_eq!(entries.len(), 16, "十六條工程通則");
        assert_eq!(snapshot["nodeSetVersion"], "postflop-nodes/v1");

        for entry in entries {
            assert!(
                entry["condition"].is_object(),
                "每條規則都要有條件，否則重建不出來"
            );
            assert!(
                entry["intent"].as_object().is_some_and(|map| !map.is_empty()),
                "每條規則都要有意圖頻率"
            );
            assert!(entry["source"].is_string());
            assert_eq!(
                entry["consultantApproved"], false,
                "工程通則未經簽核，快照必須誠實記下來"
            );
        }
    }

    #[test]
    fn 使用者覆寫也進同一份快照() {
        let overrides = crate::postflop::PostflopOverridesView {
            nodes: vec![crate::postflop::PostflopNodeOverrideView {
                node_key: "flop|no-bet|cbet-chance|rainbow|dry|strong-made|none".to_owned(),
                weights: vec![crate::postflop::PostflopWeightInput {
                    kind: "check".to_owned(),
                    myriad: 10_000,
                }],
            }],
            rules: Vec::new(),
        };
        let rules = crate::postflop::to_rule_set(&overrides);
        let snapshot = postflop(&rules);
        let entries = snapshot["rules"].as_array().expect("規則列");

        assert_eq!(entries.len(), 17, "十六條通則加一條使用者覆寫");
        // 使用者覆寫排在最前面：解析順序也必須能從快照重建
        assert_eq!(entries[0]["source"], "user-override");
        assert_eq!(entries[0]["intent"]["check"], 10_000);
    }

    #[test]
    fn 翻後快照的體積留在預算內() {
        let rules = crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default());
        let size = postflop(&rules).to_string().len();
        assert!(
            size < 80_000,
            "翻後規則列 {size} 位元組偏大——與翻前共用同一個 80 KB 閘門"
        );
    }

    #[test]
    fn 條件只寫出有指定的欄位() {
        let rules = crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default());
        let snapshot = postflop(&rules);
        let condition = snapshot["rules"][0]["condition"]
            .as_object()
            .expect("條件");

        // 工程通則只指定下注狀態與牌力組，其餘萬用。全部寫出來的話
        // 會多出十幾個 null，體積白白翻倍
        assert!(condition.contains_key("situation"));
        assert!(condition.contains_key("handStrength"));
        assert!(!condition.contains_key("heroPosition"));
        assert!(!condition.contains_key("sprCenti"));
    }
}
