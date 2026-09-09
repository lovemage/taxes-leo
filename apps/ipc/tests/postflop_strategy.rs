//! 面板 D 翻後策略 IPC 的驗收測試（0907 計劃 Stage 3）。
//!
//! 重點在**恢復語意**與**來源層級**：兩者在畫面上只是一顆按鈕和一行字，
//! 但錯了會讓使用者以為自己的覆寫還在（或已經沒了）。

use poker_ipc::postflop::{
    classify_postflop_hand, postflop_nodes, postflop_rule, PostflopNodeOverrideView,
    PostflopOverridesView, PostflopRuleOverrideView, PostflopRuleQuery, PostflopWeightInput,
};

fn weights(pairs: &[(&str, u32)]) -> Vec<PostflopWeightInput> {
    pairs
        .iter()
        .map(|(kind, myriad)| PostflopWeightInput {
            kind: (*kind).to_owned(),
            myriad: *myriad,
        })
        .collect()
}

/// 翻牌、無人下注、c-bet 機會、彩虹乾燥面、強成牌
fn query() -> PostflopRuleQuery {
    PostflopRuleQuery {
        street: "flop".to_owned(),
        situation: "no-bet".to_owned(),
        line: "cbet-chance".to_owned(),
        surface: "rainbow".to_owned(),
        connectivity: "dry".to_owned(),
        hand_strength: "strong-made".to_owned(),
        facing_size: "none".to_owned(),
    }
}

fn node_key() -> String {
    "flop|no-bet|cbet-chance|rainbow|dry|strong-made|none".to_owned()
}

// ── 導覽選項 ────────────────────────────────────────────────────

#[test]
fn 河牌只列出五個仍成立的牌力組() {
    let empty = PostflopOverridesView::default();
    let flop = postflop_nodes("flop", &empty);
    let river = postflop_nodes("river", &empty);

    assert_eq!(flop.hand_strengths.len(), 8);
    assert_eq!(river.hand_strengths.len(), 5, "河牌沒有未來補牌");
    assert!(
        !river
            .hand_strengths
            .iter()
            .any(|option| option.key.contains("draw")),
        "河牌不得列出任何聽牌組別：{:?}",
        river.hand_strengths
    );
}

#[test]
fn 線路帶著自己屬於哪一種下注狀態() {
    let view = postflop_nodes("flop", &PostflopOverridesView::default());
    let no_bet = view
        .lines
        .iter()
        .filter(|line| line.situation == "no-bet")
        .count();
    let facing = view
        .lines
        .iter()
        .filter(|line| line.situation == "facing-bet")
        .count();

    assert_eq!(no_bet, 7);
    assert_eq!(facing, 6);
    assert!(
        !view.lines.iter().any(|line| line.key.contains("other")),
        "不得出現共用的「其他」桶"
    );
}

#[test]
fn 牌面兩軸分開列出() {
    let view = postflop_nodes("flop", &PostflopOverridesView::default());
    assert_eq!(view.surfaces.len(), 6);
    assert_eq!(view.connectivities.len(), 2);
}

// ── 來源層級與恢復語意（計劃 §5.2）──────────────────────────────

#[test]
fn 沒有覆寫時值來自通則且恢復按鈕停用() {
    let view = postflop_rule(&query(), &PostflopOverridesView::default()).expect("節點");

    assert_eq!(view.source, "generic");
    assert!(!view.consultant_approved, "工程通則未經簽核");
    assert!(!view.restore.enabled);
    assert_eq!(view.restore.kind, "inherited");
    assert!(view.restore.reason.contains("沒有你的覆寫"));
    assert_eq!(view.total_myriad, 10_000);
}

#[test]
fn 節點覆寫生效且可以就地清除() {
    let overrides = PostflopOverridesView {
        nodes: vec![PostflopNodeOverrideView {
            node_key: node_key(),
            weights: weights(&[("check", 3_000), ("two-thirds-pot", 7_000)]),
        }],
        rules: Vec::new(),
    };
    let view = postflop_rule(&query(), &overrides).expect("節點");

    assert_eq!(view.source, "user-node-override");
    assert_eq!(
        view.weights
            .iter()
            .find(|weight| weight.kind == "check")
            .map(|weight| weight.myriad),
        Some(3_000)
    );
    assert!(view.restore.enabled, "節點覆寫可以就地清除");
    assert_eq!(view.restore.kind, "node-override");
    assert_eq!(
        view.restore.affected_nodes, 1,
        "驗收條件 10：節點覆寫只影響那一個節點"
    );
}

#[test]
fn 規則覆寫不就地清除並顯示受影響節點數() {
    let overrides = PostflopOverridesView {
        nodes: Vec::new(),
        rules: vec![PostflopRuleOverrideView {
            id: "R1".to_owned(),
            street: None,
            situation: Some("no-bet".to_owned()),
            line: None,
            surface: None,
            connectivity: None,
            hand_strength: Some("strong-made".to_owned()),
            facing_size: None,
            weights: weights(&[("check", 2_000), ("pot", 8_000)]),
        }],
    };
    let view = postflop_rule(&query(), &overrides).expect("節點");

    assert_eq!(view.source, "user-rule-override");
    assert!(
        !view.restore.enabled,
        "就地清除會連帶影響其他節點，因此按鈕停用"
    );
    assert_eq!(view.restore.kind, "rule-override");
    assert_eq!(view.restore.rule_id.as_deref(), Some("R1"));
    assert!(
        view.restore.affected_nodes > 1,
        "一條只指定牌力組與狀態的規則涵蓋很多節點，刪除前必須說出數量"
    );
    assert!(view.restore.reason.contains("R1"));
}

#[test]
fn 節點覆寫排在規則覆寫之前() {
    // 兩者都涵蓋同一個節點：更具體的節點覆寫應該贏
    let overrides = PostflopOverridesView {
        nodes: vec![PostflopNodeOverrideView {
            node_key: node_key(),
            weights: weights(&[("check", 10_000)]),
        }],
        rules: vec![PostflopRuleOverrideView {
            id: "R1".to_owned(),
            street: None,
            situation: Some("no-bet".to_owned()),
            line: None,
            surface: None,
            connectivity: None,
            hand_strength: Some("strong-made".to_owned()),
            facing_size: None,
            weights: weights(&[("pot", 10_000)]),
        }],
    };
    let view = postflop_rule(&query(), &overrides).expect("節點");

    assert_eq!(view.source, "user-node-override");
    assert_eq!(
        view.weights
            .iter()
            .find(|weight| weight.kind == "check")
            .map(|weight| weight.myriad),
        Some(10_000)
    );
}

#[test]
fn 覆寫不滲透到其他節點() {
    let overrides = PostflopOverridesView {
        nodes: vec![PostflopNodeOverrideView {
            node_key: node_key(),
            weights: weights(&[("check", 10_000)]),
        }],
        rules: Vec::new(),
    };

    // 只有牌力組不同的鄰居節點
    let mut neighbour = query();
    neighbour.hand_strength = "medium-made".to_owned();
    let view = postflop_rule(&neighbour, &overrides).expect("節點");
    assert_eq!(view.source, "generic", "驗收條件 10：不得滲透到其他牌力組");

    // 只有乾濕不同
    let mut wet = query();
    wet.connectivity = "wet".to_owned();
    assert_eq!(
        postflop_rule(&wet, &overrides).expect("節點").source,
        "generic",
        "彩虹乾燥與彩虹濕潤是不同節點"
    );
}

// ── 動作合法性 ──────────────────────────────────────────────────

#[test]
fn 無人下注的跟注與蓋牌固定不成立() {
    let view = postflop_rule(&query(), &PostflopOverridesView::default()).expect("節點");
    for kind in ["call", "fold"] {
        let weight = view
            .weights
            .iter()
            .find(|weight| weight.kind == kind)
            .expect("欄位仍要顯示");
        assert!(!weight.available, "{kind} 在無人下注時不成立");
        assert_eq!(weight.myriad, 0);
        assert!(weight.unavailable_reason.is_some(), "必須說明為什麼不成立");
    }
}

#[test]
fn 面對下注時不能過牌() {
    let mut facing = query();
    facing.situation = "facing-bet".to_owned();
    facing.line = "facing-cbet".to_owned();
    facing.facing_size = "two-thirds".to_owned();

    let view = postflop_rule(&facing, &PostflopOverridesView::default()).expect("節點");
    let check = view
        .weights
        .iter()
        .find(|weight| weight.kind == "check")
        .expect("欄位仍要顯示");
    assert!(!check.available);
    assert_eq!(view.total_myriad, 10_000);
}

// ── 不可達與輸入驗證 ────────────────────────────────────────────

#[test]
fn 不可達的節點不給編輯() {
    let mut unreachable = query();
    unreachable.street = "river".to_owned();
    unreachable.hand_strength = "strong-draw".to_owned();

    let error = postflop_rule(&unreachable, &PostflopOverridesView::default())
        .expect_err("河牌沒有聽牌組別");
    assert!(error.contains("不可達"), "{error}");
}

#[test]
fn 合計不等於百分之百的覆寫不會生效() {
    let overrides = PostflopOverridesView {
        nodes: vec![PostflopNodeOverrideView {
            node_key: node_key(),
            weights: weights(&[("check", 3_000), ("pot", 3_000)]),
        }],
        rules: Vec::new(),
    };
    let view = postflop_rule(&query(), &overrides).expect("節點");
    assert_eq!(
        view.source, "generic",
        "合計 60% 的覆寫不成立，值應落回下一層而不是被接受"
    );
}

#[test]
fn 未知的鍵回傳說明而不是靜默落回預設() {
    let mut bad = query();
    bad.hand_strength = "not-a-group".to_owned();
    let error = postflop_rule(&bad, &PostflopOverridesView::default()).expect_err("未知鍵");
    assert!(error.contains("牌力組"), "{error}");
}

// ── 靜態完整度 ──────────────────────────────────────────────────

#[test]
fn 完整度只算使用者覆寫且帶節點集合版本() {
    let empty = postflop_nodes("flop", &PostflopOverridesView::default());
    assert_eq!(empty.coverage.node_set_version, "postflop-nodes/v1");
    assert_eq!(
        empty.coverage.user, 0,
        "還沒寫任何覆寫時玩家完整度必須是 0"
    );
    assert!(
        empty.coverage.generic > 0,
        "工程通則涵蓋了節點，但那不算玩家寫的"
    );
    assert_eq!(empty.coverage.completeness_myriad, 0);
    assert_eq!(
        empty.coverage.user + empty.coverage.official + empty.coverage.generic + empty.coverage.fallback,
        empty.coverage.total_nodes,
        "四層合計必須等於可達節點數"
    );

    let overrides = PostflopOverridesView {
        nodes: vec![PostflopNodeOverrideView {
            node_key: node_key(),
            weights: weights(&[("check", 10_000)]),
        }],
        rules: Vec::new(),
    };
    let with_override = postflop_nodes("flop", &overrides);
    assert_eq!(with_override.coverage.user, 1);
    assert!(with_override.coverage.completeness_myriad > 0);
}

// ── 分類預覽 ────────────────────────────────────────────────────

#[test]
fn 分類預覽帶著對手模型與未簽核標記() {
    let preview = classify_postflop_hand("As Ts", "Ah 7s 3s").expect("預覽");

    assert_eq!(preview.hand_strength, "made-plus-draw");
    assert_eq!(preview.surface, "flush-draw");
    assert_eq!(
        preview.opponent_model, "uniformRandom",
        "顧問校準門檻時必須看得到對手模型"
    );
    assert!(!preview.consultant_approved);
    assert!(preview.percentile_myriad > 0);
    assert!(preview.draw_outs_centi > 0);
}

#[test]
fn 分類預覽拒絕重複或張數不對的牌() {
    assert!(classify_postflop_hand("As As", "Kh 7d 3c").is_err());
    assert!(classify_postflop_hand("As Ks", "Kh 7d").is_err());
    assert!(classify_postflop_hand("As", "Kh 7d 3c").is_err());
    assert!(classify_postflop_hand("Xx Ks", "Kh 7d 3c").is_err());
}

// ── 契約形狀 ────────────────────────────────────────────────────

#[test]
fn dto_序列化為_camel_case() {
    let view = postflop_rule(&query(), &PostflopOverridesView::default()).expect("節點");
    let json = serde_json::to_value(&view).expect("序列化");

    for key in ["nodeKey", "totalMyriad", "sourceLabel", "consultantApproved"] {
        assert!(json.get(key).is_some(), "缺少 {key}");
    }
    let restore = json.get("restore").expect("restore");
    for key in ["affectedNodes", "ruleId"] {
        assert!(restore.get(key).is_some(), "restore 缺少 {key}");
    }

    let nodes = postflop_nodes("flop", &PostflopOverridesView::default());
    let json = serde_json::to_value(&nodes).expect("序列化");
    assert!(json.get("nodeSetVersion").is_some());
    assert!(json
        .get("coverage")
        .and_then(|coverage| coverage.get("completenessMyriad"))
        .is_some());
}
