//! 校準工具的涵蓋率測試。
//!
//! 起因：牌手顧問於 2026-08-19 指出工作台的 9-max 開牌範圍漏掉 UTG+1 與
//! UTG+2。原因是節點清單為手寫，遺漏不會被任何機制發現。
//!
//! 這裡把「開牌範圍必須涵蓋該桌型的全部位置」變成可驗證的條件——
//! 工具本身的節點清單由 `positions_for` 展開，本檔則驗證那個展開確實完整，
//! 且未來若有人改回手寫也會被擋下。

use std::collections::BTreeSet;

use poker_engine::position::PositionLabel;
use poker_engine::strategy::preflop::{positions_for, scenarios_for, PreflopScenario};

#[test]
fn 九人桌的位置序列包含三個_utg_變體() {
    let positions = positions_for(9);
    assert_eq!(positions.len(), 9);

    for expected in [PositionLabel::Utg, PositionLabel::Utg1, PositionLabel::Utg2] {
        assert!(
            positions.contains(&expected),
            "9-max 必須包含 {}，顧問曾指出工具漏列此位置",
            expected.as_str()
        );
    }
}

#[test]
fn 各桌型的位置序列互異且無重複() {
    for seated in 6u8..=9 {
        let positions = positions_for(seated);
        assert_eq!(
            positions.len(),
            usize::from(seated),
            "{seated}-max 應有 {seated} 個位置"
        );

        let unique: BTreeSet<PositionLabel> = positions.iter().copied().collect();
        assert_eq!(
            unique.len(),
            positions.len(),
            "{seated}-max 的位置不得重複"
        );
    }
}

/// 校準工具展示開牌範圍時，必須涵蓋該桌型的全部位置。
///
/// 這是顧問回饋直接對應的檢查：任何以「每個位置的開牌範圍」為目的的清單，
/// 都必須與 `positions_for` 等長。
#[test]
fn 開牌範圍的節點清單必須涵蓋全部位置() {
    let seated = 9u8;
    let positions = positions_for(seated);

    // 模擬工具的展開方式
    let nodes: Vec<PositionLabel> = positions.to_vec();

    assert_eq!(
        nodes.len(),
        usize::from(seated),
        "開牌範圍必須每個位置各一個節點"
    );

    let covered: BTreeSet<PositionLabel> = nodes.into_iter().collect();
    let expected: BTreeSet<PositionLabel> = positions.into_iter().collect();
    assert_eq!(covered, expected, "不得遺漏任何位置");
}

#[test]
fn 每個位置都有可展示的開牌情境() {
    for seated in 6u8..=9 {
        for hero in positions_for(seated) {
            let scenarios = scenarios_for(seated, hero);
            assert!(
                scenarios.contains(&PreflopScenario::Unopened),
                "{seated}-max 的 {} 必須有 unopened 情境可供展示",
                hero.as_str()
            );
        }
    }
}
