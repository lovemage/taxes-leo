//! `peek_status` 的關鍵性質：呼叫它不會初始化共用表。
//!
//! 狀態列在開機畫面就會讀執行環境，屬於啟動路徑。若那條路徑會觸發載入，
//! 資產壞掉的 debug 建置就會就地現算一份低樣本替代品——那正是
//! `rankings` 模組註解要求避免的「把預熱換個名字裝回去」。
//!
//! 這條測試刻意獨立成一個檔案（＝獨立的行程）。`RANKINGS` 是進程層級的
//! `OnceLock`，與其他測試同檔的話會被它們先載走，斷言就永遠是假的。

use poker_ipc::rankings;

#[test]
fn peek_status_不初始化共用表() {
    assert!(!rankings::is_loaded(), "測試起點必須是尚未載入");

    // 真正的迴歸點：狀態列讀的是 runtime::status()，它一旦改回呼叫
    // rankings::status()，啟動路徑就又會觸發載入
    let _ = poker_ipc::runtime::status();
    assert!(
        !rankings::is_loaded(),
        "runtime::status 不得初始化共用表——狀態列掛載時就會呼叫它"
    );

    let peeked = rankings::peek_status();
    assert_eq!(peeked.source, "asset/v1");
    assert!(peeked.content_grade, "內建資產應為正式內容");
    assert!(
        !rankings::is_loaded(),
        "peek_status 不得初始化共用表——狀態列會在啟動路徑上呼叫它"
    );

    // 真的載入之後，兩者對同一份排序的判定必須一致。不一致的話，狀態列
    // 說的與執行層實際在用的就是兩回事
    let loaded = rankings::status();
    assert!(rankings::is_loaded());
    assert_eq!(loaded.source, peeked.source);
    assert_eq!(loaded.samples, peeked.samples);
    assert_eq!(loaded.content_grade, peeked.content_grade);
}
