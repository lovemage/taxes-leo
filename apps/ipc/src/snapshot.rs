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
        map.insert("street".to_owned(), json!(crate::postflop::street_key(street)));
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
        map.insert("potType".to_owned(), json!(pot.key()));
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

/// 由快照內容重建翻後規則集。
///
/// 驗收條件 15：「每次 run 預設可只靠 manifest 內的完整翻後內容與 hash
/// 還原策略」。存了內容卻沒有讀回來的路徑，那句話就只是宣稱——
/// `快照可以完整重建回規則集` 測試走的就是這一支。
///
/// 回傳 `None` 代表快照結構不對；個別規則解析不出來時**整份失敗**，
/// 不做部分還原：少了一條規則的規則集會安靜地打出不一樣的牌。
#[must_use]
pub fn rebuild_postflop(snapshot: &Value) -> Option<RuleSet> {
    let entries = snapshot.get("rules")?.as_array()?;
    let fallback_version = snapshot.get("fallbackVersion")?.as_str()?.to_owned();

    // 節點集合版本是內容的一部分：沒有它就分不出這份規則是對哪一版節點
    // 寫的，跨版本比較也失去意義
    snapshot.get("nodeSetVersion")?.as_str()?;
    // 筆數對不上代表清單被截斷或改過。少一條規則的規則集會安靜地打出
    // 不一樣的牌，因此寧可拒絕還原
    if snapshot.get("ruleCount")?.as_u64()? != u64::try_from(entries.len()).ok()? {
        return None;
    }

    let mut rules = Vec::with_capacity(entries.len());
    for entry in entries {
        rules.push(rebuild_rule(entry)?);
    }
    Some(RuleSet::new(rules, fallback_version))
}

fn rebuild_rule(entry: &Value) -> Option<poker_engine::strategy::postflop::PostflopRule> {
    use poker_engine::strategy::postflop::{PostflopRule, RuleSource};

    let source = match entry.get("source")?.as_str()? {
        "user-override" => RuleSource::UserOverride,
        "official" => RuleSource::Official,
        "generic" => RuleSource::Generic,
        "engineering-fallback" => RuleSource::EngineeringFallback,
        _ => return None,
    };

    let weights: Vec<_> = entry
        .get("intent")?
        .as_object()?
        .iter()
        .map(|(kind, myriad)| {
            Some((
                crate::postflop::parse_action_kind(kind)?,
                u32::try_from(myriad.as_u64()?).ok()?,
            ))
        })
        .collect::<Option<_>>()?;

    Some(PostflopRule {
        id: entry.get("id")?.as_str()?.to_owned(),
        name: entry.get("name")?.as_str()?.to_owned(),
        condition: rebuild_condition(entry.get("condition")?)?,
        intent: PostflopIntentDistribution::new(weights).ok()?,
        source,
        version: entry.get("version")?.as_str()?.to_owned(),
        consultant_approved: entry.get("consultantApproved")?.as_bool()?,
    })
}

/// 由快照重建一條規則的條件。
///
/// 兩件事都不能省：
///
/// 1. **每個可保存的欄位都要讀回來。** 少讀一個，原本只適用 3-bet pot、
///    BTN、某個深度的規則還原後就變成萬用，適用範圍比使用者寫的更大，
///    重播出來的策略也就不是當初跑的那一份。因此這裡逐欄列出、
///    不寫 `..Default::default()`——條件多一個欄位就要編譯失敗。
/// 2. **欄位缺席才是萬用；有值卻解析不出來是錯誤。** 兩者都收斂成同一個
///    `None` 的話，`handStrength: "typo"` 會被讀成「不限牌力」，
///    而 [`rebuild_postflop`] 宣告的是「個別規則解析不出來時整份失敗」。
fn rebuild_condition(value: &Value) -> Option<PostflopCondition> {
    let map = value.as_object()?;

    Some(PostflopCondition {
        street: enum_field(map, "street", crate::postflop::parse_street)?,
        situation: enum_field(map, "situation", crate::postflop::parse_situation)?,
        board_surface: enum_field(map, "boardSurface", crate::postflop::parse_surface)?,
        board_connectivity: enum_field(
            map,
            "boardConnectivity",
            crate::postflop::parse_connectivity,
        )?,
        hand_strength: enum_field(map, "handStrength", crate::postflop::parse_hand_strength)?,
        active_players: range_field::<u8>(map, "activePlayers")?,
        hero_position: enum_field(map, "heroPosition", crate::postflop::parse_position)?,
        opponents_behind: range_field::<u8>(map, "opponentsBehind")?,
        pot_type: enum_field(map, "potType", crate::postflop::parse_pot_type)?,
        facing_size: enum_field(map, "facingSize", crate::postflop::parse_facing_size)?,
        spr_centi: range_field::<u32>(map, "sprCenti")?,
        effective_stack_bucket: enum_field(
            map,
            "effectiveStackBucket",
            crate::postflop::parse_stack_bucket,
        )?,
        line: rebuild_line(map.get("line"))?,
    })
}

/// 缺席（或 `null`）是萬用，回 `Some(None)`；有值但不是字串、或字串解析
/// 不出來，回 `None` 讓整份還原失敗。
fn enum_field<T>(
    map: &Map<String, Value>,
    key: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Option<Option<T>> {
    match map.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(parse(value.as_str()?)?)),
    }
}

fn bool_field(map: &Map<String, Value>, key: &str) -> Option<Option<bool>> {
    match map.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(value.as_bool()?)),
    }
}

/// 範圍必須是**恰好兩個**端點的陣列，兩端都要落在目標型別內。
///
/// 長度不檢查的話，`[3]` 會被讀成一個端點缺失的範圍，`[3, 4, 5]` 則會
/// 安靜地丟掉第三個值——兩種都代表寫入端或檔案有問題，不該當成可還原。
fn range_field<T: TryFrom<u64>>(
    map: &Map<String, Value>,
    key: &str,
) -> Option<Option<std::ops::RangeInclusive<T>>> {
    match map.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(value) => {
            let pair = value.as_array()?;
            if pair.len() != 2 {
                return None;
            }
            let start = T::try_from(pair[0].as_u64()?).ok()?;
            let end = T::try_from(pair[1].as_u64()?).ok()?;
            Some(Some(start..=end))
        }
    }
}

/// 線路條件。與 [`rebuild_condition`] 同樣的嚴格度：缺席是萬用，
/// 有值卻解析不出來就整份失敗。
fn rebuild_line(value: Option<&Value>) -> Option<PostflopLineCondition> {
    use poker_engine::strategy::postflop::{AggressorRole, RelativeAggressorOrder};

    let map = match value {
        None | Some(Value::Null) => return Some(PostflopLineCondition::default()),
        Some(value) => value.as_object()?,
    };
    let role = |key: &str| {
        enum_field(map, key, |text| {
            AggressorRole::ALL.into_iter().find(|role| role.key() == text)
        })
    };

    Some(PostflopLineCondition {
        previous_street_aggressor: role("previousStreetAggressor")?,
        last_aggressor_before_current_street: role("lastAggressorBeforeCurrentStreet")?,
        relative_aggressor_order: enum_field(map, "relativeAggressorOrder", |text| {
            RelativeAggressorOrder::ALL
                .into_iter()
                .find(|order| order.key() == text)
        })?,
        hero_checked_this_street: bool_field(map, "heroCheckedThisStreet")?,
        current_street_bet_count: range_field::<u8>(map, "currentStreetBetCount")?,
        current_street_raise_count: range_field::<u8>(map, "currentStreetRaiseCount")?,
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

    // ── 翻後（0907 計劃 §6.2）────────────────────────────────────

    #[test]
    fn 翻後快照存的是完整內容而不是版本指標() {
        let rules = crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default())
            .expect("預設覆寫");
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
        let rules = crate::postflop::to_rule_set(&overrides).expect("覆寫合法");
        let snapshot = postflop(&rules);
        let entries = snapshot["rules"].as_array().expect("規則列");

        assert_eq!(entries.len(), 17, "十六條通則加一條使用者覆寫");
        // 使用者覆寫排在最前面：解析順序也必須能從快照重建
        assert_eq!(entries[0]["source"], "user-override");
        assert_eq!(entries[0]["intent"]["check"], 10_000);
    }

    #[test]
    fn 翻後快照的體積留在預算內() {
        let rules = crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default())
            .expect("預設覆寫");
        let size = postflop(&rules).to_string().len();
        assert!(
            size < 80_000,
            "翻後規則列 {size} 位元組偏大——與翻前共用同一個 80 KB 閘門"
        );
    }

    #[test]
    fn 快照可以完整重建回規則集() {
        // 驗收條件 15：只靠 manifest 內的內容就要還原得出當次策略。
        // 存了內容卻沒有讀回來的路徑，那句話就只是宣稱
        let overrides = crate::postflop::PostflopOverridesView {
            nodes: vec![crate::postflop::PostflopNodeOverrideView {
                node_key: "turn|facing-bet|facing-cbet|rainbow|wet|bluff-catcher|two-thirds"
                    .to_owned(),
                weights: vec![
                    crate::postflop::PostflopWeightInput {
                        kind: "call".to_owned(),
                        myriad: 4_000,
                    },
                    crate::postflop::PostflopWeightInput {
                        kind: "fold".to_owned(),
                        myriad: 6_000,
                    },
                ],
            }],
            rules: Vec::new(),
        };
        let original = crate::postflop::to_rule_set(&overrides).expect("覆寫合法");

        let rebuilt = rebuild_postflop(&postflop(&original)).expect("重建");

        assert_eq!(rebuilt.rules().len(), original.rules().len());
        for (before, after) in original.rules().iter().zip(rebuilt.rules()) {
            assert_eq!(before.id, after.id);
            assert_eq!(before.source, after.source);
            assert_eq!(before.consultant_approved, after.consultant_approved);
            assert_eq!(
                before.condition, after.condition,
                "條件必須逐欄還原，否則重建出來的規則會命中不同的節點"
            );
            assert_eq!(before.intent, after.intent);
        }

        // 重建後再存一次應該得到同一份內容——hash 才有意義
        assert_eq!(postflop(&original), postflop(&rebuilt));
    }

    #[test]
    fn 快照結構不對時整份失敗而不是部分還原() {
        // 少了一條規則的規則集會安靜地打出不一樣的牌
        let mut snapshot = postflop(
            &crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default())
                .expect("預設覆寫"),
        );
        snapshot["rules"][3]["intent"] = json!({ "notAnAction": 10_000 });
        assert!(rebuild_postflop(&snapshot).is_none());

        assert!(rebuild_postflop(&json!({ "rules": [] })).is_none(), "缺欄位");
    }

    #[test]
    fn 每個可保存的條件欄位都還原得回來() {
        // 存了卻沒讀回來的欄位會靜靜地把規則放寬：只適用 3-bet pot、BTN、
        // 40–70BB 的一條規則，還原後變成「什麼情況都適用」
        use poker_engine::hand::Street;
        use poker_engine::position::PositionLabel;
        use poker_engine::strategy::decision::StackBucket;
        use poker_engine::strategy::postflop::{
            AggressorRole, BoardConnectivity, BoardSurface, FacingSize, HandStrength,
            PostflopIntentDistribution, PostflopRule, PostflopSituation, PotType,
            RelativeAggressorOrder, RuleSource,
        };

        let condition = PostflopCondition {
            street: Some(Street::Turn),
            situation: Some(PostflopSituation::FacingBet),
            board_surface: Some(BoardSurface::FlushDrawPaired),
            board_connectivity: Some(BoardConnectivity::Wet),
            hand_strength: Some(HandStrength::BluffCatcher),
            active_players: Some(2..=3),
            hero_position: Some(PositionLabel::Btn),
            opponents_behind: Some(0..=1),
            pot_type: Some(PotType::ThreeBet),
            facing_size: Some(FacingSize::TwoThirds),
            spr_centi: Some(150..=600),
            effective_stack_bucket: Some(StackBucket::Deep),
            line: PostflopLineCondition {
                previous_street_aggressor: Some(AggressorRole::Hero),
                last_aggressor_before_current_street: Some(AggressorRole::Opponent),
                relative_aggressor_order: Some(RelativeAggressorOrder::HeroAfter),
                hero_checked_this_street: Some(true),
                current_street_bet_count: Some(1..=1),
                current_street_raise_count: Some(0..=2),
            },
        };
        let original = RuleSet::new(
            vec![PostflopRule {
                id: "U-full".to_owned(),
                name: "每個欄位都填滿的覆寫".to_owned(),
                condition: condition.clone(),
                intent: PostflopIntentDistribution::new(vec![
                    (poker_engine::strategy::postflop::PostflopActionKind::Call, 4_000),
                    (poker_engine::strategy::postflop::PostflopActionKind::Fold, 6_000),
                ])
                .expect("意圖"),
                source: RuleSource::UserOverride,
                version: "user/rule".to_owned(),
                consultant_approved: false,
            }],
            "test/v1",
        );

        let rebuilt = rebuild_postflop(&postflop(&original)).expect("重建");
        assert_eq!(
            rebuilt.rules()[0].condition,
            condition,
            "條件必須逐欄還原，否則重建出來的規則會命中不同的節點"
        );
        assert_eq!(postflop(&original), postflop(&rebuilt), "再存一次要一模一樣");
    }

    #[test]
    fn 快照裡解析不出來的值是錯誤而不是萬用() {
        let base = || {
            postflop(
                &crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default())
                    .expect("預設覆寫"),
            )
        };

        // 欄位存在但內容非法：`and_then` 會把它變成 None，於是這條規則
        // 從「只管抓詐牌」擴張成「什麼牌力都管」
        let mut typo = base();
        typo["rules"][0]["condition"]["handStrength"] = json!("typo");
        assert!(rebuild_postflop(&typo).is_none(), "未知的牌力組必須拒絕");

        let mut bad_pot = base();
        bad_pot["rules"][0]["condition"]["potType"] = json!("ThreeBet");
        assert!(
            rebuild_postflop(&bad_pot).is_none(),
            "存的是 PotType::key，Debug 名稱不是合法輸入"
        );

        // 範圍要恰好兩個端點
        let mut short_range = base();
        short_range["rules"][0]["condition"]["activePlayers"] = json!([2]);
        assert!(rebuild_postflop(&short_range).is_none(), "缺一個端點");

        let mut long_range = base();
        long_range["rules"][0]["condition"]["activePlayers"] = json!([2, 3, 4]);
        assert!(rebuild_postflop(&long_range).is_none(), "多一個端點");

        let mut bad_line = base();
        bad_line["rules"][0]["condition"]["line"] = json!({ "previousStreetAggressor": "nobody" });
        assert!(rebuild_postflop(&bad_line).is_none(), "未知的主動方");

        let mut bad_flag = base();
        bad_flag["rules"][0]["condition"]["line"] = json!({ "heroCheckedThisStreet": "yes" });
        assert!(rebuild_postflop(&bad_flag).is_none(), "布林欄位不接受字串");
    }

    #[test]
    fn 規則被截斷時整份拒絕() {
        let mut snapshot = postflop(
            &crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default())
                .expect("預設覆寫"),
        );
        let count = snapshot["rules"].as_array().expect("規則列").len();
        snapshot["rules"]
            .as_array_mut()
            .expect("規則列")
            .truncate(count - 1);

        assert!(
            rebuild_postflop(&snapshot).is_none(),
            "少一條規則的規則集會安靜地打出不一樣的牌，ruleCount 就是為了測出這件事"
        );
    }

    #[test]
    fn 缺少節點集合版本時拒絕還原() {
        let mut snapshot = postflop(
            &crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default())
                .expect("預設覆寫"),
        );
        snapshot
            .as_object_mut()
            .expect("快照")
            .remove("nodeSetVersion");
        assert!(rebuild_postflop(&snapshot).is_none());
    }

    #[test]
    fn 條件只寫出有指定的欄位() {
        let rules = crate::postflop::to_rule_set(&crate::postflop::PostflopOverridesView::default())
            .expect("預設覆寫");
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
