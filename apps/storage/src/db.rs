//! SQLite 資料層：run、逐手事件 log 與查詢。
//!
//! 設計取捨：
//! - `hand` 表用 `WITHOUT ROWID` ＋ 複合主鍵 `(run_id, hand_index)`，
//!   讓「載入指定手牌」直接命中 B-tree，對應核心規格 7.2 的
//!   「100 萬手 DB 載入指定手牌 p95 ≤ 200 ms」。
//! - 寫入採批次交易（核心規格 3.2：「寫入須採背壓或批次交易，
//!   避免 UI 餓死」）。
//! - `hero_delta` 另存一欄，統計聚合不必解碼 blob。

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::codec::{decode, encode, CodecError, HandRecord};
use crate::manifest::{RunManifest, SCHEMA_VERSION};

#[derive(Debug)]
pub enum StorageError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    Codec(CodecError),
    SchemaMismatch { found: u32, expected: u32 },
    /// `RunManifest` 未通過核心規格 3.3 的必要欄位檢查
    InvalidManifest(String),
    NotFound,
}

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}
impl From<serde_json::Error> for StorageError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl From<CodecError> for StorageError {
    fn from(e: CodecError) -> Self {
        Self::Codec(e)
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// 開啟或建立資料庫檔案。
    ///
    /// # Errors
    /// 檔案無法開啟或 schema 版本不符時回傳錯誤。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// 建立記憶體資料庫，供測試與容量量測使用。
    ///
    /// # Errors
    /// 初始化失敗時回傳錯誤。
    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, StorageError> {
        // WAL 提升批次寫入吞吐；NORMAL 同步在本機單機情境下足夠，
        // 崩潰復原由 SQLite 自身的 journal 保證
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS run (
                 id            INTEGER PRIMARY KEY AUTOINCREMENT,
                 manifest      TEXT    NOT NULL,
                 created_at    INTEGER NOT NULL,
                 completed     INTEGER NOT NULL DEFAULT 0,
                 hands_played  INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS hand (
                 run_id         INTEGER NOT NULL REFERENCES run(id) ON DELETE CASCADE,
                 hand_index     INTEGER NOT NULL,
                 instance_index INTEGER NOT NULL,
                 seated         INTEGER NOT NULL,
                 hero_delta     INTEGER NOT NULL,
                 blob           BLOB    NOT NULL,
                 PRIMARY KEY (run_id, hand_index)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS hand_by_instance
                 ON hand(run_id, instance_index);",
        )?;

        let found: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| {
                r.get(0)
            })
            .optional()?;

        match found {
            None => {
                self.conn.execute(
                    "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION.to_string()],
                )?;
            }
            Some(value) => {
                let found: u32 = value.parse().unwrap_or(0);
                if found != SCHEMA_VERSION {
                    return Err(StorageError::SchemaMismatch {
                        found,
                        expected: SCHEMA_VERSION,
                    });
                }
            }
        }
        Ok(())
    }

    /// 建立一個 run，回傳 run id。
    ///
    /// # Errors
    /// manifest 不合法或寫入失敗時回傳錯誤。
    pub fn create_run(&mut self, manifest: &RunManifest) -> Result<i64, StorageError> {
        manifest.validate().map_err(StorageError::InvalidManifest)?;
        let json = serde_json::to_string(manifest)?;
        self.conn.execute(
            "INSERT INTO run(manifest, created_at, completed) VALUES (?1, ?2, 0)",
            params![json, manifest.created_at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 批次寫入多手 log。
    ///
    /// 核心規格 3.2 要求批次交易，避免逐手 commit 讓 UI 餓死。
    ///
    /// # Errors
    /// 寫入失敗時回傳錯誤，整批回滾。
    pub fn write_hands(
        &mut self,
        run_id: i64,
        hands: &[(HandRecord, usize, i64)],
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO hand
                     (run_id, hand_index, instance_index, seated, hero_delta, blob)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (record, seated, hero_delta) in hands {
                stmt.execute(params![
                    run_id,
                    i64::try_from(record.hand_index).expect("手序必在 i64 範圍"),
                    i64::try_from(record.instance_index).expect("桌次序必在 i64 範圍"),
                    i64::try_from(*seated).expect("人數必在 i64 範圍"),
                    hero_delta,
                    encode(record),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 標記 run 完成，並寫回含桌次邊界的最終 manifest。
    ///
    /// # Errors
    /// 寫入失敗時回傳錯誤。
    pub fn finish_run(
        &mut self,
        run_id: i64,
        manifest: &RunManifest,
        hands_played: u64,
    ) -> Result<(), StorageError> {
        let json = serde_json::to_string(manifest)?;
        self.conn.execute(
            "UPDATE run SET manifest = ?1, completed = 1, hands_played = ?2 WHERE id = ?3",
            params![
                json,
                i64::try_from(hands_played).expect("手數必在 i64 範圍"),
                run_id
            ],
        )?;
        Ok(())
    }

    /// 讀取 run 的 manifest。
    ///
    /// # Errors
    /// 找不到或反序列化失敗時回傳錯誤。
    pub fn load_manifest(&self, run_id: i64) -> Result<RunManifest, StorageError> {
        let json: String = self
            .conn
            .query_row("SELECT manifest FROM run WHERE id = ?1", params![run_id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or(StorageError::NotFound)?;
        Ok(serde_json::from_str(&json)?)
    }

    /// 載入指定手牌。核心規格 7.2 的 p95 ≤ 200 ms 門檻即針對此路徑。
    ///
    /// # Errors
    /// 找不到或解碼失敗時回傳錯誤。
    pub fn load_hand(&self, run_id: i64, hand_index: u64) -> Result<HandRecord, StorageError> {
        let blob: Vec<u8> = self
            .conn
            .query_row(
                "SELECT blob FROM hand WHERE run_id = ?1 AND hand_index = ?2",
                params![run_id, i64::try_from(hand_index).expect("手序必在 i64 範圍")],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(StorageError::NotFound)?;
        Ok(decode(&blob)?)
    }

    /// 分頁瀏覽（面板 G 的逐手列表）。
    ///
    /// # Errors
    /// 查詢或解碼失敗時回傳錯誤。
    pub fn page_hands(
        &self,
        run_id: i64,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<HandRecord>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT blob FROM hand WHERE run_id = ?1
             ORDER BY hand_index LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(
            params![
                run_id,
                i64::try_from(limit).expect("limit 必在 i64 範圍"),
                i64::try_from(offset).expect("offset 必在 i64 範圍")
            ],
            |r| r.get::<_, Vec<u8>>(0),
        )?;
        let mut out = Vec::new();
        for blob in rows {
            out.push(decode(&blob?)?);
        }
        Ok(out)
    }

    /// 使用者的逐手損益序列，供統計層聚合（免解碼 blob）。
    ///
    /// # Errors
    /// 查詢失敗時回傳錯誤。
    pub fn hero_deltas(&self, run_id: i64) -> Result<Vec<i64>, StorageError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT hero_delta FROM hand WHERE run_id = ?1 ORDER BY hand_index")?;
        let rows = stmt.query_map(params![run_id], |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 依日期範圍刪除 run（M0 log 規格的「清理」要求）。
    ///
    /// # Errors
    /// 刪除失敗時回傳錯誤。
    pub fn delete_runs_before(&mut self, created_before: i64) -> Result<usize, StorageError> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute(
            "DELETE FROM run WHERE created_at < ?1",
            params![created_before],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    /// 目前資料庫佔用的位元組數（不含可清除的 WAL 暫存）。
    ///
    /// M0 的 log 容量測試以此對照「100 萬手 ≤ 2GB」門檻。
    ///
    /// # Errors
    /// 查詢失敗時回傳錯誤。
    pub fn size_bytes(&self) -> Result<u64, StorageError> {
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let page_size: i64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok(u64::try_from(page_count * page_size).unwrap_or(0))
    }

    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}
