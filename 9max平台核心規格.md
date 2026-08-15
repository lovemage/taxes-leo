# 9-max 平台核心規格

> 版本：v1.0  
> 日期：2026-08-15  
> 狀態：**現行權威規格（Normative）**

本文件是 9-max 德州撲克模擬平台的現行產品、牌局、策略、統計與效能基線。`過期決議/` 內的文件只保留歷史脈絡，**不得作為開發、測試或驗收依據**。若其他現行文件與本文件衝突，以本文件為準，並須同步修正文句，不得以口頭解釋帶過。

---

## 一、產品邊界與優先順序

### 1.1 產品形態

- 單機 Windows 桌面應用程式，牌局、Bot、策略、統計、log 與重播全部在本機執行。
- 一桌固定為 **1 名本機使用者 + 2～8 個 Bot**，共 3-max～9-max。
- 「3～9 人」只表示模擬桌型與座位數，**不代表真人多人連線**。
- 互動對打時由使用者在自己的座位手動選擇行動；Bot 座位由本機引擎決策。
- 批次模擬時使用者座位改由已選策略自動決策，不播放逐手動畫。

### 1.2 明確不做

- 不做真人多人連線、房間、配對、邀請、觀戰或聊天。
- 不做伺服器權威牌局、帳號、雲端同步或跨裝置同步。
- 不做真錢、代幣交易、儲值、兌現或其他金流玩法。
- 不做 2 人 heads-up；最低為 3-max。
- 手機、Web、雲端與真人連線均不是目前產品路線。若未來提出，視為新的產品決策與獨立範圍，不得用「已預留」擴張 v1。

### 1.3 交付優先順序

1. 牌局規則、隱藏資訊隔離、籌碼與統計正確。
2. 參數容易找到、理解、比較、修改、復原與驗證。
3. 統計資料能顯示定義、樣本數、不確定性與切片脈絡。
4. 操作期間 UI 保持順暢，批次運算不阻塞介面。
5. 牌局動畫保留並須清楚，但屬輔助理解，不得犧牲前四項，也不得阻擋操作或批次模擬。

---

## 二、牌桌與牌局規則

### 2.1 桌型設定

| 欄位 | 現行定義 |
|---|---|
| `players` | 3～9；恆為 1 名使用者，其餘為 Bot |
| `smallBlind` / `bigBlind` | 以最小籌碼單位保存的正整數；`SB < BB` |
| `stackBySeat` | 每座從 **20／50／100 BB** 三檔選一；預設全桌同檔，可逐座覆寫為不同檔。開始一手後不可改當手籌碼 |
| `stackPolicy` | 固定為 `resetEachHand`：每手開始時所有座位重置為 `stackBySeat`。不實作補碼與破產離桌，桌上人數在整個 run 中固定 |
| `ante.mode` | `none`／`perPlayer`／`bbAnte`／`btnAnte` |
| `ante.amount` | 最小籌碼單位的非負整數；`none` 時必須為 0 |
| `straddle.mode` | `none`／`single`／`double` |
| `straddle.seats` | 指定合法座位與順序；double 必須有兩個不同座位 |
| `straddle.amounts` | 每段為最小籌碼單位正整數；首段為 2×BB，後一段必須為前一段的 **2 倍**（現實規則） |
| `rake.pct` | 0～100% 之固定點數值 |
| `rake.cap` | 每手總抽水上限，以最小籌碼單位或 BB 顯示、整數保存 |
| `rake.noFlopNoDrop` | **布林值**；開啟時，該手未發出 flop 即不抽水 |

Rake 以整手可抽水底池計算，先依比例向下取整至最小籌碼單位，再套用每手 cap；main／side pot 的分配不得重複扣除 rake。任何 rake 規則變體若不符合這個模型，必須新增具名 enum，不得以「三段」等未定義文字代替。

### 2.2 位置與行動順序

- 3-max 為 BTN、SB、BB 三個獨立位置。BTN 翻前第一個行動、翻後最後行動。
- 4～9 人依按鈕順時針映射座位；每手結束後按鈕移至下一個仍在桌座位。
- Straddle、double straddle、ante、all-in、短額加注是否重新開放行動及最小加注額，必須由引擎的合法行動產生器統一決定；規則內容依 [`德州撲克規則細則.md`](德州撲克規則細則.md) 第一～三章，完全比照現實德州撲克規則。
- UI 不得自行推導可下注金額、最小加注額、底池或 side pot。

### 2.3 籌碼、side pot 與 odd chip

- 所有牌局金額使用整數最小籌碼單位或等價 fixed-point；規則層不得使用浮點籌碼。
- 每手必須滿足：開始籌碼總額 = 結束籌碼總額 + 該手 rake，誤差為 0。
- Main pot 與每個 side pot 都保存參與資格；folded 玩家已投入籌碼留在底池，但不得取得分配。
- Split pot 先按份額整除，odd chip 依按鈕左側起、在該 pot 有資格的最近座位順時針分配。每個 odd chip 的去向必須寫入事件 log。

### 2.4 隱藏資訊與使用者可見範圍

- 引擎內部的完整狀態與決策提供者取得的資訊必須使用不同型別。
- `DecisionView` 只包含該座位可見的底牌、公共牌、公開行動、公開籌碼、位置、底池與策略所需的推導特徵；不得包含牌堆順序、其他未亮底牌或未公開結果。
- `StrategyProvider` 的唯一輸入是 `DecisionView`、自身 range 與由公開資訊推導的對手 range estimates，不得接收完整 `GameState`。
- 測試必須證明：只改變不可見牌、保持資訊集相同時，Bot 在相同 seed 下產生相同 action distribution。
- 互動牌桌只顯示規則允許公開的牌；重播是否顯示未攤牌底牌採明確設定，預設不顯示。即使 log 保存完整牌局，Bot 決策路徑仍不得存取。
- 攤牌後的公開範圍依現實規則判定：輸家可 muck，只有實際亮出的底牌才是公開資訊。muck 政策為具名設定（`realistic` 為預設／`alwaysShow`），寫入 `RunManifest`；不同設定的統計結果不得混用比較。詳見 [`德州撲克規則細則.md`](德州撲克規則細則.md) 4.2。

---

## 三、執行模式與可重現性

### 3.1 互動對打

- 使用者手動選擇 Fold／Check／Call／Bet／Raise／All-in；UI 只呈現引擎回傳的合法行動與金額區間。
- Bot 回應期間 UI 必須保持可操作並顯示明確狀態。
- 動畫可選 `off`／`fast`／`normal`；動畫不得延遲引擎狀態提交，也不得成為重播正確性的資料來源。

### 3.2 批次模擬

- 使用者座位由所選策略自動決策；不播放牌桌動畫。
- 支援暫停、續跑與取消；暫停／續跑不可改變最終結果。
- 使用者可在運算中瀏覽既有報表與 log；寫入須採背壓或批次交易，避免 UI 餓死。

### 3.3 RunManifest 與重播

每次互動或批次執行都建立不可變的 `RunManifest`，至少包含：

- engine、schema、log format、RNG 演算法與版本；
- master seed、每手 stream 派生規則、執行模式；
- 完整桌型與每座初始籌碼快照；
- 使用者策略、Bot persona、level、逐座覆寫的完整快照與內容 hash；
- 基準策略與內容包版本；
- 建立時間、完成狀態、checkpoint 版本。

重播以已保存的牌局事件為主，不重新執行策略決策。若為節省容量而重算衍生數值，必須能由同一 `RunManifest` 與仍受支援的版本完全重建；只有 hash 而沒有內容快照不合格。

### 3.4 RNG

- RNG 演算法與版本固定並寫入 manifest；升級 RNG 視為格式版本變更。
- 每手使用由 master seed、hand index 與用途 domain 派生的獨立 stream；發牌、策略混頻與 Monte Carlo Equity 不得共用同一 stream。
- 平行執行的結果合併順序固定；相同 manifest 重跑須產生逐事件一致結果。

---

## 四、策略與 Bot 決策規格

### 4.1 Preflop 節點

Preflop 不是「每格只有一個頻率」。資料模型必須是：

`node × handClass × action × size → frequency`

`node` 至少包含桌型人數、使用者位置、有效籌碼 bucket、ante、rake、straddle 與完整公開翻前 action history。每個 hand class 在移除不合法行動後，Fold／Call／Raise(size)／All-in 等頻率合計必須為 100%。UI 可用 action tab、分色堆疊或切換圖層呈現，但不得把 call 與 3-bet 合併成無法解讀的單一頻率。

v1 UI 編輯至 169 hand classes；combo 權重由 schema 與引擎支援，combo 級 UI 下鑽不在 v1。

### 4.2 Postflop 節點

Postflop 規則至少包含：街別、board features、hand/draw bucket、存活人數、英雄相對行動順位、各對手相對位置、pot type、當前街與前街公開 action history、面對尺度、英雄與相關對手有效籌碼、SPR、合法行動與尺度。

- 多人底池不得只用單一 `IP/OOP` 表達位置。
- 規則採第一條命中時，編輯器必須偵測重疊、被遮蔽與永遠不會命中的規則。
- 不合法行動先 mask，再正規化；若剩餘權重為 0，必須進入 fallback，不得除以 0 或任選行動。
- Fallback 的版本與命中次數寫入 log，並在報表顯示策略覆蓋率。

### 4.3 Bot 三層修正順序

Bot 決策管線固定如下：

1. 基準策略產生合法節點的 action logits／weights。
2. Persona 只產生具名、受上下限約束的偏移。
3. Level 套用可用尺度、規則覆蓋、規劃深度、對手模型與誤差模型。
4. 逐座覆寫套用在對應欄位；不得直接注入未登錄參數。
5. 引擎套用 legal-action mask 與 exploit adjustment cap。
6. 若啟用 decision noise，以具名公式混合合法分佈，再正規化。
7. 使用該決策專屬 RNG stream 取樣最終行動。

每個參數必須在 generated schema 中具有型別、單位、合法範圍、預設值、顯示名稱、說明與可覆寫層級。實際偏移公式、套用前後值及最終分佈均寫入決策 trace，供 UI 解釋。

---

## 五、Equity、EV 與統計規格

### 5.1 Equity 計算模式

- Complete-hand evaluator 必須精確。
- Range Equity 使用混合計算：狀態空間低於門檻時精確枚舉；多人或大型 range 節點使用可重現的分層 Monte Carlo。
- M0 必須在 3／6／9-max、preflop／flop／turn／river 代表節點量測時間與誤差後，凍結 exact/sampling 門檻、每種模式樣本預算及 cache key。
- 取樣模式必須顯示 Monte Carlo 樣本數與計算誤差；不得把取樣值標示為「精算」。
- 聯合範圍必須套用 reach weight、card removal、活躍玩家與各 pot eligibility；多人平手按實際並列人數分配。

### 5.2 All-in EV

每個 main／side pot 分段計算：

`expectedNet = Σ(expectedShareOfEligiblePotAfterRake) - totalContribution`

只有對該段有資格的玩家參與該段 Equity；已 fold 玩家不參與 Equity，但其投入保留在 pot。實際結果與 All-in adjusted 結果分欄保存，禁止混用。

### 5.3 指標與區間

| 指標／情境 | 現行方法 |
|---|---|
| 獨立批次的 bb/100／EV | 依手序相關性使用 batch means 或 cluster bootstrap |
| CRN／duplicate 策略比較 | 對配對差值估計 CI；不得用兩次獨立 CI 相減 |
| Win Rate、VPIP、PFR 等比例 | 顯示分子／分母；獨立近似用 Wilson interval，有 cluster 時對 cluster bootstrap |
| 精確枚舉 Equity | 標示「模型內精確」，不顯示虛構 sampling CI |
| Monte Carlo Equity | 顯示樣本數與 Monte Carlo CI／誤差上限 |
| 169 格與大量情境切片 | 預設標示探索性；若產生「漏洞」旗標，使用 BH-FDR 或預先登錄的多重比較控制 |

CI 跨 0 只能標示「本樣本無法判定優劣」，不得直接等同「樣本不足」。樣本不足由預先定義的最低有效樣本、CI 寬度或 Monte Carlo 誤差門檻判定。

### 5.4 呈現原則

- 所有 EV／bb100 顯示樣本數、估計方法與 95% CI。
- 所有比例顯示分子／分母與區間；分母為 0 時顯示 N/A。
- 報表不得宣稱 GTO 正解或以單一 exploitability 數字包裝規則式 Bot。
- 「最虧三類情境」必須同時顯示樣本數、區間與探索性標籤，不得只依點估計排序後斷言漏洞。

---

## 六、UI／UX 呈現原則

### 6.1 參數優先

- 首層顯示常用參數與目前生效值；進階層顯示繼承來源、預設值、覆寫值與合法範圍。
- 支援搜尋、分類、重設單欄、重設群組、複製座位、比較變更、未儲存提示與 undo／redo。
- 任何修改先在前端依 generated schema 即時驗證，提交後仍由引擎再次驗證；兩端規則來源相同。
- Persona／Level／座位覆寫必須用「預設 → 修正 → 最終值」呈現，不得只顯示最後數字。

### 6.2 統計優先

- 儀表板先顯示結論是否可判定，再顯示點估計；樣本、CI 與 estimator 不得藏在 tooltip 才能看到。
- 任何切片都保留目前桌型、策略版本、Bot 組合、時間範圍與有效樣本脈絡。
- 熱力圖同時提供色階圖例、數值、樣本門檻與色盲可辨識方案；不能只靠紅綠顏色。

### 6.3 動畫為輔助層

- 動畫只負責解釋發牌、下注、收池、showdown 與重播順序。
- 動畫可關閉或加速；報表、參數編輯及批次模擬不依賴動畫。
- 動畫資源與效果延後於參數面板、統計呈現及可用性驗收，不得反向壓縮核心工期。

---

## 七、效能驗收基準

### 7.1 驗收硬體上限

- 參考機：**Intel Core Ultra 7 258V 等級筆電 CPU、32GB RAM、內顯**。
- 正式效能驗收只能使用與上述相同或更低效能的 CPU，RAM 不得高於 32GB；不得以桌機高功耗模式或獨立顯卡結果代替。
- 每次報告記錄 Windows 版本、CPU、RAM、電源模式、engine build、資料庫大小與測試設定。

### 7.2 UX 硬門檻

| 情境 | 驗收門檻 |
|---|---|
| 參數輸入、切換、即時驗證 | p95 ≤ 100 ms |
| 面板切換、篩選已載入資料 | p95 ≤ 250 ms |
| 聚合圖表更新 | p95 ≤ 500 ms；超過時須顯示 loading／progress |
| 100 萬手 DB 載入指定手牌 | p95 ≤ 200 ms，冷啟動與 warm cache 分開記錄 |
| 互動模式 Bot 回應 | p95 ≤ 1 秒，單次不得超過 3 秒；超過 250 ms 顯示思考狀態 |
| 暫停／取消 | 暫停確認 ≤ 1 秒；取消確認 ≤ 2 秒 |
| 動畫 | 目標 60fps；驗收 p95 frame time ≤ 33 ms，且不得阻塞操作 |
| 記憶體 | 100 萬手批次與報表並行時 working set ≤ 12GB，且無持續成長 |

### 7.3 批次基準

在 M0 凍結的代表設定與預設 Equity sampling budget 下：

- 10 萬手須在 60 分鐘內完成；100 萬手須在 12 小時內完成。
- DB（含 index，不含可清除 WAL 暫存）100 萬手 ≤ 2GB。
- 批次運算期間仍須通過第 7.2 節的參數操作、瀏覽與取消門檻。
- 若未達門檻，優先調整演算法、sampling budget、寫入格式或可選手數；**不得改用高於參考機的硬體通過驗收**。

---

## 八、最低驗收矩陣

- 桌型：3／4／6／9-max。
- 籌碼：統一深度與逐座不等深度；至少三人 all-in 的多層 side pot。**全桌同深度時 side pot 不會形成**，因此多層 side pot 與 odd chip 的驗收必須使用逐座不同檔位（見 [`德州撲克規則細則.md`](德州撲克規則細則.md) 8.3）。
- Forced bets：四種 ante 模式、無／單／double straddle、rake 開關、cap 與 no-flop-no-drop。
- 規則：fold、check、call、完整加注、短額 all-in、split pot、odd chip、showdown；[`德州撲克規則細則.md`](德州撲克規則細則.md) 第九章的測試向量 R1–R14 全數通過。
- 資訊隔離：每種街別驗證 DecisionView 不含未公開牌。
- 可重現：完成、暫停續跑、不同平行度三條路徑逐事件一致。
- 統計：用可人工計算的合成資料驗證每種 estimator、CI、比例分母、FDR 與 All-in EV。
- 儲存：schema migration、崩潰復原、2GB DB 搜尋、匯出與清理。
- UX：第七章全部門檻在不高於 258V／32GB 的筆電上通過。

