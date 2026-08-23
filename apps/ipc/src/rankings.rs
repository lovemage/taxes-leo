//! 進程層級的 equity 排序快取。
//!
//! 20,000 次取樣要跑近兩秒，而結果只取決於取樣數與對手數，因此算一次
//! 之後全程共用。沒有這層快取的話，每個 run 與面板 D 的每次切換節點
//! 都會重算一遍，使用者付的是純粹的等待。
//!
//! **執行層與面板 D 必須共用同一份排序。** 兩邊各自建一份的話，只要
//! 取樣數不同，面板上看到的範圍就與實際跑出來的不一樣——而那種不一致
//! 完全沒有徵兆，使用者只會覺得「策略表寫的跟 Bot 打的不同」。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use poker_engine::bot::BotAgent;
use poker_engine::strategy::ranking::EquityRanking;

/// 內容級取樣數（`EquityRanking::is_content_grade` 的門檻）。
///
/// 取樣不足時可玩性調整會被雜訊蓋過，產生的範圍與工作台顯示的對不上。
pub const RANKING_SAMPLES: u64 = 20_000;

static RANKINGS: OnceLock<BTreeMap<usize, EquityRanking>> = OnceLock::new();

/// 全部對手數的排序表。第一次呼叫會計算，之後直接取用。
#[must_use]
pub fn all() -> &'static BTreeMap<usize, EquityRanking> {
    RANKINGS.get_or_init(|| BotAgent::rankings(RANKING_SAMPLES))
}

/// 指定對手數的排序。
///
/// 超出快取範圍時退回最接近的一檔而不是 panic：`expected_opponents`
/// 的上界由引擎的常數決定，兩邊若哪天不同步，寧可算得略偏也不要讓
/// 面板整個開不起來。
#[must_use]
pub fn for_opponents(opponents: usize) -> &'static EquityRanking {
    let table = all();
    table
        .get(&opponents)
        .or_else(|| table.values().next_back())
        .or_else(|| table.values().next())
        .expect("排序表至少有一檔")
}
