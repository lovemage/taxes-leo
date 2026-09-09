//! 牌力分類器（0907 計劃第三章）。
//!
//! 把「英雄這一手在這個牌面上有多強」歸到八個組別之一。規則編輯器用它
//! 當節點鍵的一個維度，Bot 決策用它查規則，UI 用它解釋「為什麼被分到
//! 這一組」。
//!
//! # 三個不能妥協的性質
//!
//! **確定性。** 相同底牌、公共牌與公開狀態不得因隨機取樣得到不同牌力組。
//! 因此百分位不沿用 Bot 的 Monte Carlo equity，改為列舉所有合法對手兩張
//! 底牌並精確比較。這是核心規格 7.3 的明示局部例外，只適用於本模組。
//!
//! **分組相對於牌面。** 同樣是兩對，在乾燥彩虹面可能是強成牌，在四順或
//! 公對面可能只是中等成牌。分類器因此看的是百分位與補牌，不是成牌類別
//! 本身。
//!
//! **對手模型寫在臉上。** 百分位對的是**均勻隨機的合法組合**，不是「會
//! 延續到本街的範圍」。同一手頂對在 c-bet 池與 3-bet 池對抗的範圍差很多，
//! 而分類器看不到那件事。欄位名稱與設定值都明講這一點，避免顧問拿「對手
//! 續打範圍」的直覺去校準門檻。

use crate::card::{Card, Rank, Suit};
use crate::eval::{board_partial_rank, evaluate, key_rank, Category};
use crate::hand::Street;
use crate::strategy::postflop::HandStrength;

/// 萬分比的滿值。
const FULL_MYRIAD: u32 = 10_000;

/// 分類門檻與折算係數。
///
/// **全部集中在這裡，不散落成 magic number。** 這些是未簽核的工程值，
/// 由 Stage 1 的對照表產生校準起點，最終須由牌手顧問確認；
/// [`Self::consultant_approved`] 在那之前恆為 false。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandStrengthConfig {
    /// `nuts` 的百分位下限
    pub nuts_floor_myriad: u32,
    /// `strongMade` 的百分位下限
    pub strong_made_floor_myriad: u32,
    /// `mediumMade` 的百分位下限
    pub medium_made_floor_myriad: u32,
    /// 「有攤牌價值」與 `bluffCatcher` 的**同一個**下限。
    ///
    /// 同一個門檻寫兩次必然漂移，因此兩處引用同一個常數
    pub showdown_value_floor_myriad: u32,
    /// 強聽牌所需的有效補牌（百分之一張）
    pub strong_draw_floor_centi: u16,
    /// 高張成對補牌的折算係數（百分之一張）
    pub overcard_out_weight_centi: u16,
    /// 污染補牌的折算係數（百分之一張）
    pub polluted_out_weight_centi: u16,
    /// 是否經牌手顧問簽核
    pub consultant_approved: bool,
}

impl HandStrengthConfig {
    /// 未簽核的工程預設。
    ///
    /// 計劃 §3.2 的起點是 95%／80%／55%／35%，但那是拿「對手續打範圍」
    /// 的直覺寫的。實測對照表（`cargo run --release --example
    /// hand_strength_table`）顯示，對**均勻隨機**的合法對手底牌：
    ///
    /// | 牌型 | 實測百分位 |
    /// |---|---:|
    /// | 四條／葫蘆／堅果同花／頂三條 | 9_990～10_000 |
    /// | 三條 | 9_972 |
    /// | 高兩對 | 9_935 |
    /// | 頂對頂踢腳 | 9_713 |
    /// | 超對 | 9_551 |
    /// | 頂對弱踢腳 | 9_121 |
    /// | 口袋對（中）／第二對 | 8_112～8_330 |
    /// | 底對／弱口袋對 | 6_110～6_382 |
    /// | A 高／K 高 | 4_259～5_407 |
    /// | 同花聽牌／兩頭順 | 1_151～2_631 |
    ///
    /// 頂對對隨機底牌就有 91%，用 80% 當強成牌下限的話，第二對與口袋對
    /// 也會落進強成牌。門檻因此依實測重設；仍是**未簽核**的工程值。
    pub const ENGINEERING: Self = Self {
        nuts_floor_myriad: 9_980,
        strong_made_floor_myriad: 9_400,
        medium_made_floor_myriad: 7_500,
        showdown_value_floor_myriad: 4_000,
        strong_draw_floor_centi: 800,
        overcard_out_weight_centi: 50,
        polluted_out_weight_centi: 0,
        consultant_approved: false,
    };

    /// 對手模型。寫成字串是為了讓它跟著 trace 與對照表一起出現——
    /// 顧問看到「uniformRandom」才不會拿續打範圍的直覺去校準門檻。
    pub const OPPONENT_MODEL: &'static str = "uniformRandom";
}

impl Default for HandStrengthConfig {
    fn default() -> Self {
        Self::ENGINEERING
    }
}

/// 一次分類的完整結果。
///
/// 除了組別本身，還帶著判定用到的每一個中間量：UI 要解釋「為什麼被分到
/// 這一組」，trace 要能逐手重播，測試要能鎖住分類是否穩定。
///
/// 百分位、補牌數這些是**指定底牌的預覽／trace 資料**，不是牌力組規則
/// 節點的可編輯欄位——節點編輯器只顯示牌力組與條件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandStrengthSnapshot {
    pub group: HandStrength,
    /// 英雄目前的成牌類別
    pub made_category: Category,
    /// 英雄的底牌是否確實改善公共牌（踢腳不算）
    pub improves_board: bool,
    /// 公共牌自身的最佳部分牌型，供 UI 解釋「為什麼不算改善」
    pub board_partial_category: Category,
    /// 對「一個均勻隨機的合法對手底牌」的目前牌力百分位
    pub random_opponent_percentile_myriad: u32,
    pub has_showdown_value: bool,
    /// 去重後的實際補牌張數
    pub raw_outs: u8,
    /// 折算後的有效補牌（百分之一張）。**資訊欄位**，含成牌的漸進改善
    pub effective_outs_centi: u16,
    /// 其中能成同花或順子的部分（百分之一張）。
    ///
    /// 判定樹的聽牌條件看的是**這個**，不是 `effective_outs_centi`。
    /// 對照表實測顯示，把「一對改善為兩對／三條」的 11 張補牌算進聽牌
    /// 門檻之後，底對會被判成 `madePlusDraw`——那手根本沒有在聽什麼牌。
    /// 「聽牌」講的是還沒到手的大牌，成牌的漸進改善是另一回事
    pub draw_outs_centi: u16,
    pub nut_draw: bool,
    pub combo_draw: bool,
    pub backdoor_draw: bool,
}

impl HandStrengthSnapshot {
    /// 是否為「成牌」。
    ///
    /// **唯一定義**：成牌類別至少一對，且底牌確實改善了公共牌。只靠公共牌
    /// 成立的牌型不算——否則公對面上握 `A2` 的手也有「一對」，會被判進
    /// `mediumMade`，整個牌力組被公共牌灌水。
    #[must_use]
    pub fn is_made_hand(&self) -> bool {
        self.made_category >= Category::Pair && self.improves_board
    }
}

/// 一張補牌的折算權重（百分之一張）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutWeight {
    centi: u16,
    /// 補到之後是否形成同花或順子（供 `nut_draw` 與複合聽牌判定）
    makes_flush: bool,
    makes_straight: bool,
    nut: bool,
}

/// 分類指定底牌在指定公共牌上的牌力組。
///
/// `street` 由公共牌張數推導，因此不另外收參數；3 張是翻牌、4 張轉牌、
/// 5 張河牌。
///
/// # Panics
/// 公共牌不在 3～5 張，或底牌與公共牌重複時 panic——那代表呼叫端的
/// 狀態有誤，靜默回一個「看起來合理」的組別只會讓錯誤傳得更遠。
#[must_use]
pub fn classify(hole: [Card; 2], board: &[Card], config: &HandStrengthConfig) -> HandStrengthSnapshot {
    assert!(
        (3..=5).contains(&board.len()),
        "公共牌必須是 3～5 張，收到 {}",
        board.len()
    );
    let mut seen = Vec::with_capacity(board.len() + 2);
    seen.extend_from_slice(board);
    seen.extend_from_slice(&hole);
    for (index, card) in seen.iter().enumerate() {
        assert!(
            !seen[..index].contains(card),
            "底牌與公共牌不得重複：{card:?}"
        );
    }

    let street = street_of(board);
    let hero = key_rank(&seen);
    let board_partial = board_partial_rank(board);
    let improves = hero > board_partial;

    let percentile = random_opponent_percentile_myriad(hole, board);
    let has_showdown_value = percentile >= config.showdown_value_floor_myriad;

    let outs = enumerate_outs(hole, board, config);
    let raw_outs = u8::try_from(outs.len()).unwrap_or(u8::MAX);
    let effective_outs_centi = outs
        .iter()
        .map(|out| u32::from(out.centi))
        .sum::<u32>()
        .min(u32::from(u16::MAX));
    let effective_outs_centi = u16::try_from(effective_outs_centi).unwrap_or(u16::MAX);

    let draw_outs_centi = outs
        .iter()
        .filter(|out| out.makes_flush || out.makes_straight)
        .map(|out| u32::from(out.centi))
        .sum::<u32>()
        .min(u32::from(u16::MAX));
    let draw_outs_centi = u16::try_from(draw_outs_centi).unwrap_or(u16::MAX);

    let makes_flush = outs.iter().any(|out| out.makes_flush);
    let makes_straight = outs.iter().any(|out| out.makes_straight);
    let combo_draw = makes_flush && makes_straight;
    let nut_draw = outs.iter().any(|out| out.nut);
    let backdoor_draw = matches!(street, Street::Flop) && has_backdoor(hole, board);

    let snapshot = HandStrengthSnapshot {
        // 先放一個佔位值；判定樹需要上面全部欄位，因此最後才填
        group: HandStrength::Air,
        made_category: hero.category,
        improves_board: improves,
        board_partial_category: board_partial.category,
        random_opponent_percentile_myriad: percentile,
        has_showdown_value,
        raw_outs,
        effective_outs_centi,
        draw_outs_centi,
        nut_draw,
        combo_draw,
        backdoor_draw,
    };

    HandStrengthSnapshot {
        group: decide_group(&snapshot, street, config),
        ..snapshot
    }
}

/// 判定樹（計劃 §2.3）。
///
/// **不是純優先序。** `bluffCatcher` 必須向後檢查該手是否已符合強聽牌，
/// 這種例外無法用「依序取第一個命中」表達；若照純優先序實作，
/// `HandStrength::priority()` 的順序測試會鎖住一個判定流程實際上並不
/// 遵守的順序。
fn decide_group(
    snapshot: &HandStrengthSnapshot,
    street: Street,
    config: &HandStrengthConfig,
) -> HandStrength {
    let percentile = snapshot.random_opponent_percentile_myriad;
    let is_made = snapshot.is_made_hand();
    let strong_draw =
        snapshot.draw_outs_centi >= config.strong_draw_floor_centi || snapshot.combo_draw;

    // 河牌沒有未來補牌，因此走獨立分支：三個帶聽牌的組別在該街不可能成立
    if matches!(street, Street::River) {
        // 公共牌本身即成手的退化案例：河牌 `AKQJT` 彩虹這種牌面上，
        // 幾乎所有底牌都與公共牌同分，百分位會讓所有人落在約 50%。
        // 沒有這一條的話，打公共牌的手會被分到 `mediumMade`，
        // 而且沒有任何一手是 `nuts`
        if !snapshot.improves_board {
            return if percentile >= config.showdown_value_floor_myriad {
                HandStrength::BluffCatcher
            } else {
                HandStrength::Air
            };
        }
        if percentile >= config.nuts_floor_myriad {
            return HandStrength::Nuts;
        }
        if is_made && percentile >= config.strong_made_floor_myriad {
            return HandStrength::StrongMade;
        }
        if is_made && percentile >= config.medium_made_floor_myriad {
            return HandStrength::MediumMade;
        }
        if percentile >= config.showdown_value_floor_myriad {
            return HandStrength::BluffCatcher;
        }
        return HandStrength::Air;
    }

    // 1. 堅果
    if percentile >= config.nuts_floor_myriad {
        return HandStrength::Nuts;
    }
    // 2. 成牌＋強聽牌。不設百分位下限：底對＋堅果同花聽牌的價值不在
    //    目前的牌力，而在它同時有攤牌價值與大量補牌
    if is_made && strong_draw {
        return HandStrength::MadePlusDraw;
    }
    // 3. 強成牌
    if is_made && percentile >= config.strong_made_floor_myriad {
        return HandStrength::StrongMade;
    }
    // 4. 中等成牌
    if is_made && percentile >= config.medium_made_floor_myriad {
        return HandStrength::MediumMade;
    }
    // 5. 弱成牌／抓詐——**且不符合第 6 條的強聽牌條件**。
    //    這個向後檢查是判定樹的重點：A 高堅果同花聽牌的百分位常常
    //    高於攤牌門檻，純優先序會把它提前截成抓詐
    if percentile >= config.showdown_value_floor_myriad && !strong_draw {
        return HandStrength::BluffCatcher;
    }
    // 6. 強聽牌
    if strong_draw {
        return HandStrength::StrongDraw;
    }
    // 7. 弱聽牌。同樣只看聽牌類補牌：高張成對的補牌不讓一手空氣牌
    //    變成「在聽牌」
    if snapshot.draw_outs_centi > 0 || snapshot.backdoor_draw {
        return HandStrength::WeakDraw;
    }
    // 8. 空氣
    HandStrength::Air
}

/// 對一個均勻隨機合法對手底牌的目前牌力百分位（萬分比）。
///
/// 列舉所有合法對手兩張底牌並用 [`evaluate`] 精確比較，**不使用 Monte
/// Carlo**：相同輸入必須得到相同的組別，抽樣做不到這件事。
///
/// 對手模型與死牌集合都必須明寫，否則「翻牌 1081 種組合」這個數字沒有
/// 唯一解：
///
/// - 對手模型是均勻隨機的合法組合，不是會延續到本街的範圍。
/// - 百分位是對**一個**對手的牌力序位。多人底池由規則維度處理，
///   分類器不因人數調整。
/// - 死牌只扣英雄兩張與已公開的公共牌；棄牌者的手牌不扣、burn card
///   不扣、多人時對手之間不做互斥。
///
/// 取整固定為 `floor((2 × wins + ties) × 10_000 / (2 × total))`，
/// 避免平台或執行次序造成捨入差異。
#[must_use]
pub fn random_opponent_percentile_myriad(hole: [Card; 2], board: &[Card]) -> u32 {
    let mut hero_cards = Vec::with_capacity(board.len() + 2);
    hero_cards.extend_from_slice(board);
    hero_cards.extend_from_slice(&hole);
    let hero = evaluate(&hero_cards);

    let deck = remaining_cards(&hero_cards);
    let mut wins = 0u64;
    let mut ties = 0u64;
    let mut total = 0u64;

    let mut opponent = Vec::with_capacity(board.len() + 2);
    for (i, first) in deck.iter().enumerate() {
        for second in &deck[i + 1..] {
            opponent.clear();
            opponent.extend_from_slice(board);
            opponent.push(*first);
            opponent.push(*second);
            let rank = evaluate(&opponent);
            total += 1;
            if hero > rank {
                wins += 1;
            } else if hero == rank {
                ties += 1;
            }
        }
    }

    if total == 0 {
        return FULL_MYRIAD;
    }
    u32::try_from((2 * wins + ties) * u64::from(FULL_MYRIAD) / (2 * total)).unwrap_or(FULL_MYRIAD)
}

/// 列舉並折算補牌。
///
/// 「補牌」＝該張牌落下後英雄的成牌類別嚴格提升。先按實際牌張去重
/// （列舉本身就不會重複），再依三條規則折算：
///
/// - 一般乾淨補牌以 1.00 計。
/// - 未成對高張改善為一對的補牌以 `overcard_out_weight_centi`（0.50）計，
///   因此兩張活高牌最多由 6 張 raw outs 折算為 3.00 張。
/// - 污染補牌以 `polluted_out_weight_centi`（工程預設 0）計。
///
/// **污染補牌採白名單式**（計劃 §3.3 的方案 a）：「明顯會讓對手形成更強
/// 牌」是機率敘述，落不成確定性條件；因此只認三種可人工驗算的情形——
/// 該補牌使公共牌成三條、使公共牌成四張同花色，或完成一個純由公共牌
/// 構成的順子。窮舉式（方案 b）列為備案，改採前必須先過 benchmark。
fn enumerate_outs(hole: [Card; 2], board: &[Card], config: &HandStrengthConfig) -> Vec<OutWeight> {
    // 河牌沒有未來補牌
    if board.len() >= 5 {
        return Vec::new();
    }

    let mut seen = Vec::with_capacity(board.len() + 2);
    seen.extend_from_slice(board);
    seen.extend_from_slice(&hole);
    let current = key_rank(&seen);

    let mut outs = Vec::new();
    for card in remaining_cards(&seen) {
        let mut next_board = board.to_vec();
        next_board.push(card);
        let mut next_seven = seen.clone();
        next_seven.push(card);

        let improved = key_rank(&next_seven);
        if improved.category <= current.category {
            continue;
        }

        // 只認真正的同花／順子。用 `>= Flush` 的話葫蘆與四條也會被算成
        // 「成同花」，於是三條的葫蘆補牌會被當成聽牌，一手已經很強的
        // 成牌就變成「成牌＋強聽牌」
        let makes_flush = matches!(
            improved.category,
            Category::Flush | Category::StraightFlush
        );
        let makes_straight = matches!(
            improved.category,
            Category::Straight | Category::StraightFlush
        );

        let centi = if pollutes(&next_board) {
            config.polluted_out_weight_centi
        } else if current.category == Category::HighCard && improved.category == Category::Pair {
            config.overcard_out_weight_centi
        } else {
            100
        };

        outs.push(OutWeight {
            centi,
            makes_flush,
            makes_straight,
            nut: (makes_flush || makes_straight) && is_nut_after(hole, &next_board, improved),
        });
    }
    outs
}

/// 這張補牌落下後，公共牌自己是不是變得更危險。
fn pollutes(next_board: &[Card]) -> bool {
    let mut rank_counts = [0u8; 15];
    let mut suit_counts = [0u8; 4];
    for card in next_board {
        rank_counts[card.rank.value() as usize] += 1;
        suit_counts[card.suit.index()] += 1;
    }
    // 公共牌成三條
    if rank_counts.iter().any(|&n| n >= 3) {
        return true;
    }
    // 公共牌四張同花色
    if suit_counts.iter().any(|&n| n >= 4) {
        return true;
    }
    // 純由公共牌構成的順子（最少要五張）
    next_board.len() >= 5 && evaluate(next_board).category() == Category::Straight
}

/// 補牌完成後，英雄的同花或順子是不是該公共牌所能形成的最強者。
///
/// **是否因公對而可能被葫蘆壓過不納入判定**：這個旗標問的是「同花／順子
/// 之中最強嗎」，不是「一定贏嗎」。把葫蘆威脅算進來的話，任何公對面上
/// 都不會有堅果聽牌，這個欄位就失去用途。
fn is_nut_after(hole: [Card; 2], next_board: &[Card], improved: crate::eval::KeyRank) -> bool {
    let mut best_opponent = None;
    let hero_rank = {
        let mut cards = next_board.to_vec();
        cards.extend_from_slice(&hole);
        key_rank(&cards)
    };

    let mut seen = next_board.to_vec();
    seen.extend_from_slice(&hole);
    let deck = remaining_cards(&seen);

    // 只比同類別的最強：問的是「同花／順子之中最強嗎」
    for (i, first) in deck.iter().enumerate() {
        for second in &deck[i + 1..] {
            let mut cards = next_board.to_vec();
            cards.push(*first);
            cards.push(*second);
            let rank = key_rank(&cards);
            if rank.category != improved.category {
                continue;
            }
            best_opponent = Some(match best_opponent {
                None => rank,
                Some(best) if rank > best => rank,
                Some(best) => best,
            });
        }
    }

    best_opponent.is_none_or(|best| hero_rank >= best)
}

/// 翻牌的後門聽牌標記。
///
/// - 後門同花：英雄兩張底牌同花色，且公共牌恰有兩張同色。
/// - 後門順子：存在一個五點數視窗，英雄底牌與公共牌在其中合計有三種點數
///   且至少一張來自底牌，仍缺兩張。
fn has_backdoor(hole: [Card; 2], board: &[Card]) -> bool {
    let suited = hole[0].suit == hole[1].suit;
    if suited {
        let same_suit_on_board = board
            .iter()
            .filter(|card| card.suit == hole[0].suit)
            .count();
        if same_suit_on_board == 2 {
            return true;
        }
    }

    let mut from_hole = [false; 15];
    let mut present = [false; 15];
    for card in &hole {
        let value = usize::from(card.rank.value());
        from_hole[value] = true;
        present[value] = true;
        if card.rank == Rank::Ace {
            from_hole[1] = true;
            present[1] = true;
        }
    }
    for card in board {
        let value = usize::from(card.rank.value());
        present[value] = true;
        if card.rank == Rank::Ace {
            present[1] = true;
        }
    }

    (1usize..=10).any(|low| {
        let window = low..=low + 4;
        let distinct = window.clone().filter(|rank| present[*rank]).count();
        let has_hole_card = window.filter(|rank| from_hole[*rank]).count() > 0;
        distinct == 3 && has_hole_card
    })
}

/// 牌堆中尚未出現的牌。
///
/// 死牌只扣傳入的那些：棄牌者的手牌不扣、burn card 不扣。
fn remaining_cards(seen: &[Card]) -> Vec<Card> {
    let mut deck = Vec::with_capacity(52 - seen.len());
    for rank in Rank::ALL {
        for suit in Suit::ALL {
            let card = Card::new(rank, suit);
            if !seen.contains(&card) {
                deck.push(card);
            }
        }
    }
    deck
}

const fn street_of(board: &[Card]) -> Street {
    match board.len() {
        3 => Street::Flop,
        4 => Street::Turn,
        _ => Street::River,
    }
}
