# 9-max 德州撲克模擬平台

單機 Windows 桌面應用（開發中）。規格文件見下方「現行文件」。

## 現行文件（僅此四份具規範力）

| 文件 | 範圍 |
|---|---|
| [`9max平台核心規格.md`](9max平台核心規格.md) | 產品邊界、桌型、統計、效能的權威基線 |
| [`德州撲克規則細則.md`](德州撲克規則細則.md) | 牌局規則（完全比照現實德州撲克，TDA 標準） |
| [`9max模擬平台實做計劃.md`](9max模擬平台實做計劃.md) | 里程碑、工期、人力、風險 |
| [`UI面板詳細規格.md`](UI面板詳細規格.md) | 面板 D／F 欄位級規格、視覺設計方向 |

## 專案結構

```
apps/
├── engine      Rust 規則引擎（牌局唯一權威，零外部相依）
├── storage     SQLite 事件 log、RunManifest、重播資料
├── ipc         型別化 IPC 契約（TS 型別由此產生）
├── devserver   開發用 HTTP 外殼（M3 由 Tauri command 取代）
└── ui          React + TypeScript 前端
packages/
└── poker-types 由 Rust structs 產生的 TS 型別（請勿手改）
```

**牌局邏輯只在 `engine` 執行**，UI 不複製任何遊戲邏輯（實做計劃鐵則 6）。

## 開發環境

需要 Rust 1.85+、Node 22+、pnpm。

```bash
# 引擎與規則測試（R1–R23 驗收向量）
cargo test

# 型別檢查與 lint
cargo clippy --all-targets

# 由 Rust structs 重新產生前端 TS 型別
cargo test -p poker-ipc export_bindings
```

## 執行 M0 垂直切片

需要兩個行程：

```bash
# 1. 引擎 + 儲存 + IPC（會先產生 500 手示範資料）
cargo run -p poker-devserver

# 2. 前端（另開終端機）
pnpm install
pnpm --filter @taxes-leo/ui dev
```

開啟 http://localhost:5180 可瀏覽逐手 log、牌桌重播與行動序列。

> `devserver` 只是開發鷹架，不是產品的一部分。M3 會換成 Tauri command，
> 呼叫的是同一組 `IpcHandler` 方法，前端只需替換 `apps/ui/src/api.ts`
> 的傳輸層實作。Tauri 在 Linux 需要 webkit2gtk，開發機沒有時仍可用
> 這條路徑完整開發與測試。

## 牌手顧問校準流程

顧問**不需要安裝任何開發工具**，用瀏覽器開啟單一 HTML 檔即可。

```bash
# 產生互動工作台（可拉滑桿、即時預覽、匯出參數）
cargo run --release --example calibration_workbench
# → target/calibration-workbench.html

# 產生唯讀報告（只看範圍，不可調整）
cargo run --release --example calibration_report
# → target/calibration-report.html

# 顧問回傳 JSON 後，用歸因工具檢視單格意見的連帶影響
cargo run --release --example attribute_feedback
```

**工作台的使用方式**：把 HTML 寄給顧問 → 他在本機瀏覽器開啟 → 拉左側參數滑桿，
右側 13×13 矩陣即時重算 → 按「匯出參數」下載 JSON → 寄回。全程不需要伺服器。

**兩道防線**：

1. **漂移自我校驗**。工作台的預覽由 JS 重算，可能與 Rust 引擎漂移。因此匯出時由
   Rust 算好全部 1,859 個樣本（11 個節點 × 169 格）內嵌進頁面，JS 載入時用自己的
   實作重算並逐格比對；不一致即在頁面頂端顯示紅色警告。漂移會被當場抓到，
   而不是靜默誤導顧問。
2. **回讀時重新驗證**。`parse_workbench_export` 與 `apply_import` 不信任前端擋過的
   結果，逐項檢查範圍，且**越界時整批拒絕**——部分套用會產生一組沒有人簽核過的
   混合設定。回讀後 `consultant_approved` 仍為 `false`：調過參數不等於完成簽核。

> 工作台只負責預覽，**正式的 727,038 格全表一律由 Rust 引擎展開**。

## 目前進度

`cargo test --workspace` 全綠，共 113 個測試。

| 里程碑 | 狀態 |
|---|---|
| M0 垂直切片 | 全鏈路可跑通；兩道硬閘門未過（見下） |
| M1 規則層 | 完成 |
| M2 策略與 Bot 層 | 型別骨架已建，核心內容未動 |
| M3 桌面應用 UI／M4 收尾與發佈 | 未開始 |

**已完成**

- **M0 垂直切片**：引擎 → SQLite → IPC → React → 逐手重播全鏈路可跑通。每手 log
  229 位元組，外推 100 萬手約 218 MB（門檻 2 GB）
- **M1 規則層**：規則細則第九章的 R1–R23 驗收向量全數通過。R24（dead 位的位置標籤）
  與 R25（座位數不變量）有等價測試，但未掛 R 編號
- **M2 型別骨架**：`DecisionView` 在型別層隔離隱藏資訊（結構上無欄位可承載他人底牌或
  牌堆順序）、`ActionDistribution` 以萬分比整數表示頻率、169 手牌類

**M0 未過的兩道硬閘門**（實做計劃第四章列為「不過不進 M1」）

1. **Equity 時間預算實測**：尚無 equity 程式碼，批次模式 p50 ≤1 ms／p99 ≤5 ms 未驗證。
   此閘門失守則「100 萬手 ≤12 小時」在 M2 必然跳票
2. **內容體積產製管線**：約 32 萬格 preflop baseline 的產製方式未定案。產製管線定案前，
   M2 的 6–8 週估算不成立

**其他未完成的 M0 凍結項**：面板 A／C／E／G 的欄位級規格（M3 開工前提，
[`UI面板詳細規格.md`](UI面板詳細規格.md) 目前只有 B／D／F）、多人 Equity spike、
變異數削減 spike、258V 基準測試環境。

**M2 待做**：RangeTracker、preflop／postflop 策略表、7 組人格、多人 Equity、
Analytics 與報表、顧問校準工具。
