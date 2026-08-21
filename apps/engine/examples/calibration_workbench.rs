//! 產生**可互動**的顧問校準工作台（單一 HTML）。
//!
//! 顧問在本機用瀏覽器開啟即可挑節點、點牌格提意見、看歸因與差異，
//! 調整完按「匯出」取得帶簽核資訊的 JSON 寄回。**全程不需要伺服器。**
//!
//! # 兩層介面
//!
//! - **顧問校正**：選節點、直接點牌格說「這手該／不該打」，工具反解要調
//!   哪個參數、連帶影響哪些牌；顧問不接受連帶影響時才落到逐格覆寫。
//! - **進階參數**：原本的滑桿組，供工程端調整生成規則。
//!
//! # JS 只重寫最小的計算面
//!
//! 牌類分類、可玩性類別與 equity 排序都由 Rust 算好後**內嵌成靜態資料**，
//! JS 只重算「百分位比門檻加混合帶」與建立在其上的歸因搜尋。
//!
//! # 三層漂移自我校驗
//!
//! 重複實作就會漂移，因此三個層次都由 Rust 算好答案內嵌，JS 載入時重算比對：
//!
//! 1. **節點枚舉**：JS 自己列舉的節點總數與抽樣鍵必須與 Rust 一致；
//! 2. **逐格數值**：抽樣節點的全部 169 格必須逐位元一致；
//! 3. **歸因結果**：同一則意見反解出的參數值與連帶格數必須一致。
//!
//! 任一層不一致就在頁面頂端顯示紅色警告並**停用校正操作**。
//! 漂移會被當場抓到，而不是靜默誤導顧問。
//!
//! 執行：cargo run --release --example calibration_workbench
//! 產出：target/calibration-workbench.html

use std::fmt::Write as _;
use std::fs;

use poker_engine::position::PositionLabel;
use poker_engine::strategy::baseline::{distribution_for, expected_opponents, BaselineRules};
use poker_engine::strategy::calibration::{attribute, RangeMatrix, Verdict};
use poker_engine::strategy::decision::StackBucket;
use poker_engine::strategy::hand_class::HandClass;
use poker_engine::strategy::playability::PlayabilityCategory;
use poker_engine::strategy::preflop::{
    all_buckets, enumerate_nodes, positions_for, PreflopNode, PreflopScenario,
};
use poker_engine::strategy::ranking::{EquityRanking, CONTENT_GRADE_SAMPLES};

/// 顧問校正頁預設打開的節點，以及進階參數頁的總覽表。
///
/// 這**不再是校正的全部範圍**——選擇器可抽查全部節點。
/// 這裡只是「一打開就看得到東西」的預設值。
fn default_nodes() -> Vec<(String, PreflopNode)> {
    let deep = StackBucket::VeryDeep;
    let mut out: Vec<(String, PreflopNode)> = Vec::new();

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

/// 逐格數值校驗要覆蓋的節點。
///
/// 全部節點各 169 格會讓檔案膨脹到數 MB，因此取**質數步長**的抽樣。
/// 步長 89 與 bucket 數 9 互質，抽樣才不會系統性地落在同一個 bucket
/// ——步長若取 90 就會每次都抽到同一檔籌碼深度，等於沒驗到。
fn verification_nodes() -> Vec<PreflopNode> {
    const STRIDE: usize = 89;
    let all = enumerate_nodes();
    let mut out: Vec<PreflopNode> = all.iter().step_by(STRIDE).copied().collect();

    // 預設節點一定要驗到——顧問一打開就看到的就是這幾張
    for (_, node) in default_nodes() {
        if !out.contains(&node) {
            out.push(node);
        }
    }
    out
}

/// 這一版 baseline 的桌況前提，隨匯出的 JSON 一起走。
///
/// [`BaselineRules`] 沒有任何抽水／ante／straddle 參數，所以產生的範圍
/// 等同「不抽水、無 ante、無 straddle」。核心規格 4.1 把三者列為節點要素，
/// 目前保存在策略表 meta 而不進節點鍵（見 `preflop` 模組的說明）。
///
/// **抽水會實質改變範圍**：抽水吃掉底池一部分，邊緣手牌由打平變成虧損，
/// 範圍整體收緊，跟注比加注更吃虧。因此顧問是在什麼桌況下簽核的必須寫進
/// 檔案，日後才追溯得到。平台改成抽水時，這批範圍要整批重新校準。
///
/// 與下方警告區的文字對應，兩處要一起改。
const TABLE_ASSUMPTIONS: &str =
    r#"{"rake":"none","rakePct":0,"rakeCap":0,"ante":"none","straddle":"none"}"#;

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
    let all_nodes = enumerate_nodes();
    let verify_nodes = verification_nodes();

    // 需要哪些對手數的排序表。以**全部節點**推算，不是只看預設節點——
    // 選擇器能抽查任何節點，任何一種對手數的排序表都必須內嵌
    let mut opponent_counts: Vec<usize> = all_nodes.iter().map(expected_opponents).collect();
    opponent_counts.sort_unstable();
    opponent_counts.dedup();

    println!("計算 equity 排序表（{} 種對手數）…", opponent_counts.len());
    let rankings: Vec<(usize, EquityRanking)> = opponent_counts
        .iter()
        .map(|&n| (n, EquityRanking::compute(n, CONTENT_GRADE_SAMPLES)))
        .collect();
    for (n, ranking) in &rankings {
        assert!(ranking.is_content_grade(), "{n} 人排序表未達內容等級樣本數");
    }
    let ranking_for = |opponents: usize| -> &EquityRanking {
        &rankings
            .iter()
            .find(|(n, _)| *n == opponents)
            .expect("排序表存在")
            .1
    };

    // ── 牌類靜態資料 ────────────────────────────────────────────
    let all_classes = HandClass::all();
    let mut classes_json = String::from("[");
    for (i, class) in all_classes.iter().enumerate() {
        let (row, col) = class.grid();
        if i > 0 {
            classes_json.push(',');
        }
        // n 為該類別的 combo 數（對子 6、同花 4、非同花 12）。
        // 範圍寬度必須以此加權，否則與牌手慣用的 1,326 combo 百分比對不上
        let _ = write!(
            classes_json,
            r#"{{"l":"{}","r":{row},"c":{col},"k":"{}","n":{}}}"#,
            class.label(),
            category_key(PlayabilityCategory::of(*class)),
            class.combos()
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

    // ── 桌型的位置序列（JS 據此自行列舉節點）────────────────────
    let mut positions_json = String::from("{");
    for (i, seated) in (6u8..=9).enumerate() {
        if i > 0 {
            positions_json.push(',');
        }
        let labels: Vec<String> = positions_for(seated)
            .iter()
            .map(|p| format!(r#""{}""#, p.as_str()))
            .collect();
        let _ = write!(positions_json, r#""{seated}":[{}]"#, labels.join(","));
    }
    positions_json.push('}');

    // ── 籌碼分檔 ────────────────────────────────────────────────
    let buckets_json: Vec<String> = all_buckets()
        .iter()
        .map(|b| {
            format!(
                r#"{{"k":"{}","push":{}}}"#,
                b.as_str(),
                rules.is_push_fold(*b)
            )
        })
        .collect();

    // ── 目前參數 ────────────────────────────────────────────────
    // 六個情境全帶。選擇器可抽查任何情境，少帶一個就會算不出來
    let scenario_samples = [
        ("unopened", PreflopScenario::Unopened),
        ("vsLimp", PreflopScenario::VsLimp { limpers: 1 }),
        (
            "vsOpen",
            PreflopScenario::VsOpen {
                opener: PositionLabel::Co,
            },
        ),
        (
            "vsThreeBet",
            PreflopScenario::VsThreeBet {
                by: PositionLabel::Btn,
            },
        ),
        (
            "vsFourBet",
            PreflopScenario::VsFourBet {
                by: PositionLabel::Co,
            },
        ),
        (
            "vsSqueeze",
            PreflopScenario::VsSqueeze {
                by: PositionLabel::Btn,
            },
        ),
    ];
    let mut scenarios_json = String::from("{");
    for (i, (key, scenario)) in scenario_samples.iter().enumerate() {
        if i > 0 {
            scenarios_json.push(',');
        }
        let w = rules.widths_of(*scenario);
        // unopened 與 vsOpen 的 earliest／latest 已被逐節點查表取代，
        // 但其餘四個情境仍在讀，因此照樣帶出
        let _ = write!(
            scenarios_json,
            r#""{key}":{{"earliest":{},"latest":{},"callExtra":{},"mixBand":{}}}"#,
            w.aggressive_earliest, w.aggressive_latest, w.call_extra, w.mix_band
        );
    }
    scenarios_json.push('}');

    let mut opening_json = String::from("{");
    for (index, ((seated, position), value)) in rules.opening.entries().iter().enumerate() {
        if index > 0 {
            opening_json.push(',');
        }
        let _ = write!(opening_json, r#""{seated}.{}":{value}"#, position.as_str());
    }
    opening_json.push('}');

    let mut vs_open_json = String::from("{");
    for (index, ((seated, hero, opener), value)) in rules.vs_open_width.entries().iter().enumerate()
    {
        if index > 0 {
            vs_open_json.push(',');
        }
        let _ = write!(
            vs_open_json,
            r#""{seated}.{}.{}":{value}"#,
            hero.as_str(),
            opener.as_str()
        );
    }
    vs_open_json.push('}');

    let p = &rules.playability;
    let multipliers: Vec<String> = rules
        .bucket_multipliers()
        .iter()
        .map(ToString::to_string)
        .collect();

    let rules_json = format!(
        r#"{{"version":"{}","scenarios":{scenarios_json},"opening":{opening_json},"vsOpen":{vs_open_json},"bucketMultiplier":[{}],"playability":{{"pocketPair":{},"suitedAce":{},"suitedConnector":{},"suitedOneGap":{},"suitedTwoGap":{},"suitedWideGap":{},"offsuitBroadway":{},"offsuitOther":{}}},"overrides":{{}}}}"#,
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

    // ── 預設節點（顧問校正頁的起點與進階頁的總覽）────────────────
    let mut defaults_json = String::from("[");
    for (i, (title, node)) in default_nodes().iter().enumerate() {
        if i > 0 {
            defaults_json.push(',');
        }
        let _ = write!(defaults_json, r#"{{"t":"{title}","key":"{}"}}"#, node.key());
    }
    defaults_json.push(']');

    // ── 校驗一：節點枚舉 ────────────────────────────────────────
    // JS 自己列舉節點，因此連「列舉出來的是不是同一組節點」都要驗。
    // 只驗總數會漏掉「數量對但內容不同」的情形，因此附上抽樣鍵
    let enum_samples: Vec<String> = all_nodes
        .iter()
        .step_by(137)
        .map(|n| format!(r#""{}""#, n.key()))
        .collect();
    let enumeration_json = format!(
        r#"{{"count":{},"stride":137,"keys":[{}]}}"#,
        all_nodes.len(),
        enum_samples.join(",")
    );

    // ── 校驗二：逐格數值 ────────────────────────────────────────
    let mut verification = String::from("[");
    let mut checks = 0;
    for node in &verify_nodes {
        let matrix = RangeMatrix::build(*node, &rules, ranking_for(expected_opponents(node)));
        for class in &all_classes {
            let cell = matrix.cell(*class);
            if checks > 0 {
                verification.push(',');
            }
            let _ = write!(
                verification,
                r#"{{"k":"{}","i":{},"a":{},"c":{},"f":{}}}"#,
                node.key(),
                class.index(),
                cell.aggressive,
                cell.call,
                cell.fold
            );
            checks += 1;
        }
    }
    verification.push(']');

    // ── 校驗三：歸因結果 ────────────────────────────────────────
    // 歸因建立在 cellOf 之上，但二分搜尋與候選參數挑選是另一段重複實作。
    // 顧問看到的「要調到多少、會連帶影響幾格」若漂移，比矩陣漂移更難察覺
    let mut attribution_json = String::from("[");
    let mut attrib_count = 0;
    for node in verify_nodes.iter().step_by(3) {
        let ranking = ranking_for(expected_opponents(node));
        let matrix = RangeMatrix::build(*node, &rules, ranking);
        for class in all_classes.iter().step_by(23) {
            let cell = matrix.cell(*class);
            // 挑一個「尚未滿足」的意見，否則歸因會直接回空集合
            let verdict = if cell.aggressive == 10_000 {
                Verdict::ShouldNotBeAggressive
            } else {
                Verdict::ShouldBeAggressive
            };
            for attribution in attribute(*node, *class, verdict, &rules, ranking) {
                if attrib_count > 0 {
                    attribution_json.push(',');
                }
                let _ = write!(
                    attribution_json,
                    r#"{{"k":"{}","i":{},"v":"{}","p":"{}","r":{},"in":{},"out":{}}}"#,
                    node.key(),
                    class.index(),
                    if verdict == Verdict::ShouldBeAggressive {
                        "yes"
                    } else {
                        "no"
                    },
                    attribution.parameter.as_str(),
                    attribution.required,
                    attribution.pulled_in.len(),
                    attribution.pushed_out.len()
                );
                attrib_count += 1;
            }
        }
    }
    attribution_json.push(']');

    let data = format!(
        r#"{{"classes":{classes_json},"percentiles":{percentiles_json},"positions":{positions_json},"buckets":[{}],"defaults":{defaults_json},"rules":{rules_json},"assumptions":{TABLE_ASSUMPTIONS},"enumeration":{enumeration_json},"verification":{verification},"attribution":{attribution_json}}}"#,
        buckets_json.join(",")
    );

    let html = build_html(&data, &rules);
    fs::create_dir_all("target").expect("建立輸出目錄");
    let path = "target/calibration-workbench.html";
    fs::write(path, html).expect("寫入");

    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    println!(
        "已產生 {path}（{} KB）\n  可校正節點 {}｜逐格校驗 {checks} 樣本（{} 節點）｜歸因校驗 {attrib_count} 樣本",
        size / 1024,
        all_nodes.len(),
        verify_nodes.len()
    );

    // 驗證產生器本身仍可用（避免匯出與實際產生脫節）
    let sample = distribution_for(&default_nodes()[4].1, all_classes[0], &rules, ranking_for(2));
    assert!(sample.is_ok(), "產生器本身必須可用");
}

fn build_html(data: &str, rules: &BaselineRules) -> String {
    let script = include_str!("workbench.js");
    let style = include_str!("workbench.css");
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
<style>{style}</style></head><body><div class="wrap">

<header class="top">
  <div>
    <h1>Preflop baseline 校準工作台</h1>
    <p class="meta">規則版本 <code>{}</code>ー顧問簽核狀態：<strong>{approved}</strong></p>
  </div>
  <nav class="tabs">
    <button data-tab="calibrate" class="on">顧問校正</button>
    <button data-tab="params">進階參數</button>
  </nav>
</header>

<div id="drift" class="err"></div>

<div class="warn">
<strong>這份內容尚未經牌手顧問簽核，不得作為出貨 baseline。</strong>
範圍由 equity 排序加參數化規則產生，<strong>不是 GTO 解</strong>。
範圍寬度以 <strong>1,326 combos</strong> 加權，與牌桌上的講法一致。
正式的 687,492 格全表由我方以引擎展開，本頁負責校正與預覽。
<br>桌況前提：<strong>不抽水、無 ante、無 straddle</strong>，與平台目前的設定一致。
抽水會讓範圍整體收緊，平台日後若開抽水，這批範圍要重新校準。
</div>

<section id="tab-calibrate">
  <div class="picker" id="picker"></div>
  <div class="split">
    <main id="matrixHost"></main>
    <aside>
      <div class="card" id="attribHost"></div>
      <div class="card" id="changesHost"></div>
      <div class="card" id="signoffHost"></div>
    </aside>
  </div>
</section>

<section id="tab-params" hidden>
  <div class="split params">
    <aside>
      <div class="card">
        <h2>生成規則參數</h2>
        <p class="meta">這一層是工程端的旋鈕。顧問的意見請走「顧問校正」頁，
        由工具反解該調哪一個。</p>
        <div id="controls"></div>
        <div class="row"><button id="reset" class="ghost">重設全部參數</button></div>
      </div>
    </aside>
    <main>
      <div class="legend">
        <span><i class="swatch" style="background:#166534"></i>100% 主動（Open／3-bet／Jam，逐表標示）</span>
        <span><i class="swatch" style="background:rgba(34,197,94,.45)"></i>混合</span>
        <span><i class="swatch" style="background:#1e3a5f"></i>Call</span>
        <span><i class="swatch" style="background:#16181c"></i>Fold</span>
      </div>
      <div id="overview"></div>
    </main>
  </div>
</section>

</div>
<script id="data" type="application/json">{data}</script>
<script>{script}</script>
</body></html>"#,
        rules.version
    )
}
