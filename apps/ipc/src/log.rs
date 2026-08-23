//! 錯誤日誌。
//!
//! # 為什麼不是 eprintln!
//!
//! 產品建置的 Windows 端帶 `windows_subsystem = "windows"`，**沒有主控台**：
//! `eprintln!` 寫進一個不存在的 stderr，等於什麼都沒記。使用者回報「按下去
//! 沒反應」時，沒有日誌就只剩下猜。
//!
//! 因此桌面殼在啟動時呼叫 [`init_file`] 指定落地路徑，之後 IPC 層的每一筆
//! 錯誤都會同時寫到檔案與 stderr（開發時看得到、出貨後查得到）。
//!
//! # 為什麼在這一層而不是桌面殼
//!
//! 與 IPC 契約同一個理由：Tauri 在 Linux 編不動，寫在桌面殼裡就完全無法
//! 測試，而「日誌到底有沒有寫出來」正是出事時唯一能依靠的東西。
//!
//! # 時間戳
//!
//! 這裡讀系統時鐘。引擎刻意不讀（時間不得進入可重現路徑），但日誌不是
//! 可重現路徑的一部分——沒有時間的日誌對排查幾乎沒有用。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<File>> {
    FILE.get_or_init(|| Mutex::new(None))
}

/// 把日誌落到 `path`。
///
/// 以附加模式開啟：上一次執行的日誌必須留著，否則「重開一次就好了」會把
/// 唯一的線索一起洗掉。
///
/// # Errors
/// 無法建立目錄或開檔時回傳 `io::Error`。呼叫端可以繼續跑——沒有檔案日誌
/// 只是比較難查，不是不能用。
pub fn init_file(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    if let Ok(mut guard) = slot().lock() {
        *guard = Some(file);
    }
    info(&format!("日誌開始：{}", path.display()));
    Ok(())
}

pub fn info(message: &str) {
    write("INFO", message);
}

pub fn warn(message: &str) {
    write("WARN", message);
}

pub fn error(message: &str) {
    write("ERROR", message);
}

fn write(level: &str, message: &str) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = format!("[{stamp}] {level} {message}");
    eprintln!("{line}");
    // 鎖毀損時不再嘗試寫檔，但 stderr 已經寫過了——日誌本身不該是
    // 讓程式倒下的原因
    if let Ok(mut guard) = slot().lock() {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 未初始化時寫日誌不恐慌() {
        // 桌面殼以外的呼叫端（測試、dev server）不一定會設定檔案
        error("測試訊息，沒有檔案接收");
    }

    #[test]
    fn 初始化後把訊息附加到檔案() {
        let dir = std::env::temp_dir().join(format!(
            "poker-ipc-log-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let path = dir.join("desktop.log");
        init_file(&path).expect("開檔");
        error("排序資產載入失敗（測試）");

        let text = std::fs::read_to_string(&path).expect("讀回");
        assert!(text.contains("日誌開始"), "開檔本身要留下紀錄");
        assert!(
            text.contains("ERROR 排序資產載入失敗（測試）"),
            "錯誤必須落到檔案裡，實際內容：{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
