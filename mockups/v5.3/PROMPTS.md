# GPT Image 2 出圖 Prompt 存檔｜UI Mockup v5.3（已作廢／僅供歷史留存）

> **2026-08-05：v6.0 已完全取消功能牌、技能牌、能力卡與 Roguelike 流程。**本文件與本目錄圖片不得再用於開發、報價或驗收；目前規格以根目錄《德州撲克策略模擬App規劃》v6.0 為準。

- 模型：`gpt-image-2`　尺寸：`portrait`（1024×1536）　品質：`high`
- 產製指令：`uv run <skill>/scripts/generate.py -p "<PROMPT>" -f <out.png> --size portrait --quality high`
- 對應規格：《德州撲克策略模擬 App 規劃》v5.3
- 舊版 mockup（`mockups/01~03`）依 v2 規格繪製（真人牌桌、模擬額度），**已作廢，不可再交付業主**。

## 共用風格前綴（STYLE）

所有畫面共用，確保四張風格一致。新增畫面時直接沿用這段。

```text
Premium mobile game UI mockup for a fictional Texas Hold'em app called "POKER LAB", rendered on a modern smartphone in portrait orientation, front-facing, centered on a neutral dark-gray studio background with a soft drop shadow. Art direction: dark poker-lounge aesthetic with deep forest-green felt (#0d2b22 and #12382b), warm gold accents (#d8b25f), off-white text, thin emerald hairline borders, matte texture, restrained and premium, no excessive glassmorphism, no neon. Crisp typography, precise spacing, exact icon alignment, production-quality app store mockup. ALL interface text must be Traditional Chinese, sharp and fully legible, correctly written, with no garbled characters, no Simplified Chinese, no pinyin, and no invented English words beyond the labels specified below.
```

---

## 01 · 首頁（兩個入口）

```text
<STYLE>
Compose the app HOME screen. Layout from top to bottom:
1. A slim top bar with the in-image text "POKER LAB" on the left and a small gold pill badge reading "VIP" on the right.
2. A greeting block with large white text "早安，Leo" and a smaller gray subtitle "今天想闖關，還是想練策略？".
3. A dashed-border resume card with the text "繼續上次的挑戰" and a second line "關卡模式・第 7 關・已持有 6 個能力".
4. A large primary button block with a gold border and faint gold fill, containing the bold gold title "關卡模式" and the smaller line "選一套打法，自動闖關｜第 1～12 關".
5. A second large button block with a muted green border, containing the title "經典模式" and the smaller line "快速對局｜策略實驗".
6. A row of three small outlined buttons reading "策略庫", "統計", and "設定".
The two large mode buttons must be the clear visual focus of the screen. Generous negative space, strong hierarchy, no bottom tab bar.
```

## 02 · 牌桌（直向 9 人桌）

```text
<STYLE>
Compose the POKER TABLE screen, the core gameplay screen. Layout:
1. Top status bar with the in-image text "第 7 關　·　場景 C" on the left and "能力 ×6" on the right.
2. The center is a VERTICAL oval poker table (tall ellipse) of deep green felt with a thin gold rim, occupying most of the screen.
3. Exactly 9 player seats distributed evenly around the oval edge: the human player seat at the BOTTOM CENTER with a gold-outlined round avatar and two face-up hole cards, and 8 opponent seats around the left, top, and right sides with small round avatars, each showing a chip-count label such as "32 BB", "51 BB", "17 BB", "63 BB", "28 BB", "44 BB", "39 BB". The player's own chip label reads "48 BB".
4. Small gold square position markers beside three seats reading "D", "SB", and "BB".
5. In the middle of the felt, five community card slots: three face-up cards showing "A♥", "K♠", "7♦" and two face-down cards with a patterned back; directly below them a gold pot label reading "底池　24 BB".
6. A bottom action bar of four buttons: "蓋牌", "過牌", "跟注 8", and a highlighted solid-gold primary button "加注".
7. A thin utility row under the action bar reading "暫停", "交給電腦", "能力 ×6".
Absolutely NO dealer character and no human figures — only seats, avatars, chips, cards, and the pot. The table must read clearly as portrait-oriented, single-hand operable.
```

## 03 · 關卡地圖

```text
<STYLE>
Compose the LEVEL MAP screen. Layout:
1. Top bar with the title "關卡地圖" and a right-side counter reading "第 7 關".
2. A vertical winding path of 12 numbered level nodes running from bottom (level 1) to top (level 12), like a roguelike progress map. Levels 1 to 6 are completed nodes rendered in solid gold with small check marks; level 7 is the current node, larger, glowing, with a label "進行中"; levels 8 to 12 are locked nodes in dim green with a small lock icon.
3. Nodes 4, 8, and 12 are BOSS nodes: larger, rendered in deep red with a crown-like emblem and a small label "魔王關".
4. The background behind the path changes subtly in four horizontal bands to indicate four scene themes, with small side labels reading "場景 A", "場景 B", "場景 C", "場景 D".
5. Above level 12, a locked gate graphic with the text "無限模式" and a smaller line "通關第 12 關後解鎖・最高第 99 關".
6. A bottom bar showing a horizontal row of six small ability-card icons with the label "目前持有能力 ×6".
Clear vertical progression, obvious difficulty ramp, no clutter.
```

## 04 · 能力三選一

```text
<STYLE>
Compose the ABILITY DRAFT screen shown after clearing a level. Layout:
1. A top title in large white text "選擇一個能力" with a smaller gray subtitle "三選一，選定後無法更換，效果持續到本次挑戰結束".
2. Three trading-card-style ability cards side by side, filling most of the screen, each with a distinct rarity frame color and a rarity ribbon at the top:
   - Card 1, gray frame, ribbon text "普通", card name "紅潮", a stylized crimson-wave illustration, a green line "＋ 紅色的牌更容易出現" and a red line "－ 黑色的牌變少".
   - Card 2, gold frame with a subtle glow, ribbon text "金卡", card name "讀心師", a stylized eye-and-mask illustration, a green line "＋ 每關可偷看對手底牌 3 次" and a red line "－ 每手 10% 機率被對手看穿".
   - Card 3, purple frame, ribbon text "特殊", card name "千里眼", a stylized all-seeing-eye and crystal illustration, a green line "＋ 每關 2 次預視下一張公共牌" and a red line "－ 使用當手下注上限為七成底池".
3. A dimmed bottom hint line reading "點選卡片以選定".
The middle gold card should read as the rarest. Card illustrations are painterly and iconic, and must contain NO text inside the artwork area — all wording sits in the typographic zones.
```

---

## 後續要出的素材（同一風格指南）

| 批次 | 內容 | 建議尺寸 |
|---|---|---|
| 能力卡插圖 | 30 張，卡框由程式套用，插圖內不得有文字 | square |
| 關卡場景 | 4 主題桌面＋背景，另 3 款 Boss 強化變體 | portrait |
| Bot 頭像 | 10～12 款同批生成，滿桌不重複 | square |
| 牌背花紋 | 1 款以上，預留換膚 | square |
| 商店素材 | Icon、Feature Graphic 草案 | square / landscape |
