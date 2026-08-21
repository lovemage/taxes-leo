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

開啟 http://localhost:5180。瀏覽器模式只能檢視 devserver 的示範資料
（左側圖示欄的「重播」），執行模擬要用桌面殼——`start_run` 之類的執行
指令只存在於 Tauri 端。

> `devserver` 只是開發鷹架，不是產品的一部分。M3 會換成 Tauri command，
> 呼叫的是同一組 `IpcHandler` 方法，前端只需替換 `apps/ui/src/api.ts`
> 的傳輸層實作。Tauri 在 Linux 需要 webkit2gtk，開發機沒有時仍可用
> 這條路徑完整開發與測試。

## 在 Windows 上跑桌面殼

產品最終形態是 Tauri 桌面應用。**`apps/desktop` 刻意排除在 Cargo workspace 之外**
（根 `Cargo.toml` 的 `exclude`）：Tauri 在 Linux 需要 webkit2gtk，若併入 workspace，
開發機的 `cargo test` 會因為它編不過而全部停擺。Windows 用的是系統內建的 WebView2，
不需要 webkit2gtk。

**Windows 端一次性準備**：

1. [Rust](https://rustup.rs/)（安裝時會提示一併裝 MSVC build tools，必須裝）
2. [Node.js 22+](https://nodejs.org/) 與 `npm i -g pnpm`
3. WebView2 —— Win10/11 通常已內建；沒有的話裝
   [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)

**執行**：

```powershell
git pull
pnpm install

# 開發模式：自動啟動 Vite 並開出桌面視窗
cd apps\desktop
pnpm dev

# 打包成 installer（產出 NSIS 與 MSI）
pnpm build
```

> `pnpm dev` 會透過 Tauri CLI 讀取 `tauri.conf.json`，並由
> `beforeDevCommand` 自動啟動 Vite。直接執行 `cargo run` 只會啟動 Rust
> 執行檔，不會執行 Tauri CLI 的前置命令。
> 首次建置要編譯 Tauri 與 SQLite 的 C 原始碼，約需數分鐘。

> **權限（capability）改動要實機驗。** Tauri v2 的 ACL 只在執行期生效：
> 前端呼叫 `event.listen` 若沒有對應權限，TypeScript、Rust release 建置
> 與 installer 打包全都會過，直到實機點下去才吐
> `Command plugin:event|listen not allowed by ACL`。
> 權限宣告在 [`apps/desktop/capabilities/default.json`](apps/desktop/capabilities/default.json)，
> 目前只給主視窗 `core:event` 的 listen／unlisten——事件由 Rust 端送出，
> 前端不需要 emit。`generate_handler!` 註冊的自訂 command（`start_run` 等）
> 不受 ACL 管轄，因此不必逐一列出。

**打包產出**（2026-08-21 於 Windows 實測）：

| 目標 | 結果 |
|---|---|
| NSIS installer | 2.82 MB |
| MSI installer | 4.06 MB |

> **MSI 必須指定 `wix.language: "zh-TW"`**。WiX 預設走 `en-US`／code page 1252，
> 而產品名稱 `9max 模擬平台` 含中文字元，在該 code page 下無法編碼，MSI 打包會失敗
> （NSIS 不受影響）。改為 `zh-TW`／code page 950 後正常。
>
> 若日後把 `productName` 改為純 ASCII，這行設定就不再是必要的；
> 但只要產品名稱保有中文，就不能移除。

**前端如何分辨環境**：`apps/ui/src/api.ts` 偵測 `window.__TAURI__` 是否存在，
有就走 Tauri command，沒有就打 devserver 的 HTTP 端點。兩邊的 command 名稱與參數
形狀一致，因此切換只發生在該檔，其餘前端程式碼不需要知道自己跑在哪裡。

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

> 工作台只負責預覽，**正式的 687,492 格全表一律由 Rust 引擎展開**。

## 目前進度

`cargo test --workspace` 全綠，共 316 個測試。

| 里程碑 | 狀態 |
|---|---|
| M0 垂直切片 | 全鏈路可跑通；兩道硬閘門未過（見下） |
| M1 規則層 | 完成 |
| M2 策略與 Bot 層 | 翻前參數化 baseline 已接進執行層，翻後無內容一律 fallback |
| M3 桌面應用 UI | App shell 與面板 A／B／C／E／G 可用；D／F 未做 |
| M4 收尾與發佈 | 未開始 |

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
2. **內容體積產製管線**：精算後 baseline 為 **727,038 格**（`content_size` example），
   純人工填表約 76 人日，因此只能走參數化產生器。產生器已可跑（`generate_baseline`），
   但顧問尚未回填參數值

**其他未完成的 M0 凍結項**：多人 Equity spike、變異數削減 spike、258V 基準測試環境。
面板 A／C／E／G 的欄位級規格已補進 [`UI面板詳細規格.md`](UI面板詳細規格.md)。

**M2 待做**：翻後策略表（顧問內容）、7 組官方人格、多人 Equity、Analytics 與報表。

> **翻後目前一律走 fallback**（`checkFold/v0`）。顧問的翻後規則表還沒進來，
> 寫成別的樣子只會是我們自己編的內容，而編出來的內容會混進統計裡，
> 讓人以為那是校準過的結果。翻前走的是參數化 baseline，位置、籌碼 bucket
> 與情境都真的會影響分佈，因此面板 C 的參數調整看得到效果。
