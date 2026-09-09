//! 牌力分類器的驗收測試（0907 計劃 §2.3、§3.2、§3.3）。
//!
//! 這裡守的是分類**確定性**與判定樹的三個容易寫錯的地方：河牌的不可達
//! 組別、公共牌自己就是成手的退化牌面，以及 `bluffCatcher` 對強聽牌的
//! 向後檢查。

use poker_engine::card::Card;
use poker_engine::eval::Category;
use poker_engine::hand::Street;
use poker_engine::strategy::hand_strength::{
    classify, random_opponent_percentile_myriad, HandStrengthConfig,
};
use poker_engine::strategy::postflop::HandStrength;

fn cards(text: &str) -> Vec<Card> {
    text.split_whitespace()
        .map(|t| Card::parse(t).unwrap_or_else(|| panic!("無法解析牌張 {t}")))
        .collect()
}

fn hole(text: &str) -> [Card; 2] {
    let parsed = cards(text);
    [parsed[0], parsed[1]]
}

fn group(hole_text: &str, board_text: &str) -> HandStrength {
    classify(
        hole(hole_text),
        &cards(board_text),
        &HandStrengthConfig::ENGINEERING,
    )
    .group
}

// ── golden vectors（依 Stage 1 對照表凍結）────────────────────────

#[test]
fn 八組各有代表案例() {
    let cases: [(&str, &str, HandStrength); 8] = [
        ("As 5s", "Ks 9s 2s", HandStrength::Nuts),
        ("As Ts", "Ah 7s 3s", HandStrength::MadePlusDraw),
        ("Qh Qd", "9s 5c 2d", HandStrength::StrongMade),
        ("9h 8d", "Ks 9c 3d", HandStrength::MediumMade),
        ("3h 2d", "Ks 9c 3d", HandStrength::BluffCatcher),
        ("Js Ts", "9s 5s 2d", HandStrength::StrongDraw),
        ("9h 7d", "8s 5c 2d", HandStrength::WeakDraw),
        ("7h 4d", "Ks Qc 9d", HandStrength::Air),
    ];

    for (hole_text, board_text, expected) in cases {
        assert_eq!(
            group(hole_text, board_text),
            expected,
            "{hole_text} 對 {board_text}"
        );
    }
}

#[test]
fn 成牌的定義是至少一對且確實改善公共牌() {
    let config = HandStrengthConfig::ENGINEERING;

    // 公對面上握 A2：牌型是一對 K，但那對 K 完全來自公共牌
    let snapshot = classify(hole("As 2d"), &cards("Ks Kd 7c"), &config);
    assert_eq!(snapshot.made_category, Category::Pair);
    assert!(!snapshot.improves_board);
    assert!(
        !snapshot.is_made_hand(),
        "只靠公共牌成立的牌型不算成牌，否則整個牌力組會被公共牌灌水"
    );

    // 同一個面握 77：葫蘆，確實改善
    let snapshot = classify(hole("7s 7d"), &cards("Ks Kd 7c"), &config);
    assert!(snapshot.is_made_hand());
    assert_eq!(snapshot.board_partial_category, Category::Pair);
}

// ── 河牌的不可達組別（計劃 §2.3）─────────────────────────────────

#[test]
fn 河牌永遠不會回傳三個帶聽牌的組別() {
    let config = HandStrengthConfig::ENGINEERING;
    let boards = [
        "Ks 9s 2s 7d 3c",
        "As 9c 3d 7h 2s",
        "9s 8s 2d Th Jc",
        "As Kd Qc Jh Ts",
        "2s 2d 2c 5h 9d",
    ];
    let holes = ["As 5s", "Ah Kd", "Js Ts", "3h 2d", "7c 6c"];

    for board in boards {
        for hole_text in holes {
            let board_cards = cards(board);
            let hole_cards = hole(hole_text);
            // 跳過牌張衝突的組合
            if hole_cards
                .iter()
                .any(|card| board_cards.contains(card))
            {
                continue;
            }
            let snapshot = classify(hole_cards, &board_cards, &config);
            assert!(
                snapshot.group.reachable_on(Street::River),
                "{hole_text} 對 {board} 得到 {:?}，但河牌沒有未來補牌",
                snapshot.group
            );
            assert_eq!(snapshot.draw_outs_centi, 0, "河牌不該有補牌");
        }
    }
}

// ── 公共牌本身即成手的退化案例（計劃 §3.3）───────────────────────

#[test]
fn 河牌未改善公共牌時不得高於抓詐() {
    let config = HandStrengthConfig::ENGINEERING;
    let cases = [
        // 公共牌自己就是順子：幾乎所有底牌都與公共牌同分
        ("3h 2d", "As Kd Qc Jh Ts"),
        // 四張同花公共牌
        ("3h 2d", "As Ks Qs Js 4d"),
        // 公共牌葫蘆
        ("3h 2d", "As Ad Ac Ks Kd"),
    ];

    for (hole_text, board_text) in cases {
        let snapshot = classify(hole(hole_text), &cards(board_text), &config);
        assert!(!snapshot.improves_board, "{hole_text} 對 {board_text}");
        assert!(
            matches!(
                snapshot.group,
                HandStrength::BluffCatcher | HandStrength::Air
            ),
            "{hole_text} 對 {board_text} 得到 {:?}——打公共牌的手不該被分成成牌",
            snapshot.group
        );
    }
}

#[test]
fn 公共牌即順子時打公共牌的手落在同一組() {
    // 這種牌面上所有人約 50%，若不特別處理，每一手都會變成 mediumMade
    let config = HandStrengthConfig::ENGINEERING;
    let board = cards("As Kd Qc Jh Ts");
    let a = classify(hole("3h 2d"), &board, &config);
    let b = classify(hole("7c 6d"), &board, &config);
    assert_eq!(a.group, b.group);
    assert_eq!(a.random_opponent_percentile_myriad, 5_000);
}

// ── 判定樹的向後檢查（計劃 §2.3 第 5 條）─────────────────────────

#[test]
fn a_高堅果同花聽牌不會被提前截成抓詐() {
    let config = HandStrengthConfig::ENGINEERING;
    let snapshot = classify(hole("As 4s"), &cards("Ks 9s 2d"), &config);

    assert!(
        snapshot.random_opponent_percentile_myriad >= config.showdown_value_floor_myriad,
        "這手的百分位確實過得了攤牌門檻——純優先序會在這裡就把它截成抓詐"
    );
    assert!(snapshot.draw_outs_centi >= config.strong_draw_floor_centi);
    assert_eq!(
        snapshot.group,
        HandStrength::StrongDraw,
        "第 5 條必須向後檢查強聽牌"
    );
    assert!(snapshot.nut_draw, "A 高同花聽牌完成後是堅果同花");
}

#[test]
fn 成牌加強聽牌不設百分位下限() {
    // 底對＋同花聽牌：百分位不高，但同時有攤牌價值與大量補牌
    let config = HandStrengthConfig::ENGINEERING;
    let snapshot = classify(hole("6s 5s"), &cards("Ks 9s 6d"), &config);

    assert!(snapshot.is_made_hand());
    assert!(snapshot.draw_outs_centi >= config.strong_draw_floor_centi);
    assert_eq!(snapshot.group, HandStrength::MadePlusDraw);
    assert!(
        snapshot.random_opponent_percentile_myriad < config.strong_made_floor_myriad,
        "這一組刻意不設百分位下限（計劃 §十 已鎖定決議 8）"
    );
}

// ── 確定性與對手模型（計劃 §3.3）─────────────────────────────────

#[test]
fn 相同輸入必定得到相同分類() {
    let config = HandStrengthConfig::ENGINEERING;
    let hole_cards = hole("Js Ts");
    let board = cards("9s 8s 2d");

    let first = classify(hole_cards, &board, &config);
    for _ in 0..5 {
        assert_eq!(
            classify(hole_cards, &board, &config),
            first,
            "分類不得因隨機取樣而改變"
        );
    }
}

#[test]
fn 百分位對的是單一對手且死牌只扣自己與公共牌() {
    // 翻牌 C(47,2)=1081、轉牌 C(46,2)=1035、河牌 C(45,2)=990。
    // 這三個數字要有唯一解，死牌集合就必須明確：只扣英雄兩張與已公開的
    // 公共牌，棄牌者的手牌不扣、burn card 不扣
    let hole_cards = hole("Ah Kd");
    for (board_text, _combinations) in [
        ("As 9c 3d", 1_081u32),
        ("As 9c 3d 7h", 1_035),
        ("As 9c 3d 7h 2s", 990),
    ] {
        let percentile = random_opponent_percentile_myriad(hole_cards, &cards(board_text));
        assert!(percentile <= 10_000);
    }

    // 多人底池由規則維度處理，分類器不因人數調整——同一手在同一牌面
    // 只會有一個百分位
    let board = cards("As 9c 3d");
    assert_eq!(
        random_opponent_percentile_myriad(hole_cards, &board),
        random_opponent_percentile_myriad(hole_cards, &board)
    );
}

#[test]
fn 堅果同花的百分位是滿分() {
    assert_eq!(
        random_opponent_percentile_myriad(hole("As 5s"), &cards("Ks 9s 2s")),
        10_000
    );
}

// ── 補牌折算（計劃 §3.3）────────────────────────────────────────

#[test]
fn 高張補牌以半張折算() {
    let config = HandStrengthConfig::ENGINEERING;
    // 兩張活高牌：6 張 raw outs，折算後 3.00 張
    let snapshot = classify(hole("Ah Kd"), &cards("9s 6c 2d"), &config);
    assert!(
        snapshot.effective_outs_centi < u16::from(snapshot.raw_outs) * 100,
        "高張成對的補牌不能以整張計"
    );
}

#[test]
fn 成牌的漸進改善不算聽牌() {
    let config = HandStrengthConfig::ENGINEERING;
    // 頂對弱踢腳：有 11 張補牌可以變兩對或三條，但這手沒有在聽任何大牌
    let snapshot = classify(hole("Ah 4d"), &cards("As 9c 3d"), &config);
    assert!(snapshot.effective_outs_centi > 0);
    assert_eq!(snapshot.draw_outs_centi, 0, "沒有同花或順子可聽");
    assert_eq!(
        snapshot.group,
        HandStrength::MediumMade,
        "把成牌的漸進改善算進聽牌門檻，這手會被判成 madePlusDraw"
    );
}

#[test]
fn 複合聽牌同時具備同花與順子() {
    let config = HandStrengthConfig::ENGINEERING;
    let snapshot = classify(hole("Js Ts"), &cards("9s 8s 2d"), &config);
    assert!(snapshot.combo_draw);
    assert_eq!(snapshot.group, HandStrength::StrongDraw);

    // 只有同花聽牌不算複合
    let flush_only = classify(hole("Js Ts"), &cards("9s 5s 2d"), &config);
    assert!(!flush_only.combo_draw);
}

#[test]
fn 後門聽牌只在翻牌成立() {
    let config = HandStrengthConfig::ENGINEERING;
    let flop = classify(hole("Js 4s"), &cards("9s 6c 2d"), &config);
    assert!(flop.backdoor_draw, "兩張同花底牌＋公共牌恰兩張同色");

    let turn = classify(hole("Js 4s"), &cards("9s 6c 2d Th"), &config);
    assert!(!turn.backdoor_draw, "轉牌之後沒有後門可談");
}

// ── 設定值 ──────────────────────────────────────────────────────

#[test]
fn 工程門檻尚未經顧問簽核() {
    let config = HandStrengthConfig::ENGINEERING;
    assert!(
        !config.consultant_approved,
        "未簽核內容不得顯示為 GTO 或正式顧問策略"
    );

    // 對手模型必須跟著設定一起出現，顧問才不會拿「對手續打範圍」的
    // 直覺去校準門檻
    let model: &str = HandStrengthConfig::OPPONENT_MODEL;
    assert_eq!(model, "uniformRandom");
}

#[test]
fn 攤牌價值與抓詐共用同一個門檻() {
    // 同一個門檻寫兩次必然漂移
    let config = HandStrengthConfig::ENGINEERING;
    let snapshot = classify(hole("Kh 6d"), &cards("Qs 9c 3d"), &config);
    assert_eq!(
        snapshot.has_showdown_value,
        snapshot.random_opponent_percentile_myriad >= config.showdown_value_floor_myriad
    );
}
