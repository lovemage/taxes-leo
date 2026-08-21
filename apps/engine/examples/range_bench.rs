//! RangeTracker 的時間預算量測。
//!
//! Reach weighting 每次公開行動都要跑一次，一手最多數十次，
//! 100 萬手就是數千萬次。這裡確認它不會吃掉 equity 的時間預算。

use std::time::Instant;

use poker_engine::card::Card;
use poker_engine::strategy::range::Range;

fn main() {
    let card = |t: &str| Card::parse(t).expect("牌張");

    let rounds = 2_000;
    let mut range = Range::full();

    let start = Instant::now();
    for _ in 0..rounds {
        range.apply_reach(&|_| 8_000);
    }
    let per_reach = start.elapsed().as_secs_f64() / f64::from(rounds);

    let mut range = Range::full();
    let start = Instant::now();
    for _ in 0..rounds {
        range.remove_cards(&[card("As")]);
    }
    let per_removal = start.elapsed().as_secs_f64() / f64::from(rounds);

    println!("reach weighting：{:.1} µs／次", per_reach * 1e6);
    println!("card removal：  {:.1} µs／次", per_removal * 1e6);
    println!();
    // 一手約 30 次公開行動 + 5 次發牌
    let per_hand = per_reach * 30.0 + per_removal * 5.0;
    println!("每手約 {:.2} ms", per_hand * 1e3);
    println!("100 萬手約 {:.1} 小時", per_hand * 1_000_000.0 / 3_600.0);
}
