# UI 參數與統計呈現詳細規格

> 版本：v1.1　｜　日期：2026-08-15
> 上層文件：[`9max模擬平台實做計劃.md`](9max模擬平台實做計劃.md) 第五章
> 權威規格：[`9max平台核心規格.md`](9max平台核心規格.md)

本文件專注本產品最重要的兩個 UI 面向：**參數如何呈現**與**統計數據如何呈現**。牌桌動畫保留，但屬輔助理解，不得壓縮本文件範圍或阻塞操作。`過期決議/` 內文件不具規範力。

---

## 全域 UX 原則

### G.1 資訊架構

- 主導覽順序：牌桌設定 → 座位與 Bot → 自身策略 → 執行 → 報表／儀表板 → Log／重播。
- 參數與統計頁可以直接互相跳轉；從報表點擊情境時，策略編輯器應開啟相同桌型、位置與節點。
- 桌面寬度不足時優先保留參數值、單位、樣本數與 CI；裝飾圖、動畫與次要敘述後收合。

### G.2 參數呈現

- 每個參數顯示名稱、目前生效值、單位、合法範圍、簡短說明及來源層級。
- Bot 參數顯示「官方預設 → Persona 修正 → Level 修正 → 逐座覆寫 → 最終生效值」。
- 支援參數搜尋、分類、只看已覆寫、重設單欄、重設群組、複製座位、變更前後比較及 undo／redo。
- 未儲存修改固定顯示；切換頁面、策略或桌型前必須讓使用者儲存、放棄或返回。
- 即時驗證使用 Rust schema 產生的相同規則；前端通過不代表引擎可省略驗證。

### G.3 動畫層級

- 牌桌動畫只有 `off`／`fast`／`normal` 三種模式。
- 動畫只呈現發牌、下注、收池、showdown 與重播時間序；不得自己改變底池、籌碼或行動狀態。
- 批次模擬不播放動畫；參數編輯及報表頁不載入非必要動畫資源。

---

## 面板 D — 自身策略編輯器

### D.1 總覽與版面

- 定位：本產品**資訊密度最高**的面板，編輯「自身座位」的策略。
- 版面：左側「情境導航樹」（位置 → 情境 → 街別），右側「編輯區」（範圍矩陣／規則列表／尺度樹）。
- 預設收合（依核心規格第六章漸進揭露）；常用值固定可見，進階使用者再展開規則細節。
- 所有修改即時驗證，非法標紅並給修復提示；儲存前顯示「策略完整度」。

### D.2 策略 Meta（`Strategy.meta`）

| 欄位 | 型別 | 說明 |
|---|---|---|
| id | string | 唯一識別 |
| name | string | 顯示名稱 |
| author | string | 作者 |
| version | string | 版本（每次儲存自動遞增） |
| source | enum | official（唯讀）／ custom（複製後可編輯） |
| baselineVersion | string | 所基於的官方基準版本（如「基準 v1.2」） |
| createdAt / updatedAt | datetime | 建立／更新時間 |

### D.3 桌型適用範圍（`Strategy.table`）

| 欄位 | 型別 | 說明 |
|---|---|---|
| players | 3–9 | 座位數 |
| blind | struct | SB／BB；顯示可用 bb 正規化，保存為最小籌碼整數 |
| stackBySeatBb | map[seat, 20\|50\|100] | 每座初始籌碼深度，三檔擇一；每手開始重置為此值。策略 applicability 依英雄與相關對手有效籌碼 bucket 判定 |
| ante | enum | none／perPlayer（逐人）／bbAnte（BB ante）／btnAnte（BTN ante）＋ 金額 |
| straddle | struct | none／single／double ＋座位與金額 |
| rake | struct | pct ＋每手 cap ＋ `noFlopNoDrop: boolean` |

> 策略檔綁定桌型；桌型不合時提示「此策略非為本桌型校準」。

### D.4 Preflop 範圍矩陣

**位置集合（依座位數動態映射）**：

| 座位數 | 位置集合 |
|---|---|
| 9 | UTG／UTG+1／UTG+2／LJ／HJ／CO／BTN／SB／BB |
| 8 | UTG／UTG+1／LJ／HJ／CO／BTN／SB／BB |
| 7 | UTG／LJ／HJ／CO／BTN／SB／BB |
| 6 | LJ／HJ／CO／BTN／SB／BB |
| 5 | HJ／CO／BTN／SB／BB |
| 4 | CO／BTN／SB／BB |
| 3 | BTN／SB／BB（三個獨立位置；BTN 翻前先動、翻後最後動） |

（位置命名以牌手顧問最終定案為準；此表是引擎位置映射在 UI 上的呈現依據。）

**節點情境清單**：

| 情境 | 節點可用行動範例 |
|---|---|
| Unopened | Fold／Limp／Raise(size)／All-in |
| vs Limp | Fold／Overlimp／Isolation Raise(size)／All-in |
| vs Open | Fold／Call／3-bet(size)／All-in |
| vs 3-bet | Fold／Call／4-bet(size)／All-in |
| vs 4-bet | Fold／Call／5-bet(size)／All-in |
| vs Squeeze | Fold／Call／Raise(size)／All-in |
| Short Stack | Fold／Limp／Call／Raise(size)／All-in，依合法行動顯示 |

每個節點由桌型人數、位置、有效籌碼 bucket、ante、rake、straddle 與完整公開翻前 action history 識別；不得只用「vs Open」文字合併不同加注尺度與前序行動。

**169 格編碼**：13×13 網格；對角線 = 對子（AA…22，13 格），上三角 = 同花（AKs…32s，78 格），下三角 = 非同花（AKo…32o，78 格）；合計 169。每格保存的是**各合法行動的頻率分佈**，不是單一「進池頻率」。

**頻率編輯**：
- Action tabs 顯示 Fold／Call／Raise 尺度／All-in；矩陣以分色堆疊顯示同一手牌的混合頻率。
- 點格可直接輸入各 action 的 0–100%；同一手牌在 legal-action mask 後合計必須為 100%。
- 支援選區批次填入、複製 action layer、按比例縮放與鎖定某 action 後重新分配其餘頻率。
- 色階不得只靠紅綠；格內可切換顯示主要 action、完整分佈或數值。
- **v1 範圍**：UI 僅提供 169 格 × action 編輯；combo 權重由引擎與 schema 支援，UI 下鑽列後續。

### D.5 Postflop 規則清單

- 依優先序排列，**第一條命中即生效**。
- 每條規則欄位：

| 欄位 | 型別 | 說明 |
|---|---|---|
| street | enum | flop／turn／river |
| boardTexture | enum | 乾／濕／成對／單色／三同花／順子面／高張… |
| handStrength | enum | value／bluff／bluffcatcher／draw／其他分桶 |
| activePlayers | integer／range | 本街仍在牌局的人數 |
| actionOrder | struct | 英雄行動順位及各存活對手的相對位置；多人底池不得只用 IP／OOP |
| potType | enum | SRP／3-bet Pot／4-bet Pot |
| currentStreetAction | struct | 當前街截至此節點的公開行動序列 |
| prevAction | struct | 前街公開行動序列 |
| facingSize | enum | 面對下注尺度級距（1/4、1/3、1/2、2/3、3/4、1x、overbet…） |
| spr | range | SPR 區間 |
| effectiveStacks | struct | 英雄與相關存活對手的有效籌碼 bucket |
| actions | freq[] | Fold／Check／Call／Bet(sz)／Raise(sz)／All-in 各頻率 |

- 行動頻率合計 = 100%；不合法行動先 mask 再正規化。若合法行動剩餘權重為 0，直接進入 fallback。
- 規則編輯器即時標示條件重疊、被更高優先規則完全遮蔽及不可達規則；使用者必須處理 error，warning 可保留但寫入驗證摘要。

### D.6 下注尺度樹（`betSizeTree`）

- 預設尺度：25%／33%／50%／66%／75%／100%／125%／150%／Overbet（150–300%）／All-in。
- 每街可增刪尺度、設定可用的情境。
- 尺度以 %Pot 或 bb 表示，可切換顯示。

### D.7 Fallback

- 未命中任何規則，或合法行動 mask 後剩餘權重為 0 的節點，由具版本的 fallback 基準策略補足。
- fallback 命中次數列入報告；策略完整度 = 命中玩家規則的節點數 ÷ 總決策節點數，翻前／翻後及各切片分開統計。

### D.8 驗證與策略完整度

| 檢查 | 規則 |
|---|---|
| 頻率合計 | 每節點行動頻率 = 100% |
| 合法行動 | 不合法行動移除後正規化 |
| 參數區間 | 所有參數在合法範圍 |
| 完整度 | 儲存前顯示「你的策略 翻前 X%／翻後 Y% ＋ 基準 vZ 補足」 |
| Preflop action 維度 | 每個 hand class 的合法 action 頻率合計 100% |
| 規則可達性 | 不得存在完全不可達或資料不完整的規則；遮蔽規則需明確 warning |

---

## 面板 H — 本機方數據儀表板

### H.1 定位

- 呈現「自身座位」（本機方）的表現：勝率、EV、頻率，供策略診斷。
- 資料來源：本機引擎的逐手輸出聚合 ＋ Equity engine；小型狀態可精確枚舉，大型多人狀態採 deterministic sampling。**不拿逐手盈虧直接下結論，一律揭露樣本數、計算模式與不確定性**。

### H.2 指標定義（含公式）

| 指標 | 定義 | 公式 | 單位 |
|---|---|---|---|
| Equity（多人攤牌勝率） | 對該 pot 有資格對手的聯合範圍，套用 reach weight、card removal 與多人 runout；小狀態 exact、大狀態 sampling | Σ P(聯合組合, runout)·share(h)；多人平手 share=1/k | % |
| EV（期望值） | 每手平均淨贏 | EV = (1/N)·Σ Δstack | bb／手 |
| bb/100 | 每百手期望值 | bb/100 = EV × 100 | bb |
| 95% CI | 信賴區間 | [EV − 1.96·SE, EV + 1.96·SE]（naive SE = σ̂/√N；正式依 estimator 選定，見 H.3） | bb |
| All-in EV | 以 all-in 時的多人 equity 結算替代實際發牌，依 main／side pot 分段計算 | 非 all-in 街實際收付 ＋ Σ段（該段有資格的玩家中，自己的 equity × 該段底池），扣除 rake | bb |
| Win Rate（每手勝率） | 獨贏手數 ÷ 總手數；tie 另列，不混入 wins | wins ÷ N，並顯示 wins／ties／N | % |
| Showdown Win（攤牌勝率） | 攤牌獨贏手數 ÷ 進入攤牌手數；tie 另列 | showdownWins ÷ showdowns | % |
| σ（標準差） | 每手結果樣本標準差 | √(Σ(Δ−EV)² ÷ (N−1)) | bb |
| σ₁₀₀ | 每百手標準差 | σ × 10 | bb |
| Max Drawdown | 累計盈虧曲線最大峰谷差 | max(peak − trough) | bb |
| VPIP | 自願投入籌碼進池手數 ÷ 總手數 | — | % |
| PFR | 翻前加注手數 ÷ 總手數 | — | % |
| 3-bet | 翻前再加注次數 ÷ 面對加注機會次數 | — | % |
| C-bet | 翻牌圈下注次數 ÷ 翻牌圈作為翻前攻擊者可下注次數 | — | % |
| Fold to C-bet | 面對 C-bet 棄牌次數 ÷ 面對 C-bet 次數 | — | % |
| WTSD | 進入攤牌手數 ÷ 看到翻牌手數 | — | % |
| W$SD | 攤牌贏手數 ÷ 進入攤牌手數 | — | % |

> **多人正確性**：Equity 與 All-in EV 必須涵蓋聯合範圍、reach weight、card removal、多人 runout、1/k 平分、main／side pot eligibility、rake 與總投入。Complete-hand evaluator 精確；Range Equity 是否 exact 依凍結門檻決定，不得要求 9-max 全節點無條件枚舉。

### H.3 算法對照（指標 → 引擎輸出）

| 指標 | 引擎需輸出的原始資料 | 計算層 |
|---|---|---|
| Equity | 聯合範圍、計算模式（exact/sampling）、reach/card removal、有效樣本、誤差與多人 share | 引擎（Equity engine） |
| EV／bb100／σ／Max Drawdown | 每手 Δstack 序列 | 統計層聚合 |
| All-in EV | all-in 節點：**分段（main／side pot）** equity、各段底池、已投入籌碼、rake | 引擎記錄 ＋ 統計層加總 |
| Win Rate／Showdown Win | 每手 win/lose/tie 與是否攤牌 | 統計層計數 |
| 95% CI | estimator id、cluster／pair id、樣本均值與所需統計量 | 統計層 |
| 行為頻率（VPIP…） | 每個決策節點的 action ＋ context 計數 | 統計層計數 |

> **工程驗收依據**：引擎穩定輸出 RunManifest、每手 Δstack、每節點多人 range/equity 計算描述、每決策 action＋context、all-in 分段結算及 pair/cluster id。UI 只呈現引擎與統計層結果，不重算牌局邏輯。

### H.3.1 Estimator 決策表

| 資料／比較 | 顯示方法 |
|---|---|
| 一般 EV／bb100 | batch means 或 cluster bootstrap，顯示 estimator 名稱 |
| CRN／duplicate A/B | 對配對差值做 CI，不顯示兩次獨立區間相減 |
| Win Rate／VPIP／PFR 等比例 | 分子／分母＋Wilson interval；有 cluster 時改用 cluster bootstrap |
| Exact Equity | 標示「模型內精確」，不製造 sampling CI |
| Sampled Equity | 顯示 Monte Carlo N 與 CI／誤差上限 |
| 169 格／大量切片 | 預設「探索性」；漏洞旗標使用 BH-FDR 或預先登錄控制 |

### H.4 版面與視覺（GTO Wizard 式）

- **整體卡**：先顯示「可判定／無法判定／樣本不足」，再顯示勝率的分子／分母與區間、EV／bb100＋CI、estimator、All-in EV、σ、最大回撤及樣本數。
- **逐位置表**：列 = 位置，欄 = 勝率／EV／bb/100／VPIP／PFR。
- **13×13 EV 熱力圖**：每格可切換 EV／勝率／使用頻率；顯示值、樣本數、CI 狀態與探索性標示；提供色階圖例及非紅綠的色盲可辨識方案。
- **逐街分解**：preflop／flop／turn／river 的 EV 與頻率長條。
- **對照**：自身 vs 全桌 Bot 平均的 EV 差距與剝削幅度。

### H.5 樣本不足與誠實揭露

| 規則 | 內容 |
|---|---|
| 灰階標示 | 樣本低於閾值的手牌格灰階 ＋ 「樣本不足」 |
| CI／區間一律顯示 | EV／bb100 顯示 95% CI；比例顯示分子／分母與適用區間；sampled Equity 顯示 MC 誤差 |
| 跨 0 標示 | CI 跨 0 → 「本樣本無法判定優劣」 |
| 可分辨差距 | 顯示「此手數可分辨的最小差距」 |
| 不假精確 | 對外最多一位小數；內部保留完整精度 |

---

## UX 效能驗收

所有正式數字只接受在 **Intel Core Ultra 7 258V／32GB 或更低規格筆電**測得的結果；不得使用更高規格替代。門檻沿用 [`9max平台核心規格.md`](9max平台核心規格.md) 第七章，特別包含：

- 參數輸入與即時驗證 p95 ≤100ms；面板與已載入篩選 p95 ≤250ms。
- 聚合圖表更新 p95 ≤500ms，較久時顯示 loading／progress。
- 100 萬手 DB 載入指定手牌 p95 ≤200ms。
- 互動 Bot 回應 p95 ≤1 秒；動畫 p95 frame time ≤33ms，且可關閉。
- 批次運算期間仍須通過參數、報表、暫停與取消門檻。
