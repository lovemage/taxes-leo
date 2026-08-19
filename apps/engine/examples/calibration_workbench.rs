//! 產生**可互動**的顧問校準工作台（單一 HTML）。
//!
//! 顧問在本機用瀏覽器開啟即可拉動參數滑桿、即時看 13×13 矩陣變化，
//! 調整完按「匯出參數」取得 JSON 寄回。**全程不需要伺服器。**
//!
//! # JS 只重寫最小的計算面
//!
//! 牌類分類、可玩性類別與 equity 排序都由 Rust 算好後**內嵌成靜態資料**，
//! JS 只重算「百分位比門檻加混合帶」這一小段。這讓 Rust 與 JS 的重複實作
//! 面積降到最低。
//!
//! # 漂移自我校驗
//!
//! 即使只有一小段重複實作，仍可能與 Rust 漂移。因此匯出時由 Rust 算好一組
//! 抽樣格的答案內嵌進頁面，JS 載入時用自己的實作重算並比對；不一致就在頁面
//! 頂端顯示紅色警告。**漂移會被當場抓到，而不是靜默誤導顧問。**
//!
//! 執行：cargo run --release --example calibration_workbench
//! 產出：target/calibration-workbench.html

use std::fmt::Write as _;
use std::fs;

use poker_engine::position::PositionLabel;
use poker_engine::strategy::baseline::{distribution_for, expected_opponents, BaselineRules};
use poker_engine::strategy::calibration::RangeMatrix;
use poker_engine::strategy::decision::StackBucket;
use poker_engine::strategy::hand_class::HandClass;
use poker_engine::strategy::playability::PlayabilityCategory;
use poker_engine::strategy::preflop::{positions_for, PreflopNode, PreflopScenario};
use poker_engine::strategy::ranking::{EquityRanking, CONTENT_GRADE_SAMPLES};

/// 工作台涵蓋的節點。全部 4,302 個沒有人調得完，挑最具代表性的。
///
/// 開牌範圍**由 `positions_for` 產生而非手寫清單**。初版手寫時漏掉了
/// UTG+1 與 UTG+2（由牌手顧問於 2026-08-19 指出），改為依桌型展開後，
/// 結構上不可能再漏位置。
fn workbench_nodes() -> Vec<(String, PreflopNode)> {
    let deep = StackBucket::VeryDeep;
    let mut out: Vec<(String, PreflopNode)> = Vec::new();

    // 9-max 的全部 9 個位置，順序即由早到晚
    for hero in positions_for(9) {
        let label = if matches!(hero, PositionLabel::Bb) {
            format!("{} 主動", hero.as_str())
        } else {
            format!("{} 開牌", hero.as_str())
        };
        out.push((
            label,
            PreflopNode {
                seated: 9,
                hero,
                bucket: deep,
                scenario: PreflopScenario::Unopened,
            },
        ));
    }

    out.push((
        "BTN 面對 CO 開牌".to_owned(),
        PreflopNode {
            seated: 9,
            hero: PositionLabel::Btn,
            bucket: deep,
            scenario: PreflopScenario::VsOpen {
                opener: PositionLabel::Co,
            },
        },
    ));
    out.push((
        "BB 面對 BTN 開牌".to_owned(),
        PreflopNode {
            seated: 9,
            hero: PositionLabel::Bb,
            bucket: deep,
            scenario: PreflopScenario::VsOpen {
                opener: PositionLabel::Btn,
            },
        },
    ));
    out.push((
        "BTN 開牌（短碼 15-25BB）".to_owned(),
        PreflopNode {
            seated: 9,
            hero: PositionLabel::Btn,
            bucket: StackBucket::Short,
            scenario: PreflopScenario::Unopened,
        },
    ));
    out.push((
        "BTN 開牌（6-max）".to_owned(),
        PreflopNode {
            seated: 6,
            hero: PositionLabel::Btn,
            bucket: deep,
            scenario: PreflopScenario::Unopened,
        },
    ));

    out
}

fn scenario_key(scenario: PreflopScenario) -> &'static str {
    match scenario {
        PreflopScenario::Unopened => "unopened",
        PreflopScenario::VsLimp { .. } => "vsLimp",
        PreflopScenario::VsOpen { .. } => "vsOpen",
        PreflopScenario::VsThreeBet { .. } => "vsThreeBet",
        PreflopScenario::VsFourBet { .. } => "vsFourBet",
        PreflopScenario::VsSqueeze { .. } => "vsSqueeze",
    }
}

fn category_key(category: PlayabilityCategory) -> &'static str {
    match category {
        PlayabilityCategory::PocketPair => "pocketPair",
        PlayabilityCategory::SuitedAce => "suitedAce",
        PlayabilityCategory::SuitedConnector => "suitedConnector",
        PlayabilityCategory::SuitedOneGap => "suitedOneGap",
        PlayabilityCategory::SuitedTwoGap => "suitedTwoGap",
        PlayabilityCategory::SuitedWideGap => "suitedWideGap",
        PlayabilityCategory::OffsuitBroadway => "offsuitBroadway",
        PlayabilityCategory::OffsuitOther => "offsuitOther",
    }
}

fn main() {
    let rules = BaselineRules::engineering_placeholder();
    let nodes = workbench_nodes();

    // 需要哪些對手數的排序表
    let mut opponent_counts: Vec<usize> = nodes
        .iter()
        .map(|(_, node)| expected_opponents(node))
        .collect();
    opponent_counts.sort_unstable();
    opponent_counts.dedup();

    println!("計算 equity 排序表（{} 種對手數）…", opponent_counts.len());
    let rankings: Vec<(usize, EquityRanking)> = opponent_counts
        .iter()
        .map(|&n| (n, EquityRanking::compute(n, CONTENT_GRADE_SAMPLES)))
        .collect();
    for (n, ranking) in &rankings {
        assert!(
            ranking.is_content_grade(),
            "{n} 人排序表未達內容等級樣本數"
        );
    }

    // ── 牌類靜態資料 ────────────────────────────────────────────
    let all_classes = HandClass::all();
    let mut classes_json = String::from("[");
    for (i, class) in all_classes.iter().enumerate() {
        let (row, col) = class.grid();
        if i > 0 {
            classes_json.push(',');
        }
        let _ = write!(
            classes_json,
            r#"{{"l":"{}","r":{row},"c":{col},"k":"{}"}}"#,
            class.label(),
            category_key(PlayabilityCategory::of(*class))
        );
    }
    classes_json.push(']');

    // ── 各對手數的百分位 ────────────────────────────────────────
    let mut percentiles_json = String::from("{");
    for (i, (opponents, ranking)) in rankings.iter().enumerate() {
        if i > 0 {
            percentiles_json.push(',');
        }
        let _ = write!(percentiles_json, r#""{opponents}":["#);
        for (j, class) in all_classes.iter().enumerate() {
            if j > 0 {
                percentiles_json.push(',');
            }
            let _ = write!(percentiles_json, "{}", ranking.percentile_myriad(*class));
        }
        percentiles_json.push(']');
    }
    percentiles_json.push('}');

    // ── 節點資料 ────────────────────────────────────────────────
    let mut nodes_json = String::from("[");
    for (i, (title, node)) in nodes.iter().enumerate() {
        if i > 0 {
            nodes_json.push(',');
        }
        let order = positions_for(node.seated);
        let hero_index = order.iter().position(|&p| p == node.hero).unwrap_or(0);
        let non_blind = order.len().saturating_sub(2).max(1);
        let is_blind = matches!(node.hero, PositionLabel::Sb | PositionLabel::Bb);
        let _ = write!(
            nodes_json,
            r#"{{"t":"{title}","key":"{}","seated":{},"hero":"{}","heroIndex":{hero_index},"lastIndex":{},"blind":"{}","scenario":"{}","bucketIndex":{},"bucket":"{}","opponents":{},"pushFold":{},"openingKey":"{}.{}"}}"#,
            node.key(),
            node.seated,
            node.hero.as_str(),
            non_blind.saturating_sub(1).max(1),
            if is_blind { node.hero.as_str() } else { "" },
            scenario_key(node.scenario),
            BaselineRules::bucket_index_of(node.bucket),
            node.bucket.as_str(),
            expected_opponents(node),
            rules.is_push_fold(node.bucket),
            node.seated,
            node.hero.as_str()
        );
    }
    nodes_json.push(']');

    // ── 目前參數 ────────────────────────────────────────────────
    let mut scenarios_json = String::from("{");
    for (i, (key, scenario)) in [
        ("unopened", PreflopScenario::Unopened),
        (
            "vsOpen",
            PreflopScenario::VsOpen {
                opener: PositionLabel::Co,
            },
        ),
    ]
    .iter()
    .enumerate()
    {
        if i > 0 {
            scenarios_json.push(',');
        }
        let w = rules.widths_of(*scenario);
        let _ = write!(
            scenarios_json,
            r#""{key}":{{"earliest":{},"latest":{},"callExtra":{},"mixBand":{}}}"#,
            w.aggressive_earliest, w.aggressive_latest, w.call_extra, w.mix_band
        );
    }
    scenarios_json.push('}');

    let p = &rules.playability;
    let multipliers: Vec<String> = rules
        .bucket_multipliers()
        .iter()
        .map(ToString::to_string)
        .collect();

    // 逐（桌型 × 位置）開牌寬度。工作台的每個滑桿對應一張範圍表
    let mut opening_json = String::from("{");
    for (index, ((seated, position), value)) in rules.opening.entries().iter().enumerate() {
        if index > 0 {
            opening_json.push(',');
        }
        let _ = write!(
            opening_json,
            r#""{seated}.{}":{value}"#,
            position.as_str()
        );
    }
    opening_json.push('}');

    let rules_json = format!(
        r#"{{"version":"{}","scenarios":{scenarios_json},"opening":{opening_json},"bucketMultiplier":[{}],"playability":{{"pocketPair":{},"suitedAce":{},"suitedConnector":{},"suitedOneGap":{},"suitedTwoGap":{},"suitedWideGap":{},"offsuitBroadway":{},"offsuitOther":{}}}}}"#,
        rules.version,
        multipliers.join(","),
        p.pocket_pair,
        p.suited_ace,
        p.suited_connector,
        p.suited_one_gap,
        p.suited_two_gap,
        p.suited_wide_gap,
        p.offsuit_broadway,
        p.offsuit_other,
    );

    // ── 漂移校驗樣本 ────────────────────────────────────────────
    // 由 Rust 算好，JS 載入時用自己的實作重算並比對
    let mut verification = String::from("[");
    let mut checks = 0;
    for (node_index, (_, node)) in nodes.iter().enumerate() {
        let opponents = expected_opponents(node);
        let ranking = &rankings
            .iter()
            .find(|(n, _)| *n == opponents)
            .expect("排序表存在")
            .1;
        let matrix = RangeMatrix::build(*node, &rules, ranking);
        // 全覆蓋校驗：每個節點的 169 格都比對。抽樣校驗會漏掉差異，
        // 而漂移一旦漏掉就會靜默誤導顧問，因此寧可讓檔案大一點
        for class in all_classes.iter() {
            let cell = matrix.cell(*class);
            if checks > 0 {
                verification.push(',');
            }
            let _ = write!(
                verification,
                r#"{{"n":{node_index},"i":{},"a":{},"c":{},"f":{}}}"#,
                class.index(),
                cell.aggressive,
                cell.call,
                cell.fold
            );
            checks += 1;
        }
    }
    verification.push(']');

    let data = format!(
        r#"{{"classes":{classes_json},"percentiles":{percentiles_json},"nodes":{nodes_json},"rules":{rules_json},"verification":{verification}}}"#
    );

    let html = build_html(&data, &rules);
    fs::create_dir_all("target").expect("建立輸出目錄");
    let path = "target/calibration-workbench.html";
    fs::write(path, html).expect("寫入");

    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    println!("已產生 {path}（{:.0} KB，{checks} 個校驗樣本）", size / 1024);

    // 驗證 distribution_for 仍可用（避免匯出與實際產生脫節）
    let sample = distribution_for(
        &nodes[4].1,
        all_classes[0],
        &rules,
        &rankings.iter().find(|(n, _)| *n == 2).expect("排序表").1,
    );
    assert!(sample.is_ok(), "產生器本身必須可用");
}

fn build_html(data: &str, rules: &BaselineRules) -> String {
    let script = include_str!("workbench.js");
    let approved = if rules.consultant_approved {
        "已簽核"
    } else {
        "尚未簽核"
    };
    format!(
        r#"<!doctype html>
<html lang="zh-Hant"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Preflop baseline 校準工作台</title>
<style>
:root{{color-scheme:dark}}
*{{box-sizing:border-box}}
body{{margin:0;padding:24px;background:#0b0c0e;color:#e8eaed;
  font:13px/1.6 "Noto Sans TC",system-ui,sans-serif}}
.wrap{{max-width:1240px;margin:0 auto}}
h1{{font-size:20px;margin:0 0 4px}}
h2{{font-size:15px;margin:0 0 8px}}
h3{{font-size:13px;margin:16px 0 6px;color:#9aa0a6;font-weight:600}}
.meta{{color:#9aa0a6;font-size:12px;margin:0 0 4px}}
code{{font-family:ui-monospace,Consolas,monospace;color:#9aa0a6}}
.warn{{border:1px solid #f59e0b;color:#f59e0b;padding:12px 16px;border-radius:8px;margin:16px 0}}
.err{{border:1px solid #ef4444;color:#ef4444;padding:12px 16px;border-radius:8px;margin:16px 0;display:none}}
.ok{{border:1px solid #22c55e;color:#22c55e;padding:8px 14px;border-radius:8px;margin:16px 0;font-size:12px}}
.layout{{display:grid;grid-template-columns:320px 1fr;gap:20px;align-items:start}}
.panel{{padding:16px;border:1px solid #2a2e34;border-radius:8px;background:#16181c;
  position:sticky;top:24px;max-height:calc(100vh - 48px);overflow-y:auto}}
.node{{margin:0 0 24px;padding:16px;border:1px solid #2a2e34;border-radius:8px;background:#16181c}}
table.matrix{{border-collapse:collapse;font-family:ui-monospace,Consolas,monospace;font-size:11px}}
table.matrix td{{width:50px;height:38px;text-align:center;vertical-align:middle;
  border:1px solid #0b0c0e;border-radius:0;cursor:default}}
.pct{{font-size:9px;opacity:.85}}
.slider{{margin:0 0 12px}}
.slider label{{display:flex;justify-content:space-between;font-size:12px;margin-bottom:2px}}
.slider .val{{font-family:ui-monospace,Consolas,monospace;color:#22c55e}}
.slider input{{width:100%;accent-color:#22c55e}}
.legend{{display:flex;gap:14px;flex-wrap:wrap;margin:10px 0 18px;font-size:12px;color:#9aa0a6}}
.legend span{{display:inline-flex;align-items:center;gap:5px}}
.swatch{{width:13px;height:13px;display:inline-block;border:1px solid #2a2e34}}
button{{font:inherit;padding:8px 14px;border-radius:6px;border:1px solid #22c55e;
  background:#166534;color:#e8eaed;cursor:pointer}}
button:hover{{background:#22c55e;color:#0b0c0e}}
button.ghost{{background:transparent;border-color:#2a2e34;color:#9aa0a6}}
button.ghost:hover{{background:#22262c;color:#e8eaed}}
textarea{{width:100%;height:150px;margin-top:10px;background:#0b0c0e;color:#e8eaed;
  border:1px solid #2a2e34;border-radius:6px;padding:8px;
  font-family:ui-monospace,Consolas,monospace;font-size:11px}}
.width-badge{{font-family:ui-monospace,Consolas,monospace;color:#22c55e}}
</style></head><body><div class="wrap">

<h1>Preflop baseline 校準工作台</h1>
<p class="meta">規則版本 <code>{}</code>ー顧問簽核狀態：<strong>{approved}</strong></p>

<div id="drift" class="err"></div>

<div class="warn">
<strong>這份內容尚未經牌手顧問簽核，不得作為出貨 baseline。</strong><br>
範圍由 equity 排序加參數化規則產生，<strong>不是 GTO 解</strong>。
左側滑桿即時重算右側全部矩陣。調整完請按<strong>「匯出參數」</strong>取得 JSON 檔並回傳；
正式的 727,038 格全表仍由我方以引擎展開，本頁只負責預覽。
</div>

<div class="layout">
<aside class="panel">
  <h2>參數</h2>
  <div id="controls"></div>
  <div style="display:flex;gap:8px;margin-top:16px;flex-wrap:wrap">
    <button id="export">匯出參數</button>
    <button id="reset" class="ghost">重設</button>
  </div>
  <p class="meta" style="margin-top:10px">若瀏覽器擋下下載，請複製下方文字回傳。</p>
  <textarea id="output" readonly></textarea>
</aside>

<main>
  <div class="legend">
    <span><i class="swatch" style="background:#166534"></i>100% 主動</span>
    <span><i class="swatch" style="background:rgba(34,197,94,.45)"></i>混合</span>
    <span><i class="swatch" style="background:#1e3a5f"></i>跟注</span>
    <span><i class="swatch" style="background:#16181c"></i>棄牌</span>
  </div>
  <div id="matrices"></div>
</main>
</div>

<script id="data" type="application/json">{data}</script>
<script>{script}</script>
</div></body></html>"#,
        rules.version
    )
}
