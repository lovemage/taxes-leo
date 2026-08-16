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

## 目前進度

- **M1 規則層完成**：規則細則第九章的 R1–R23 驗收向量全數通過
- **M0 垂直切片完成**：引擎 → SQLite → IPC → React → 逐手重播全鏈路可跑通
- 尚未開始：策略與 Bot 層（M2）、完整八大面板（M3）、Tauri 打包（M4）
