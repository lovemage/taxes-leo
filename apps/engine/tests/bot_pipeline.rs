//! Bot 決策管線的驗收測試（核心規格 4.3）。
//!
//! 管線的七個步驟順序固定。本檔的重點不只是「跑得動」，而是驗證
//! **順序不能換**——特別是 legal mask 必須在偏移之後、noise 必須在
//! mask 之後，這兩處寫反都不會爆炸，只會靜默產生錯誤的分佈。

use poker_engine::betting::Action;
use poker_engine::bot::params::ParamValue;
use poker_engine::bot::pipeline::{run, BotConfig, PipelineError, PipelineStage};
use poker_engine::chips::Chips;
use poker_engine::strategy::distribution::{ActionDistribution, Myriad, FULL};

fn raise(units: u64) -> Action {
    Action::RaiseTo(Chips::new(units))
}

/// 典型的翻前分佈：棄／跟／兩種加注尺度。
fn baseline() -> ActionDistribution {
    ActionDistribution::new(vec![
        (Action::Fold, 3_000),
        (Action::Call, 3_000),
        (raise(6), 2_500),
        (raise(12), 1_500),
    ])
    .expect("建立基準分佈")
}

fn all_legal(_: Action) -> bool {
    true
}

// ── 管線結構 ────────────────────────────────────────────────────────────

#[test]
fn trace_涵蓋規格的七個階段且順序固定() {
    let config = BotConfig::defaults("預設");
    let trace = run(&baseline(), &config, all_legal, 0).expect("管線執行");

    let stages: Vec<PipelineStage> = trace.stages.iter().map(|(s, _)| *s).collect();
    assert_eq!(
        stages,
        vec![
            PipelineStage::Baseline,
            PipelineStage::Persona,
            PipelineStage::Behavior,
            PipelineStage::SeatOverride,
            PipelineStage::LegalMaskAndCap,
            PipelineStage::Noise,
            PipelineStage::Sampled,
        ],
        "階段順序必須與核心規格 4.3 的七步一一對應"
    );
}

#[test]
fn 每個階段的分佈合計皆為百分之百() {
    let mut config = BotConfig::defaults("侵略");
    config
        .set_persona("preflopAggression", ParamValue::Myriad(16_000))
        .expect("設定");
    config
        .set_behavior("decisionNoisePp", ParamValue::Myriad(1_500))
        .expect("設定");

    let trace = run(&baseline(), &config, all_legal, 5_000).expect("管線執行");
    for (stage, distribution) in &trace.stages {
        let total: Myriad = distribution.entries().iter().map(|(_, w)| *w).sum();
        assert_eq!(total, FULL, "{} 階段的合計不為 100%", stage.as_str());
    }
}

#[test]
fn 預設參數不改變基準分佈() {
    let config = BotConfig::defaults("預設");
    let trace = run(&baseline(), &config, all_legal, 0).expect("管線執行");
    assert_eq!(
        trace.at(PipelineStage::Noise).expect("有噪音階段"),
        &baseline(),
        "全預設時管線不應改變基準分佈"
    );
    assert!(trace.applied_offsets.is_empty(), "全預設時不應有具名偏移");
}

// ── 步驟 2：Persona 偏移 ────────────────────────────────────────────────

#[test]
fn 提高侵略性會把權重移向加注() {
    let mut config = BotConfig::defaults("侵略");
    config
        .set_persona("preflopAggression", ParamValue::Myriad(18_000))
        .expect("設定");

    let trace = run(&baseline(), &config, all_legal, 0).expect("管線執行");
    let persona = trace.at(PipelineStage::Persona).expect("有人格階段");

    let aggressive: Myriad = persona
        .entries()
        .iter()
        .filter(|(a, _)| matches!(a, Action::RaiseTo(_)))
        .map(|(_, w)| *w)
        .sum();
    assert!(aggressive > 4_000, "加注權重應由 40% 提高，實得 {aggressive}");
    assert!(
        trace
            .applied_offsets
            .iter()
            .any(|(k, _)| *k == "preflopAggression"),
        "具名偏移必須寫入 trace 供 UI 解釋"
    );
}

#[test]
fn 提高跟注黏著度會把權重移向跟注() {
    let mut config = BotConfig::defaults("跟注站");
    config
        .set_persona("callPersistence", ParamValue::Myriad(18_000))
        .expect("設定");

    let trace = run(&baseline(), &config, all_legal, 0).expect("管線執行");
    let persona = trace.at(PipelineStage::Persona).expect("有人格階段");
    assert!(
        persona.weight_of(Action::Call) > 3_000,
        "跟注權重應提高"
    );
}

// ── 步驟 3：行為參數 ────────────────────────────────────────────────────

#[test]
fn 限制可用尺度時權重併入保留尺度而非消失() {
    let mut config = BotConfig::defaults("單一尺度");
    config
        .set_behavior("allowedBetSizes", ParamValue::Count(1))
        .expect("設定");

    let trace = run(&baseline(), &config, all_legal, 0).expect("管線執行");
    let behavior = trace.at(PipelineStage::Behavior).expect("有行為階段");

    let sizes = behavior
        .entries()
        .iter()
        .filter(|(a, w)| matches!(a, Action::RaiseTo(_)) && *w > 0)
        .count();
    assert_eq!(sizes, 1, "只應保留一個加注尺度");

    // 關鍵：主動總權重不得因為砍尺度而減少，否則等於偷改了侵略性
    let aggressive: Myriad = behavior
        .entries()
        .iter()
        .filter(|(a, _)| matches!(a, Action::RaiseTo(_)))
        .map(|(_, w)| *w)
        .sum();
    assert_eq!(
        aggressive, 4_000,
        "被砍掉的尺度權重必須併入保留尺度，不得憑空消失"
    );
}

// ── 步驟 4：逐座覆寫 ────────────────────────────────────────────────────

#[test]
fn 逐座覆寫優先於人格層() {
    let mut config = BotConfig::defaults("覆寫");
    config
        .set_persona("preflopAggression", ParamValue::Myriad(8_000))
        .expect("設定");
    config
        .set_override("preflopAggression", ParamValue::Myriad(16_000))
        .expect("覆寫");

    let layers = config.layers("preflopAggression");
    assert_eq!(layers.configured, Some(ParamValue::Myriad(8_000)));
    assert_eq!(layers.overridden, Some(ParamValue::Myriad(16_000)));
    assert_eq!(
        layers.effective,
        Some(ParamValue::Myriad(16_000)),
        "覆寫必須優先"
    );
    assert_eq!(
        layers.default,
        Some(ParamValue::Myriad(10_000)),
        "UI 需顯示官方預設值以呈現逐層對照"
    );
}

#[test]
fn 未登錄的參數不得注入() {
    let mut config = BotConfig::defaults("測試");
    assert!(
        config
            .set_override("notARealParameter", ParamValue::Myriad(1))
            .is_err(),
        "核心規格 4.3 第 4 點：不得直接注入未登錄參數"
    );
    assert!(
        config
            .set_persona("decisionNoisePp", ParamValue::Myriad(1))
            .is_err(),
        "行為層欄位不得從人格層設定"
    );
}

#[test]
fn 越界值不得寫入() {
    let mut config = BotConfig::defaults("測試");
    assert!(
        config
            .set_persona("preflopAggression", ParamValue::Myriad(99_000))
            .is_err(),
        "超出 schema 範圍的值必須被攔下"
    );
}

// ── 步驟 5：legal mask 與 exploit cap ───────────────────────────────────

#[test]
fn 遮蔽發生在偏移之後() {
    // 若順序寫反（先遮蔽再套偏移），移到不合法行動上的權重會憑空消失。
    // 這裡讓大尺度加注不合法，驗證其權重被重新分配而非蒸發
    let mut config = BotConfig::defaults("侵略");
    config
        .set_persona("preflopAggression", ParamValue::Myriad(16_000))
        .expect("設定");
    config
        .set_behavior("exploitAdjustmentCapPp", ParamValue::Myriad(3_000))
        .expect("設定");

    let trace = run(&baseline(), &config, |a| a != raise(12), 0).expect("管線執行");
    let masked = trace.at(PipelineStage::LegalMaskAndCap).expect("有遮蔽階段");

    assert_eq!(masked.weight_of(raise(12)), 0, "不合法行動的權重必須為 0");
    let total: Myriad = masked.entries().iter().map(|(_, w)| *w).sum();
    assert_eq!(total, FULL, "遮蔽後必須重新正規化");
}

#[test]
fn 剝削上限夾住偏離基準過大的行動() {
    let mut config = BotConfig::defaults("極端");
    config
        .set_persona("preflopAggression", ParamValue::Myriad(18_000))
        .expect("設定");
    config
        .set_persona("foldDiscipline", ParamValue::Myriad(5_000))
        .expect("設定");
    // 上限設得很小，任何偏離都會被夾住
    config
        .set_behavior("exploitAdjustmentCapPp", ParamValue::Myriad(200))
        .expect("設定");

    let trace = run(&baseline(), &config, all_legal, 0).expect("管線執行");
    assert!(trace.exploit_cap_applied, "應記錄上限確實生效");

    let capped = trace.at(PipelineStage::LegalMaskAndCap).expect("有階段");
    for &(action, weight) in capped.entries() {
        let base = baseline().weight_of(action);
        let deviation = weight.abs_diff(base);
        assert!(
            deviation <= 400,
            "{action:?} 偏離基準 {deviation}，超過上限容許值"
        );
    }
}

#[test]
fn 全部行動皆不合法時要求走_fallback() {
    let config = BotConfig::defaults("預設");
    let result = run(&baseline(), &config, |_| false, 0);
    assert_eq!(
        result.err(),
        Some(PipelineError::NoLegalAction),
        "核心規格 4.2：無合法行動必須進入 fallback，不得任選"
    );
}

// ── 步驟 6：決策噪音 ────────────────────────────────────────────────────

#[test]
fn 噪音在遮蔽之後混合() {
    // 若順序寫反（先混噪音再遮蔽），權重會被灑到不合法行動上再被砍掉，
    // 等於實際噪音量小於設定值
    let mut config = BotConfig::defaults("噪音");
    config
        .set_behavior("decisionNoisePp", ParamValue::Myriad(3_000))
        .expect("設定");

    let trace = run(&baseline(), &config, |a| a != raise(12), 0).expect("管線執行");
    let noised = trace.at(PipelineStage::Noise).expect("有噪音階段");

    assert_eq!(
        noised.weight_of(raise(12)),
        0,
        "噪音不得把權重灑到不合法行動上"
    );
    let total: Myriad = noised.entries().iter().map(|(_, w)| *w).sum();
    assert_eq!(total, FULL);
}

#[test]
fn 噪音把分佈拉向均勻() {
    let mut config = BotConfig::defaults("高噪音");
    config
        .set_behavior("decisionNoisePp", ParamValue::Myriad(3_000))
        .expect("設定");

    let trace = run(&baseline(), &config, all_legal, 0).expect("管線執行");
    let before = trace.at(PipelineStage::LegalMaskAndCap).expect("有階段");
    let after = trace.at(PipelineStage::Noise).expect("有階段");

    // 基準本來就有的行動之中，權重最低的那個應被拉高
    let (weakest, min_before) = before
        .entries()
        .iter()
        .min_by_key(|(_, w)| *w)
        .copied()
        .expect("非空");
    assert!(
        after.weight_of(weakest) > min_before,
        "噪音應拉高基準中權重最低的行動"
    );
    // 最大權重的應被拉低
    let (strongest, max_before) = before
        .entries()
        .iter()
        .max_by_key(|(_, w)| *w)
        .copied()
        .expect("非空");
    assert!(after.weight_of(strongest) < max_before, "噪音應拉低最高權重");
}

/// 噪音必須能打出基準之外的行動。
///
/// 只在既有支撐上重配權重的話，純策略的格子完全動不了——顧問的預設
/// 組合表逐格只有一個動作，那支滑桿就整個失效。
#[test]
fn 噪音會打出基準之外的合法行動() {
    let mut config = BotConfig::defaults("高噪音");
    config
        .set_behavior("decisionNoisePp", ParamValue::Myriad(3_000))
        .expect("設定");

    // 純策略：只有一個動作
    let pure = ActionDistribution::new(vec![(raise(6), FULL)]).expect("建立純策略");
    let trace = run(&pure, &config, all_legal, 0).expect("管線執行");
    let after = trace.at(PipelineStage::Noise).expect("有階段");

    assert!(after.weight_of(raise(6)) < FULL, "純策略也要被噪音拉開");
    assert!(after.weight_of(Action::Fold) > 0, "噪音應能打出基準之外的行動");
}

// ── 步驟 7：取樣與可重現性 ─────────────────────────────────────────────

#[test]
fn 相同_roll_產生相同行動() {
    let config = BotConfig::defaults("預設");
    for roll in [0, 2_500, 5_000, 9_999] {
        let a = run(&baseline(), &config, all_legal, roll).expect("執行");
        let b = run(&baseline(), &config, all_legal, roll).expect("執行");
        assert_eq!(a.final_action, b.final_action, "相同 roll 必須得到相同行動");
        assert_eq!(a, b, "整個 trace 必須逐位元一致");
    }
}

#[test]
fn 取樣結果落在分佈的支撐集內() {
    let config = BotConfig::defaults("預設");
    for roll in (0..FULL).step_by(137) {
        let trace = run(&baseline(), &config, all_legal, roll).expect("執行");
        let weight = trace
            .at(PipelineStage::Noise)
            .expect("有階段")
            .weight_of(trace.final_action);
        assert!(weight > 0, "取樣不得選中權重為 0 的行動");
    }
}
