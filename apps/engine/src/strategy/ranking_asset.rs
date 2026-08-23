//! 內容級 equity 排序的離線資產。
//!
//! # 為什麼需要這個檔案
//!
//! 內容級排序是 [`CONTENT_GRADE_SAMPLES`] 取樣 × 169 類 × 1–4 名對手的
//! Monte Carlo：release 建置約 5 秒，**debug 建置約 80 秒**。桌面程式若在
//! 啟動時或第一次請求時現算，使用者面對的是一個不回應的視窗——Windows
//! 直接判成 AppHangB1，而使用者只會說「按下去就死了」。
//!
//! 因此排序**離線產製一次**，以版本化的資產隨程式載入：
//!
//! ```text
//! cargo run --release -p poker-engine --example generate_rankings
//! ```
//!
//! # 為什麼只存量測值
//!
//! 資產只記每一類的 equity；排名、順序與百分位一律由
//! [`EquityRanking::from_measurements`] 推導，與現算走同一段程式碼。
//! 若讓資產自己存排名，格式與 `compute` 之間就有第二處實作，兩者漂移
//! 不會報錯，只會讓面板顯示的範圍與 Bot 實際打的不一樣。
//!
//! # 為什麼是純文字
//!
//! 引擎沒有任何相依（連 serde 都沒有），而這份東西是**內容**：顧問要看得
//! 懂，diff 要讀得出哪一類動了。純文字兩者都成立。校驗碼防的是手改與
//! 傳輸損壞，不是惡意竄改——那是另一個層級的問題。
//!
//! # 換行不參與校驗
//!
//! 校驗碼算在**解析後的數值**上，不是原始位元組。Windows 端 checkout 出
//! CRLF 時資產仍然可用；把換行納入校驗只會製造一種只在某些機器上出現的
//! 載入失敗。

use std::collections::BTreeMap;
use std::fmt;

use crate::strategy::hand_class::HandClass;
use crate::strategy::ranking::{EquityRanking, RankingError, CONTENT_GRADE_SAMPLES, RANKING_SEED};

/// 資產格式版本。欄位語意改變時遞增，舊資產會在載入時被擋下。
pub const ASSET_FORMAT: u32 = 1;

/// 隨程式一起編譯進去的資產路徑（相對於 `apps/engine`）。
pub const ASSET_PATH: &str = "assets/equity-rankings-v1.txt";

/// 資產內容。放在二進位檔裡而不是執行期讀檔：Tauri 打包後的工作目錄
/// 不由我們決定，讀檔會在使用者的機器上失敗而在開發機上永遠成功。
const EMBEDDED: &str = include_str!("../../assets/equity-rankings-v1.txt");

/// 一份載入完成的排序資產。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankingAsset {
    pub format: u32,
    /// 產製時使用的 seed。與 [`RANKING_SEED`] 不同即代表內容版本已經換過
    pub seed: u64,
    pub samples: u64,
    pub rankings: BTreeMap<usize, EquityRanking>,
}

impl RankingAsset {
    /// 取樣數是否足以產製正式內容。
    #[must_use]
    pub const fn is_content_grade(&self) -> bool {
        self.samples >= CONTENT_GRADE_SAMPLES
    }
}

/// 載入資產時的失敗原因。
///
/// 每一種都帶著足夠定位問題的資訊：載入失敗時使用者看到的是「策略內容
/// 載不進來」，工程端看到的必須是「哪一行、期望什麼」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetError {
    /// 檔案是空的或只有註解
    Empty,
    /// 第 `line` 行無法解析
    Malformed { line: usize, reason: String },
    /// 缺少必要的表頭欄位
    MissingHeader(&'static str),
    /// 格式版本與本程式不符
    FormatMismatch { found: u32, expected: u32 },
    /// 產製 seed 與引擎常數不符，代表資產屬於另一份內容
    SeedMismatch { found: u64, expected: u64 },
    /// 某個對手數的 169 類不完整
    Ranking { opponents: usize, error: RankingError },
    /// 校驗碼不符：資產被改過或損壞
    Checksum { found: u64, expected: u64 },
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "排序資產是空的"),
            Self::Malformed { line, reason } => write!(f, "第 {line} 行無法解析：{reason}"),
            Self::MissingHeader(key) => write!(f, "表頭缺少 {key}"),
            Self::FormatMismatch { found, expected } => {
                write!(f, "資產格式為 v{found}，本程式只認得 v{expected}")
            }
            Self::SeedMismatch { found, expected } => write!(
                f,
                "資產的 seed 為 {found:#x}，引擎為 {expected:#x}——這是另一份內容"
            ),
            Self::Ranking { opponents, error } => {
                write!(f, "{opponents} 名對手的排序不完整：{error}")
            }
            Self::Checksum { found, expected } => write!(
                f,
                "校驗碼不符（檔案 {found:#018x}，實算 {expected:#018x}）：資產被改過或已損壞"
            ),
        }
    }
}

/// 載入編譯進程式的資產。
///
/// # Errors
/// 資產缺漏、格式不符或校驗碼不符時回傳 [`AssetError`]。
pub fn embedded() -> Result<RankingAsset, AssetError> {
    decode(EMBEDDED)
}

/// 把排序表寫成資產文字。
#[must_use]
pub fn encode(samples: u64, rankings: &BTreeMap<usize, EquityRanking>) -> String {
    let mut out = String::new();
    out.push_str("# 9max 模擬平台 — equity 排序資產\n");
    out.push_str("#\n");
    out.push_str("# 這是離線產製的內容，請勿手改。重新產生：\n");
    out.push_str("#   cargo run --release -p poker-engine --example generate_rankings\n");
    out.push_str("#\n");
    out.push_str("# 每個區塊是一種「預期對手數」下的 169 類 equity（萬分比），由強到弱。\n");
    out.push_str("# 排名與百分位不存在這裡：兩者由 EquityRanking::from_measurements 推導，\n");
    out.push_str("# 與引擎現算走同一段程式碼，因此不可能與執行層漂移。\n");
    out.push_str(&format!("format {ASSET_FORMAT}\n"));
    out.push_str(&format!("seed {RANKING_SEED}\n"));
    out.push_str(&format!("samples {samples}\n"));
    out.push_str(&format!(
        "checksum {:#018x}\n",
        checksum(samples, rankings)
    ));

    for (opponents, ranking) in rankings {
        out.push_str(&format!("\nopponents {opponents}\n"));
        for (class, equity) in ranking.measurements() {
            out.push_str(&format!("{} {}\n", class.label(), equity));
        }
    }
    out
}

/// 解析資產文字。
///
/// # Errors
/// 見 [`AssetError`]。
pub fn decode(text: &str) -> Result<RankingAsset, AssetError> {
    let mut format: Option<u32> = None;
    let mut seed: Option<u64> = None;
    let mut samples: Option<u64> = None;
    let mut declared_checksum: Option<u64> = None;
    // 逐區塊累積量測值。用 Vec 而不是直接建表，是因為 169 類的完整性
    // 要等區塊結束才知道
    let mut blocks: Vec<(usize, Vec<(HandClass, u64)>)> = Vec::new();

    for (offset, raw) in text.lines().enumerate() {
        let line = offset + 1;
        let content = raw.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let (key, value) = content.split_once(char::is_whitespace).ok_or_else(|| {
            AssetError::Malformed {
                line,
                reason: format!("「{content}」不是「鍵 值」的形狀"),
            }
        })?;
        let value = value.trim();

        match key {
            "format" => format = Some(parse_u32(value, line)?),
            "seed" => seed = Some(parse_u64(value, line)?),
            "samples" => samples = Some(parse_u64(value, line)?),
            "checksum" => declared_checksum = Some(parse_hex(value, line)?),
            "opponents" => {
                let count = usize::try_from(parse_u64(value, line)?).map_err(|_| {
                    AssetError::Malformed {
                        line,
                        reason: format!("對手數 {value} 超出範圍"),
                    }
                })?;
                blocks.push((count, Vec::with_capacity(169)));
            }
            label => {
                let class = HandClass::from_label(label).ok_or_else(|| AssetError::Malformed {
                    line,
                    reason: format!("未知的牌類：{label}"),
                })?;
                let equity = parse_u64(value, line)?;
                let block = blocks.last_mut().ok_or(AssetError::Malformed {
                    line,
                    reason: "牌類出現在任何 opponents 區塊之前".to_owned(),
                })?;
                block.1.push((class, equity));
            }
        }
    }

    if blocks.is_empty() && format.is_none() {
        return Err(AssetError::Empty);
    }

    let format = format.ok_or(AssetError::MissingHeader("format"))?;
    if format != ASSET_FORMAT {
        return Err(AssetError::FormatMismatch {
            found: format,
            expected: ASSET_FORMAT,
        });
    }
    let seed = seed.ok_or(AssetError::MissingHeader("seed"))?;
    if seed != RANKING_SEED {
        return Err(AssetError::SeedMismatch {
            found: seed,
            expected: RANKING_SEED,
        });
    }
    let samples = samples.ok_or(AssetError::MissingHeader("samples"))?;
    let declared_checksum = declared_checksum.ok_or(AssetError::MissingHeader("checksum"))?;
    if blocks.is_empty() {
        return Err(AssetError::Empty);
    }

    let mut rankings = BTreeMap::new();
    for (opponents, measured) in blocks {
        let ranking = EquityRanking::from_measurements(opponents, samples, &measured)
            .map_err(|error| AssetError::Ranking { opponents, error })?;
        rankings.insert(opponents, ranking);
    }

    let actual = checksum(samples, &rankings);
    if actual != declared_checksum {
        return Err(AssetError::Checksum {
            found: declared_checksum,
            expected: actual,
        });
    }

    Ok(RankingAsset {
        format,
        seed,
        samples,
        rankings,
    })
}

// ── 校驗碼 ──────────────────────────────────────────────────────────────

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 走過解析後的數值。
///
/// 算在數值上而不是原始位元組：註解、空白與換行因此都不影響結果，
/// 而 Windows 端 checkout 出 CRLF 也不會讓資產突然載不進來。
#[must_use]
fn checksum(samples: u64, rankings: &BTreeMap<usize, EquityRanking>) -> u64 {
    let mut hash = feed(FNV_OFFSET, u64::from(ASSET_FORMAT));
    hash = feed(hash, RANKING_SEED);
    hash = feed(hash, samples);
    for (opponents, ranking) in rankings {
        hash = feed(hash, u64::try_from(*opponents).unwrap_or(u64::MAX));
        for (class, equity) in ranking.measurements() {
            hash = feed(hash, u64::try_from(class.index()).unwrap_or(u64::MAX));
            hash = feed(hash, equity);
        }
    }
    hash
}

fn feed(seed: u64, value: u64) -> u64 {
    let mut hash = seed;
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn parse_u64(value: &str, line: usize) -> Result<u64, AssetError> {
    value.parse().map_err(|_| AssetError::Malformed {
        line,
        reason: format!("「{value}」不是整數"),
    })
}

fn parse_u32(value: &str, line: usize) -> Result<u32, AssetError> {
    value.parse().map_err(|_| AssetError::Malformed {
        line,
        reason: format!("「{value}」不是整數"),
    })
}

fn parse_hex(value: &str, line: usize) -> Result<u64, AssetError> {
    let digits = value.strip_prefix("0x").unwrap_or(value);
    u64::from_str_radix(digits, 16).map_err(|_| AssetError::Malformed {
        line,
        reason: format!("「{value}」不是十六進位整數"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 低樣本的兩檔排序，夠測格式而不用等 Monte Carlo。
    fn sample_table() -> BTreeMap<usize, EquityRanking> {
        [1usize, 2]
            .into_iter()
            .map(|opponents| (opponents, EquityRanking::compute(opponents, 120)))
            .collect()
    }

    #[test]
    fn 編碼再解碼還原同一份排序表() {
        let table = sample_table();
        let asset = decode(&encode(120, &table)).expect("解析");
        assert_eq!(asset.rankings, table, "往返必須是無損的");
        assert_eq!(asset.samples, 120);
        assert_eq!(asset.format, ASSET_FORMAT);
        assert!(!asset.is_content_grade(), "120 取樣不得標為內容等級");
    }

    /// CRLF 是 Windows checkout 的常態，不該讓資產載不進來。
    #[test]
    fn crlf_換行不影響載入() {
        let table = sample_table();
        let text = encode(120, &table).replace('\n', "\r\n");
        assert_eq!(decode(&text).expect("解析").rankings, table);
    }

    #[test]
    fn 改過一格就被校驗碼擋下() {
        let table = sample_table();
        let text = encode(120, &table);
        // 把最強那一類的 equity 動掉一點
        let strongest = table[&1].measurements()[0];
        let broken = text.replacen(
            &format!("{} {}", strongest.0.label(), strongest.1),
            &format!("{} {}", strongest.0.label(), strongest.1 + 1),
            1,
        );
        assert_ne!(broken, text, "測試本身必須真的改到內容");
        assert!(
            matches!(decode(&broken), Err(AssetError::Checksum { .. })),
            "手改過的資產必須被擋下"
        );
    }

    #[test]
    fn 缺一類就被擋下而不是靜默補零() {
        let table = sample_table();
        let text = encode(120, &table);
        let dropped: String = text
            .lines()
            .filter(|line| !line.starts_with("72o "))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            matches!(decode(&dropped), Err(AssetError::Ranking { .. })),
            "缺類必須失敗；靜默補零會產生一份看起來正常、實際錯誤的排序"
        );
    }

    #[test]
    fn 格式版本不符時拒絕載入() {
        let text = encode(120, &sample_table()).replace("format 1", "format 2");
        assert!(matches!(
            decode(&text),
            Err(AssetError::FormatMismatch { found: 2, .. })
        ));
    }

    #[test]
    fn 空檔案回報為空而不是恐慌() {
        assert_eq!(decode("# 只有註解\n"), Err(AssetError::Empty));
    }

    // ── 隨程式編譯進去的那一份 ─────────────────────────────────────────

    /// 這一組守的是「出貨的二進位檔裡真的有一份可用的內容級排序」。
    /// 資產沒產生或產壞了，桌面程式會在使用者按下開始時才炸開。
    #[test]
    fn 內建資產可載入且為內容等級() {
        let asset = embedded().expect("內建資產必須可載入");
        assert_eq!(asset.seed, RANKING_SEED);
        assert!(
            asset.is_content_grade(),
            "出貨的資產必須達內容等級（取樣 {}）",
            asset.samples
        );
        for (opponents, ranking) in &asset.rankings {
            assert_eq!(ranking.strongest_first().len(), 169);
            assert!(ranking.is_content_grade());
            assert_eq!(ranking.opponents, *opponents);
        }
    }

    /// 內建資產必須涵蓋 `baseline::expected_opponents` 會要到的全部檔位。
    ///
    /// 少一檔的話那些節點在執行層會退回別的人數，範圍系統性偏掉——
    /// 而那不會報錯（見 `bot::agent::preflop_baseline`）。
    #[test]
    fn 內建資產涵蓋全部預期對手數() {
        let asset = embedded().expect("內建資產");
        for opponents in 1..=crate::bot::MAX_EXPECTED_OPPONENTS {
            assert!(
                asset.rankings.contains_key(&opponents),
                "資產缺少 {opponents} 名對手的排序"
            );
        }
    }

    /// 資產是不是「用宣稱的 seed 與取樣數真的算出來的」。
    ///
    /// 預設不跑：20,000 取樣在 debug 建置要八十秒。改動排序演算法、
    /// `RANKING_SEED` 或 equity 計算之後，用下面這行重新驗一次：
    ///
    /// ```text
    /// cargo test --release -p poker-engine 內建資產與重算結果一致 -- --ignored
    /// ```
    #[test]
    #[ignore = "重算 20,000 取樣：release 約 5 秒、debug 約 80 秒"]
    fn 內建資產與重算結果一致() {
        let asset = embedded().expect("內建資產");
        for (opponents, stored) in &asset.rankings {
            let fresh = EquityRanking::compute(*opponents, asset.samples);
            assert_eq!(
                *stored, fresh,
                "{opponents} 名對手的資產內容與重算結果不同，請重新產製資產"
            );
        }
    }
}
