//! 產生給牌手顧問的校準報告（自帶樣式的單一 HTML）。
//!
//! 顧問不需要安裝 Rust 或 Node，用瀏覽器開啟即可檢視全部範圍矩陣、
//! 標出不同意的格，並看到目前的參數值。
//!
//! 執行：cargo run --release --example calibration_report
//! 產出：target/calibration-report.html

use std::fmt::Write as _;
use std::fs;

use poker_engine::position::PositionLabel;
use poker_engine::strategy::baseline::{expected_opponents, BaselineRules};
use poker_engine::strategy::calibration::{MatrixCell, RangeMatrix};
use poker_engine::strategy::decision::StackBucket;
use poker_engine::strategy::distribution::FULL;
use poker_engine::strategy::preflop::{positions_for, PreflopNode, PreflopScenario};
use poker_engine::strategy::playability::PlayabilityCategory;
use poker_engine::strategy::ranking::EquityRanking;

/// 報告涵蓋的節點。全部 4,302 個沒有人調得完，挑最具代表性的。
///
/// 開牌範圍**由 `positions_for` 產生而非手寫清單**。初版手寫時漏掉了
/// UTG+1 與 UTG+2（由牌手顧問於 2026-08-19 指出），改為依桌型展開後，
/// 結構上不可能再漏位置。
fn report_nodes() -> Vec<(String, PreflopNode)> {
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

fn cell_style(cell: &MatrixCell) -> String {
    // 主動比例決定綠色濃度；純跟注為藍；純棄牌為深灰。
    // 文字一律標出數值，顏色只是加速辨識（UI 規格 V.7 的色盲可用性原則）
    if cell.aggressive == FULL {
        "background:#166534;color:#e8eaed".to_owned()
    } else if cell.aggressive > 0 {
        let ratio = cell.aggressive * 100 / FULL;
        format!("background:rgba(34,197,94,{:.2});color:#0b0c0e", f64::from(ratio) / 130.0)
    } else if cell.call > 0 {
        "background:#1e3a5f;color:#e8eaed".to_owned()
    } else {
        "background:#16181c;color:#4b5563".to_owned()
    }
}

fn matrix_html(title: &str, matrix: &RangeMatrix) -> String {
    let mut out = String::new();
    let width = f64::from(matrix.width_myriad()) / 100.0;
    let mixed = matrix.mixed_cells().len();

    let _ = write!(
        out,
        r#"<section class="node">
<h2>{title}</h2>
<p class="meta">節點鍵 <code>{}</code> ・ 範圍寬度 <strong>{width:.1}%</strong> ・ 混合格 {mixed} 個</p>
<table class="matrix">"#,
        matrix.node.key()
    );

    for row in matrix.grid() {
        out.push_str("<tr>");
        for cell in row {
            let aggressive = f64::from(cell.aggressive) / 100.0;
            let label = cell.class.label();
            let text = if cell.aggressive == FULL {
                label.clone()
            } else if cell.aggressive > 0 {
                format!("{label}<br><span class=\"pct\">{aggressive:.0}%</span>")
            } else if cell.call > 0 {
                format!("{label}<br><span class=\"pct\">跟</span>")
            } else {
                label.clone()
            };
            let _ = write!(
                out,
                r#"<td style="{}" title="{label}｜{}｜主動 {aggressive:.0}%｜可玩性調整後 equity 前 {:.1}%">{text}</td>"#,
                cell_style(&cell),
                PlayabilityCategory::of(cell.class).as_str(),
                f64::from(cell.percentile) / 100.0
            );
        }
        out.push_str("</tr>");
    }
    out.push_str("</table></section>");
    out
}

fn main() {
    let rules = BaselineRules::engineering_placeholder();
    // 依 expected_opponents 的說明：開牌與面對加注都以少人數排序，
    // 用 8 人排序會產生「開 K9s 卻棄 88」這種明顯錯誤
    let ranking_two = EquityRanking::compute(2, 20_000);
    let ranking_one = EquityRanking::compute(1, 20_000);

    let mut body = String::new();
    for (title, node) in report_nodes() {
        // 開牌情境面對全桌，面對加注多為少人底池
        let ranking = if expected_opponents(&node) >= 2 {
            &ranking_two
        } else {
            &ranking_one
        };
        let matrix = RangeMatrix::build(node, &rules, ranking);
        body.push_str(&matrix_html(&title, &matrix));
    }

    let widths = rules.widths_of(PreflopScenario::Unopened);

    let examples: &[(PlayabilityCategory, &str)] = &[
        (PlayabilityCategory::PocketPair, "AA、77、22"),
        (PlayabilityCategory::SuitedAce, "AKs、A5s、A2s"),
        (PlayabilityCategory::SuitedConnector, "87s、65s、KQs"),
        (PlayabilityCategory::SuitedOneGap, "97s、J9s"),
        (PlayabilityCategory::SuitedTwoGap, "T7s、96s"),
        (PlayabilityCategory::SuitedWideGap, "K2s、Q4s"),
        (PlayabilityCategory::OffsuitBroadway, "KJo、QTo"),
        (PlayabilityCategory::OffsuitOther, "K9o、72o"),
    ];
    let mut playability_rows = String::new();
    for (category, example) in examples {
        let shift = rules.playability.of(*category);
        let sign = if shift > 0 { "+" } else { "" };
        let _ = write!(
            playability_rows,
            "<tr><td>{}</td><td class=\"num\">{sign}{:.1}%</td><td>{example}</td></tr>",
            category.as_str(),
            f64::from(shift) / 100.0
        );
    }

    let html = format!(
        r#"<!doctype html>
<html lang="zh-Hant"><head><meta charset="utf-8">
<title>Preflop baseline 校準報告</title>
<style>
:root{{color-scheme:dark}}
body{{margin:0;padding:32px;background:#0b0c0e;color:#e8eaed;
  font:13px/1.6 "Noto Sans TC",system-ui,sans-serif;max-width:1100px;margin-inline:auto}}
h1{{font-size:20px;margin:0 0 8px}}
h2{{font-size:15px;margin:0 0 4px}}
.warn{{border:1px solid #f59e0b;color:#f59e0b;padding:12px 16px;border-radius:8px;margin:16px 0 28px}}
.meta{{color:#9aa0a6;margin:0 0 10px;font-size:12px}}
code{{font-family:ui-monospace,Consolas,monospace;color:#9aa0a6}}
.node{{margin:0 0 32px;padding:16px;border:1px solid #2a2e34;border-radius:8px;background:#16181c}}
table.matrix{{border-collapse:collapse;font-family:ui-monospace,Consolas,monospace;font-size:11px}}
table.matrix td{{width:52px;height:40px;text-align:center;vertical-align:middle;
  border:1px solid #0b0c0e;border-radius:0}}
.pct{{font-size:9px;opacity:.85}}
.legend{{display:flex;gap:16px;flex-wrap:wrap;margin:12px 0 24px;font-size:12px;color:#9aa0a6}}
.legend span{{display:inline-flex;align-items:center;gap:6px}}
.swatch{{width:14px;height:14px;display:inline-block;border:1px solid #2a2e34}}
.params{{margin:28px 0;padding:16px;border:1px solid #2a2e34;border-radius:8px;background:#16181c}}
.params table{{border-collapse:collapse;font-size:12px}}
.params td,.params th{{padding:4px 12px;text-align:left;border-bottom:1px solid #2a2e34}}
.params td.num{{text-align:right;font-family:ui-monospace,Consolas,monospace}}
</style></head><body>
<h1>Preflop baseline 校準報告</h1>
<p class="meta">規則集 <code>{}</code>ー版本 <code>{}</code></p>

<div class="warn">
<strong>這份內容尚未經牌手顧問簽核，不得作為出貨 baseline。</strong><br>
範圍由 equity 排序加參數化規則產生，<strong>不是 GTO 解</strong>。已知的簡化：以 raw equity
代替可玩性，因此低估同花連牌的翻後價值、A5s 的阻斷與堅果同花價值、以及 AKo 的支配優勢；
每個情境目前只有單一加注尺度；盲注位僅用單一寬度，未表達 SB 的 limp／raise 混合與 BB 的防守結構。
</div>

<div class="legend">
<span><i class="swatch" style="background:#166534"></i>100% 主動</span>
<span><i class="swatch" style="background:rgba(34,197,94,.45)"></i>混合（格內標示比例）</span>
<span><i class="swatch" style="background:#1e3a5f"></i>跟注</span>
<span><i class="swatch" style="background:#16181c"></i>棄牌</span>
</div>

<p class="meta">滑鼠移到格子上可看該手牌的 equity 百分位。<strong>請直接在此標出不同意的格</strong>
（例如「BTN 的 55 應為 100% 開牌」），我方會用歸因工具算出該調哪個參數、以及調整後會連帶
影響哪些手牌，再回報給您確認。</p>

{body}

<div class="params">
<h2>目前的規則參數（顧問實際要調的就是這些數字）</h2>
<table>
<tr><th>參數</th><th>值</th><th>說明</th></tr>
<tr><td><code>unopened.aggressive_earliest</code></td><td class="num">{:.1}%</td><td>最早位置（UTG）的開牌寬度</td></tr>
<tr><td><code>unopened.aggressive_latest</code></td><td class="num">{:.1}%</td><td>最晚非盲注位置（BTN）的開牌寬度</td></tr>
<tr><td><code>unopened.mix_band</code></td><td class="num">{:.1}%</td><td>邊界混合帶寬度</td></tr>
<tr><td><code>sb_aggressive</code></td><td class="num">{:.1}%</td><td>SB 專用寬度（不套用「越晚越寬」）</td></tr>
<tr><td><code>bb_aggressive</code></td><td class="num">{:.1}%</td><td>BB 專用寬度</td></tr>
<tr><td><code>open_size_centi_bb</code></td><td class="num">{:.2} BB</td><td>開牌尺度</td></tr>
<tr><td><code>three_bet_size_centi_bb</code></td><td class="num">{:.2} BB</td><td>3-bet 尺度</td></tr>
</table>
<p class="meta">另有各情境的寬度、9 檔 bucket 乘數與尺度參數，合計數十個數字。
調整任何一個即可全表重算（727,038 格，耗時約 0.02 秒）。</p>
</div>

<div class="params">
<h2>可玩性調整（八個類別偏移）</h2>
<p class="meta">Equity 排序衡量的是「攤牌時誰的牌大」，但翻前價值有一大部分來自翻後可玩性。
這八個偏移讓「同花連牌比弱同花高張更值得開」得以表達。<strong>正值代表往更強的方向移動。</strong>
這些是最需要您判斷的數字——方向依撲克共識設定，幅度刻意保守。</p>
<table>
<tr><th>類別</th><th>偏移</th><th>代表牌例</th></tr>
{playability_rows}
</table>
<p class="meta">單一類別的偏移上限為 ±15.0%。若某類別需要超過上限的調整，
代表該類別應再細分，而不是把旋鈕轉到底。</p>
</div>
</body></html>"#,
        rules.name,
        rules.version,
        f64::from(widths.aggressive_earliest) / 100.0,
        f64::from(widths.aggressive_latest) / 100.0,
        f64::from(widths.mix_band) / 100.0,
        f64::from(rules.sb_aggressive) / 100.0,
        f64::from(rules.bb_aggressive) / 100.0,
        f64::from(rules.open_size_centi_bb) / 100.0,
        f64::from(rules.three_bet_size_centi_bb) / 100.0,
    );

    fs::create_dir_all("target").expect("建立輸出目錄");
    let path = "target/calibration-report.html";
    fs::write(path, html).expect("寫入報告");
    println!("已產生 {path}");
}
