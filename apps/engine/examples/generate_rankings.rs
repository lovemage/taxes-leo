//! 產製內容級 equity 排序資產。
//!
//! ```text
//! cargo run --release -p poker-engine --example generate_rankings
//! ```
//!
//! **一定要加 `--release`**：debug 建置跑同一件事要八十秒。
//!
//! 產出蓋掉 `apps/engine/assets/equity-rankings-v1.txt`，該檔以
//! `include_str!` 編進引擎（見 `strategy::ranking_asset`）。改動
//! `RANKING_SEED`、equity 計算或排序規則之後都必須重跑，否則出貨的
//! 二進位檔帶的是舊內容。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use poker_engine::bot::MAX_EXPECTED_OPPONENTS;
use poker_engine::strategy::ranking::{EquityRanking, CONTENT_GRADE_SAMPLES, RANKING_SEED};
use poker_engine::strategy::ranking_asset::{self, ASSET_PATH};

fn main() {
    let samples = CONTENT_GRADE_SAMPLES;
    println!("產製 equity 排序資產");
    println!("  seed      {RANKING_SEED:#x}");
    println!("  取樣      {samples}");
    println!("  對手數    1–{MAX_EXPECTED_OPPONENTS}");
    if cfg!(debug_assertions) {
        println!("\n  ⚠ 目前是 debug 建置，這會跑上一分多鐘。請改用 --release。\n");
    }

    let start = Instant::now();
    let rankings: BTreeMap<usize, EquityRanking> = (1..=MAX_EXPECTED_OPPONENTS)
        .map(|opponents| {
            let at = Instant::now();
            let ranking = EquityRanking::compute(opponents, samples);
            println!(
                "  {opponents} 名對手完成（{:.1} 秒）",
                at.elapsed().as_secs_f64()
            );
            (opponents, ranking)
        })
        .collect();
    println!("  合計 {:.1} 秒\n", start.elapsed().as_secs_f64());

    let text = ranking_asset::encode(samples, &rankings);

    // 寫檔前先自己讀回來一次。產出一份載不回來的資產，症狀會延到
    // 使用者按下開始才出現，而那時已經看不出是產製這一步壞的
    let parsed = ranking_asset::decode(&text).expect("剛產出的資產必須能解回來");
    assert_eq!(parsed.rankings, rankings, "往返必須是無損的");
    assert!(parsed.is_content_grade(), "產出的資產必須達內容等級");

    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), ASSET_PATH].iter().collect();
    std::fs::write(&path, &text).expect("寫入資產");

    println!("已寫入 {}", path.display());
    println!("  {} 位元組、{} 行", text.len(), text.lines().count());
    for (opponents, ranking) in &rankings {
        let strongest = ranking.strongest_first();
        println!(
            "  {opponents} 名對手：最強 {} → 最弱 {}",
            strongest[0].label(),
            strongest[168].label()
        );
    }
    println!("\n記得把資產與程式一起提交——兩者是同一個版本。");
}
