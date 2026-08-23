//! 進程層級的 equity 排序表。
//!
//! **執行層與面板 D 必須共用同一份排序。** 兩邊各自建一份的話，只要
//! 取樣數不同，面板上看到的範圍就與實際跑出來的不一樣——而那種不一致
//! 完全沒有徵兆，使用者只會覺得「策略表寫的跟 Bot 打的不同」。
//!
//! # 為什麼不現算
//!
//! 內容級排序（20,000 取樣 × 169 類 × 4 種對手數）在 release 建置要 5 秒、
//! **debug 建置要 80 秒**。舊版在桌面殼啟動時就丟一條執行緒去算，而
//! `strategy_matrix` 這類同步 Tauri command 跑在主執行緒上，一呼叫就整個
//! 卡住——Windows 判成 AppHangB1，使用者看到的是「按下去就死了」。
//!
//! 現在排序離線產製、以版本化資產編進二進位檔（見
//! [`poker_engine::strategy::ranking_asset`]），載入是解析六千多位元組的
//! 純文字，成本可以忽略。**這裡不再有任何 Monte Carlo。**
//!
//! # debug 的低樣本退路
//!
//! 資產載不進來時（產製前的開發中途、檔案被改壞），debug 建置退回低樣本
//! 現算，讓開發不必停下來。那份排序**不是正式內容**，因此
//! [`Status::content_grade`] 為 false，面板與 RunManifest 都會照實標示。
//! release 建置沒有這條退路：出貨的程式寧可明確失敗，也不能安靜地拿一份
//! 不夠格的排序去跑一整晚的統計。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use poker_engine::bot::{BotAgent, MAX_EXPECTED_OPPONENTS};
use poker_engine::strategy::ranking::{EquityRanking, CONTENT_GRADE_SAMPLES};
use poker_engine::strategy::ranking_asset;

/// debug 退路的取樣數。
///
/// 刻意遠低於內容級門檻：這條路只是不讓開發停擺，跑得快比跑得準重要，
/// 而「不準」已經由 `content_grade == false` 明白寫在面板與快照上。
pub const DEBUG_FALLBACK_SAMPLES: u64 = 500;

/// 排序的來源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// 離線產製的版本化資產（正式內容）
    Asset { format: u32 },
    /// debug 建置在資產不可用時的低樣本替代品（**非正式內容**）
    DebugFallback,
}

impl Source {
    /// 快照與面板共用的來源鍵。
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Asset { format } => format!("asset/v{format}"),
            Self::DebugFallback => "debugFallback".to_owned(),
        }
    }
}

/// 已載入的排序表。
#[derive(Debug)]
pub struct Rankings {
    source: Source,
    samples: u64,
    table: BTreeMap<usize, EquityRanking>,
}

impl Rankings {
    #[must_use]
    pub const fn source(&self) -> Source {
        self.source
    }

    #[must_use]
    pub const fn samples(&self) -> u64 {
        self.samples
    }

    /// 取樣數是否足以產製正式內容。
    #[must_use]
    pub const fn is_content_grade(&self) -> bool {
        self.samples >= CONTENT_GRADE_SAMPLES
    }

    #[must_use]
    pub const fn table(&self) -> &BTreeMap<usize, EquityRanking> {
        &self.table
    }

    /// 指定對手數的排序。
    ///
    /// 超出快取範圍時退回最接近的一檔而不是 panic：`expected_opponents`
    /// 的上界由引擎的常數決定，兩邊若哪天不同步，寧可算得略偏也不要讓
    /// 面板整個開不起來。
    #[must_use]
    pub fn for_opponents(&self, opponents: usize) -> &EquityRanking {
        self.table
            .get(&opponents)
            .or_else(|| self.table.values().next_back())
            .or_else(|| self.table.values().next())
            .expect("排序表至少有一檔")
    }
}

/// 排序現況，**不會失敗**。
///
/// 面板 D 一開就要說明自己畫的是什麼內容；載入失敗時它該顯示的是
/// 「內容載不進來，原因是⋯」，而不是整個面板開不起來。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub samples: u64,
    pub content_grade: bool,
    /// `asset/v1`／`debugFallback`／`unavailable`
    pub source: String,
    /// 給使用者看的一句話說明
    pub note: String,
    /// 載入失敗的原因，成功為 `None`
    pub error: Option<String>,
}

static RANKINGS: OnceLock<Result<Rankings, String>> = OnceLock::new();

/// 取得進程共用的排序表。第一次呼叫會載入資產，之後直接取用。
///
/// # Errors
/// 資產載入失敗、且本建置沒有 debug 退路時回傳原因。
pub fn load() -> Result<&'static Rankings, &'static str> {
    RANKINGS
        .get_or_init(init)
        .as_ref()
        .map_err(String::as_str)
}

/// 指定對手數的排序。
///
/// # Errors
/// 同 [`load`]。
pub fn for_opponents(opponents: usize) -> Result<&'static EquityRanking, &'static str> {
    load().map(|rankings| rankings.for_opponents(opponents))
}

/// 載入結果的摘要。與 [`load`] 共用同一次載入，不會重算。
#[must_use]
pub fn status() -> Status {
    match load() {
        Ok(rankings) => Status {
            samples: rankings.samples(),
            content_grade: rankings.is_content_grade(),
            source: rankings.source().key(),
            note: match rankings.source() {
                Source::Asset { format } => format!(
                    "離線產製的排序資產 v{format}，{} 取樣",
                    rankings.samples()
                ),
                Source::DebugFallback => format!(
                    "debug 建置的低樣本替代品（{} 取樣）。**非正式內容**，不得作為統計依據",
                    rankings.samples()
                ),
            },
            error: None,
        },
        Err(reason) => Status {
            samples: 0,
            content_grade: false,
            source: "unavailable".to_owned(),
            note: "equity 排序內容載入失敗，策略與執行都不可用".to_owned(),
            error: Some(reason.to_owned()),
        },
    }
}

/// 只檢查內建資產解不解得開，**不初始化共用表、不觸發任何退路**。
///
/// 啟動時的健康檢查用這個而不是 [`status`]：後者在資產壞掉的 debug 建置
/// 會就地現算低樣本替代品，而那是幾秒鐘的 Monte Carlo。把它放在啟動路徑
/// 上，等於把剛拆掉的預熱換個名字裝回去。
///
/// # Errors
/// 資產無法解析時回傳原因。
pub fn probe() -> Result<(u32, u64), String> {
    ranking_asset::embedded()
        .map(|asset| (asset.format, asset.samples))
        .map_err(|error| {
            format!(
                "排序資產（{}）解析失敗：{error}",
                ranking_asset::ASSET_PATH
            )
        })
}

fn init() -> Result<Rankings, String> {
    match ranking_asset::embedded() {
        Ok(asset) => {
            let missing: Vec<usize> = (1..=MAX_EXPECTED_OPPONENTS)
                .filter(|opponents| !asset.rankings.contains_key(opponents))
                .collect();
            // 缺檔不是可以將就的狀況：那些節點在執行層會退回別的人數，
            // 範圍系統性偏掉而且完全不報錯（見 `bot::agent::preflop_baseline`）
            if !missing.is_empty() {
                return fallback(&format!(
                    "排序資產缺少 {missing:?} 名對手的檔位（需要 1–{MAX_EXPECTED_OPPONENTS}）"
                ));
            }
            crate::log::info(&format!(
                "equity 排序已載入：資產 v{}，{} 取樣，對手數 {:?}",
                asset.format,
                asset.samples,
                asset.rankings.keys().collect::<Vec<_>>()
            ));
            Ok(Rankings {
                source: Source::Asset {
                    format: asset.format,
                },
                samples: asset.samples,
                table: asset.rankings,
            })
        }
        Err(error) => fallback(&format!(
            "排序資產（{}）載入失敗：{error}",
            ranking_asset::ASSET_PATH
        )),
    }
}

/// 資產不可用時的處置。
///
/// debug 退回低樣本現算，release 直接失敗——理由見模組說明。
fn fallback(reason: &str) -> Result<Rankings, String> {
    crate::log::error(reason);
    if !cfg!(debug_assertions) {
        crate::log::error(
            "release 建置沒有低樣本退路。請重新產製資產：cargo run --release -p poker-engine --example generate_rankings",
        );
        return Err(format!(
            "{reason}。請重新產製 equity 排序資產後再建置。"
        ));
    }

    crate::log::error(&format!(
        "debug 建置改用 {DEBUG_FALLBACK_SAMPLES} 取樣的替代排序。這不是正式內容，面板與 RunManifest 都會照實標示"
    ));
    Ok(Rankings {
        source: Source::DebugFallback,
        samples: DEBUG_FALLBACK_SAMPLES,
        table: BotAgent::rankings(DEBUG_FALLBACK_SAMPLES),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 載入的排序涵蓋全部預期對手數且為內容等級() {
        let rankings = load().expect("排序資產必須可載入");
        for opponents in 1..=MAX_EXPECTED_OPPONENTS {
            assert_eq!(
                rankings.for_opponents(opponents).opponents,
                opponents,
                "缺 {opponents} 名對手的排序會讓那些節點靜靜退回別的人數"
            );
        }
        assert!(
            rankings.is_content_grade(),
            "載入的必須是內容級資產，而不是 debug 退路"
        );
        assert_eq!(rankings.samples(), CONTENT_GRADE_SAMPLES);
    }

    /// 面板與快照都靠這一份摘要說明「你看到的是什麼內容」。
    #[test]
    fn 摘要如實回報來源與等級() {
        let status = status();
        assert_eq!(status.source, "asset/v1");
        assert!(status.content_grade);
        assert_eq!(status.error, None);
        assert!(status.note.contains("20000"), "說明必須帶上取樣數");
    }

    /// 探測不得初始化共用表，也不得現算任何東西。
    #[test]
    fn 探測是純解析且回報資產資訊() {
        let start = std::time::Instant::now();
        let (format, samples) = probe().expect("內建資產必須解得開");
        let elapsed = start.elapsed();

        assert_eq!(format, 1);
        assert_eq!(samples, CONTENT_GRADE_SAMPLES);
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "探測花了 {elapsed:?}——啟動路徑上不得有現算"
        );
    }

    /// 載入不得有任何 Monte Carlo：這正是舊版假死的原因。
    #[test]
    fn 載入在毫秒等級完成() {
        let start = std::time::Instant::now();
        load().expect("載入");
        let first = start.elapsed();
        assert!(
            first < std::time::Duration::from_millis(500),
            "第一次載入花了 {first:?}——資產路徑不該有現算"
        );
    }
}
