//! Bot 決策管線（核心規格 4.3）。
//!
//! 管線的七個步驟**順序固定**，不得調換：
//!
//! 1. 基準策略產生合法節點的 action weights
//! 2. Persona 只產生具名、受上下限約束的偏移
//! 3. 行為參數套用可用尺度、規則覆蓋、規劃深度、對手模型與誤差模型
//! 4. 逐座覆寫套用在對應欄位；不得直接注入未登錄參數
//! 5. 引擎套用 legal-action mask 與 exploit adjustment cap
//! 6. 若啟用 decision noise，以具名公式混合合法分佈，再正規化
//! 7. 使用該決策專屬 RNG stream 取樣最終行動
//!
//! # 為什麼順序不能換
//!
//! 第 5 步的 legal-action mask 必須在 persona 與行為參數**之後**：偏移可能
//! 把權重移到當下不合法的行動上，先遮蔽會讓那些權重憑空消失、改變其餘行動
//! 的相對比例。同理，noise 必須在 mask **之後**才混合，否則會把權重灑到
//! 不合法行動上。
//!
//! # 決策 trace
//!
//! 規格要求「實際偏移公式、套用前後值及最終分佈均寫入決策 trace，供 UI 解釋」。
//! [`DecisionTrace`] 逐步保存每一階段的分佈，面板 G 的
//! 「原始規則 → 人格修正 → 行為參數 → 正規化 → 最終選擇」直接由它渲染。

use std::collections::BTreeMap;

use crate::betting::Action;
use crate::bot::params::{
    spec_of, ParamError, ParamValue, BEHAVIOR_SPECS, PERSONA_SPECS,
};
use crate::strategy::distribution::{ActionDistribution, DistributionError, Myriad, FULL};

/// 一組 Bot 設定：人格層 ＋ 行為層 ＋ 逐座覆寫。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotConfig {
    pub name: String,
    persona: BTreeMap<&'static str, ParamValue>,
    behavior: BTreeMap<&'static str, ParamValue>,
    /// 逐座覆寫。兩層的任一欄皆可覆寫
    overrides: BTreeMap<&'static str, ParamValue>,
}

impl BotConfig {
    /// 全部採預設值。
    #[must_use]
    pub fn defaults(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            persona: PERSONA_SPECS.iter().map(|s| (s.key, s.default)).collect(),
            behavior: BEHAVIOR_SPECS.iter().map(|s| (s.key, s.default)).collect(),
            overrides: BTreeMap::new(),
        }
    }

    /// 設定人格層欄位。
    ///
    /// # Errors
    /// 欄位未登錄於人格層，或值越界／型別不符時回傳錯誤。
    pub fn set_persona(&mut self, key: &str, value: ParamValue) -> Result<(), ParamError> {
        let spec = PERSONA_SPECS
            .iter()
            .find(|s| s.key == key)
            .ok_or(ParamError::UnknownKey("未登錄於人格層"))?;
        spec.validate(value)?;
        self.persona.insert(spec.key, value);
        Ok(())
    }

    /// 設定行為層欄位。
    ///
    /// # Errors
    /// 欄位未登錄於行為層，或值越界／型別不符時回傳錯誤。
    pub fn set_behavior(&mut self, key: &str, value: ParamValue) -> Result<(), ParamError> {
        let spec = BEHAVIOR_SPECS
            .iter()
            .find(|s| s.key == key)
            .ok_or(ParamError::UnknownKey("未登錄於行為層"))?;
        spec.validate(value)?;
        self.behavior.insert(spec.key, value);
        Ok(())
    }

    /// 設定逐座覆寫。
    ///
    /// 核心規格 4.3 第 4 點：「不得直接注入未登錄參數」——因此覆寫的欄位鍵
    /// 必須先存在於兩層之一，否則拒絕。
    ///
    /// # Errors
    /// 欄位未登錄，或值越界／型別不符時回傳錯誤。
    pub fn set_override(&mut self, key: &str, value: ParamValue) -> Result<(), ParamError> {
        let spec = spec_of(key).ok_or(ParamError::UnknownKey("未登錄的參數"))?;
        spec.validate(value)?;
        self.overrides.insert(spec.key, value);
        Ok(())
    }

    /// 取得某欄位的**最終生效值**（覆寫優先）。
    #[must_use]
    pub fn effective(&self, key: &str) -> Option<ParamValue> {
        self.overrides
            .get(key)
            .or_else(|| self.persona.get(key))
            .or_else(|| self.behavior.get(key))
            .copied()
    }

    /// 取得某欄位的層級值，供 UI 顯示「預設 → 修正 → 最終值」。
    #[must_use]
    pub fn layers(&self, key: &str) -> ParamLayers {
        let spec = spec_of(key);
        ParamLayers {
            default: spec.map(|s| s.default),
            configured: self.persona.get(key).or_else(|| self.behavior.get(key)).copied(),
            overridden: self.overrides.get(key).copied(),
            effective: self.effective(key),
        }
    }

    fn myriad(&self, key: &str) -> Myriad {
        self.effective(key)
            .and_then(ParamValue::as_myriad)
            .unwrap_or(FULL)
    }
}

/// 某欄位的逐層值，對應 UI 規格 UX.2 的「官方預設 → 人格修正 → 行為參數 → 逐座覆寫 → 最終生效值」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamLayers {
    pub default: Option<ParamValue>,
    pub configured: Option<ParamValue>,
    pub overridden: Option<ParamValue>,
    pub effective: Option<ParamValue>,
}

/// 管線階段。順序即核心規格 4.3 的七步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipelineStage {
    Baseline,
    Persona,
    Behavior,
    SeatOverride,
    LegalMaskAndCap,
    Noise,
    Sampled,
}

impl PipelineStage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "基準策略",
            Self::Persona => "人格修正",
            Self::Behavior => "行為參數",
            Self::SeatOverride => "逐座覆寫",
            Self::LegalMaskAndCap => "合法遮蔽與剝削上限",
            Self::Noise => "決策噪音",
            Self::Sampled => "最終選擇",
        }
    }
}

/// 決策 trace：逐階段的分佈與最終行動。
///
/// 核心規格 4.3 要求保存「實際偏移公式、套用前後值及最終分佈」，
/// 面板 G 的逐行動原因直接由此渲染。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionTrace {
    /// 每個階段結束後的分佈，依階段順序
    pub stages: Vec<(PipelineStage, ActionDistribution)>,
    /// 套用過的具名偏移及其實際倍率（萬分比）
    pub applied_offsets: Vec<(&'static str, Myriad)>,
    /// 是否因剝削上限而被夾住
    pub exploit_cap_applied: bool,
    pub final_action: Action,
}

impl DecisionTrace {
    /// 取某階段的分佈。
    #[must_use]
    pub fn at(&self, stage: PipelineStage) -> Option<&ActionDistribution> {
        self.stages
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, d)| d)
    }
}

/// 決策管線的錯誤。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineError {
    /// 遮蔽後無合法行動，呼叫端必須走 fallback（核心規格 4.2）
    NoLegalAction,
    Distribution(DistributionError),
}

impl From<DistributionError> for PipelineError {
    fn from(error: DistributionError) -> Self {
        match error {
            DistributionError::AllWeightsMasked | DistributionError::Empty => Self::NoLegalAction,
            other => Self::Distribution(other),
        }
    }
}

/// 執行完整決策管線。
///
/// `baseline` 為第 1 步的產物；`is_legal` 由引擎的合法行動產生器提供
/// （核心規格 2.2：策略層不得自行推導合法性）；`roll` 為第 7 步的取樣值，
/// 必須來自該決策專屬的 RNG stream。
///
/// # Errors
/// 遮蔽後無合法行動時回傳 [`PipelineError::NoLegalAction`]，呼叫端須走 fallback。
pub fn run(
    baseline: &ActionDistribution,
    config: &BotConfig,
    is_legal: impl Fn(Action) -> bool,
    roll: Myriad,
) -> Result<DecisionTrace, PipelineError> {
    run_with_reference(baseline, baseline, config, is_legal, roll)
}

/// 與 [`run`] 相同，但另外指定第 5 步夾幅度時要比對的**基準**。
///
/// 核心規格 4.3 第 5 步夾的是「偏離基準的幅度」。人格若在**內容層**就
/// 已經動過手（顧問的預設組合表是純策略，權重縮放對它無效，人格改走
/// `ChartShift` 的邊界位移），那麼進到管線的分佈本身就已經含著偏移，
/// 拿它當基準等於量到 0，上限永遠夾不到東西。
///
/// 因此呼叫端要把**未套用人格的內容**當 `reference` 傳進來。
///
/// # Errors
/// 見 [`run`]。
pub fn run_with_reference(
    reference: &ActionDistribution,
    baseline: &ActionDistribution,
    config: &BotConfig,
    is_legal: impl Fn(Action) -> bool,
    roll: Myriad,
) -> Result<DecisionTrace, PipelineError> {
    let mut stages = Vec::with_capacity(7);
    let mut applied_offsets = Vec::new();

    // ── 步驟 1：基準策略 ──────────────────────────────────────────
    stages.push((PipelineStage::Baseline, baseline.clone()));

    // ── 步驟 2：Persona 偏移 ─────────────────────────────────────
    // 偏移是具名且受上下限約束的（schema 的 min/max 已保證），
    // 這裡只依欄位語意決定作用在哪些行動上
    let aggression = config.myriad("preflopAggression");
    let call_persistence = config.myriad("callPersistence");
    let fold_discipline = config.myriad("foldDiscipline");

    let persona = scale_by_action(baseline, |action| match action {
        Action::RaiseTo(_) | Action::AllIn => aggression,
        Action::Call => call_persistence,
        Action::Fold => fold_discipline,
        Action::Check => FULL,
    })?;
    if aggression != FULL {
        applied_offsets.push(("preflopAggression", aggression));
    }
    if call_persistence != FULL {
        applied_offsets.push(("callPersistence", call_persistence));
    }
    if fold_discipline != FULL {
        applied_offsets.push(("foldDiscipline", fold_discipline));
    }
    stages.push((PipelineStage::Persona, persona.clone()));

    // ── 步驟 3：行為參數 ─────────────────────────────────────────
    // 可用尺度數限制取用的加注尺度數量。超出的尺度權重併回最接近的保留尺度，
    // 而非直接丟棄——丟棄會讓總權重憑空減少
    let allowed_sizes = config
        .effective("allowedBetSizes")
        .and_then(ParamValue::as_count)
        .unwrap_or(u32::MAX);
    let behavior = limit_bet_sizes(&persona, allowed_sizes)?;
    // 只記錄**實際生效**的偏移：參數有設值但未改變分佈時不記，
    // 否則 UI 會顯示一堆沒有作用的「修正」而掩蓋真正起作用的那些
    if behavior != persona {
        applied_offsets.push(("allowedBetSizes", allowed_sizes));
    }
    stages.push((PipelineStage::Behavior, behavior.clone()));

    // ── 步驟 4：逐座覆寫 ─────────────────────────────────────────
    // 覆寫在 BotConfig::effective 已生效，因此此階段的分佈與上一階段相同；
    // 仍保留為獨立階段，讓 trace 的階段數與規格的七步一一對應
    stages.push((PipelineStage::SeatOverride, behavior.clone()));

    // ── 步驟 5：legal mask 與 exploit cap ────────────────────────
    // 必須在偏移之後才遮蔽：先遮蔽會讓移到不合法行動上的權重憑空消失
    let masked = behavior.mask_and_renormalise(&is_legal)?;
    let cap = config.myriad("exploitAdjustmentCapPp");
    let (capped, cap_applied) = apply_exploit_cap(reference, &masked, cap, &is_legal)?;
    stages.push((PipelineStage::LegalMaskAndCap, capped.clone()));

    // ── 步驟 6：決策噪音 ─────────────────────────────────────────
    // 在 mask 之後才混合，否則會把權重灑到不合法行動上
    let noise = config.myriad("decisionNoisePp");
    let noised = if noise == 0 {
        capped.clone()
    } else {
        applied_offsets.push(("decisionNoisePp", noise));
        mix_uniform(&capped, noise, &is_legal)?
    };
    stages.push((PipelineStage::Noise, noised.clone()));

    // ── 步驟 7：取樣 ─────────────────────────────────────────────
    let final_action = noised.sample(roll);
    stages.push((PipelineStage::Sampled, noised));

    Ok(DecisionTrace {
        stages,
        applied_offsets,
        exploit_cap_applied: cap_applied,
        final_action,
    })
}

/// 依行動類別套用倍率後重新正規化。
fn scale_by_action(
    distribution: &ActionDistribution,
    factor: impl Fn(Action) -> Myriad,
) -> Result<ActionDistribution, DistributionError> {
    let weights: Vec<(Action, u64)> = distribution
        .entries()
        .iter()
        .map(|&(action, weight)| {
            (
                action,
                u64::from(weight) * u64::from(factor(action)) / u64::from(FULL),
            )
        })
        .collect();
    ActionDistribution::from_weights(weights)
}

/// 限制可用的加注尺度數量。
///
/// 超出限制的尺度權重**併入保留的最大尺度**，不直接丟棄——丟棄會讓
/// 主動行動的總權重減少，等於偷偷改變了人格的侵略性。
fn limit_bet_sizes(
    distribution: &ActionDistribution,
    allowed: u32,
) -> Result<ActionDistribution, DistributionError> {
    let mut sizes: Vec<Action> = distribution
        .entries()
        .iter()
        .map(|(a, _)| *a)
        .filter(|a| matches!(a, Action::RaiseTo(_)))
        .collect();
    if sizes.len() as u64 <= u64::from(allowed) {
        return Ok(distribution.clone());
    }
    sizes.sort_by_key(|a| match a {
        Action::RaiseTo(chips) => chips.units(),
        _ => 0,
    });
    let keep = usize::try_from(allowed).unwrap_or(sizes.len()).max(1);
    let kept: Vec<Action> = sizes.iter().take(keep).copied().collect();
    let merge_target = *kept.last().expect("至少保留一個尺度");

    let mut merged: Vec<(Action, u64)> = Vec::new();
    for &(action, weight) in distribution.entries() {
        let target = if matches!(action, Action::RaiseTo(_)) && !kept.contains(&action) {
            merge_target
        } else {
            action
        };
        if let Some(slot) = merged.iter_mut().find(|(a, _)| *a == target) {
            slot.1 += u64::from(weight);
        } else {
            merged.push((target, u64::from(weight)));
        }
    }
    ActionDistribution::from_weights(merged)
}

/// 套用剝削調整上限。
///
/// 核心規格 4.3 第 5 步：引擎強制限制任一行動偏離基準的幅度。
/// 回傳是否確實夾住，供 trace 記錄。
fn apply_exploit_cap(
    baseline: &ActionDistribution,
    adjusted: &ActionDistribution,
    cap: Myriad,
    is_legal: &impl Fn(Action) -> bool,
) -> Result<(ActionDistribution, bool), DistributionError> {
    if cap >= FULL {
        return Ok((adjusted.clone(), false));
    }
    // 基準也要先遮蔽，否則比較的是兩個不同行動集合
    let masked_baseline = baseline.mask_and_renormalise(is_legal)?;

    let mut applied = false;
    let weights: Vec<(Action, u64)> = adjusted
        .entries()
        .iter()
        .map(|&(action, weight)| {
            let base = masked_baseline.weight_of(action);
            let lower = base.saturating_sub(cap);
            let upper = base.saturating_add(cap).min(FULL);
            let clamped = weight.clamp(lower, upper);
            if clamped != weight {
                applied = true;
            }
            (action, u64::from(clamped))
        })
        .collect();
    Ok((ActionDistribution::from_weights(weights)?, applied))
}

/// 以具名公式混入均勻分佈。
///
/// 公式：`最終 = (1 - noise) × 策略 + noise × 均勻`。
/// 均勻分佈涵蓋當下合法的行動（本函式在 mask 之後呼叫）。
///
/// # 為什麼候選集不能只取分佈自己的支撐
///
/// 噪音的語意是「有時候打出基準之外的行動」。只在既有項目上重新配權重
/// 的話，**純策略的格子完全動不了**——單一項目乘上任何倍率再正規化仍是
/// 100%。顧問的預設組合表逐格只有一個動作，噪音在它上面就會整個失效，
/// 使用者拉的是一支沒有作用的滑桿。
///
/// 因此候選集是「分佈已有的行動 ∪ 當下合法的基本行動」。加注**尺度**
/// 不憑空生成：噪音不該發明一個內容沒考慮過的下注額，那是另一個參數
/// （可用尺度）的職責。
fn mix_uniform(
    distribution: &ActionDistribution,
    noise: Myriad,
    is_legal: impl Fn(Action) -> bool,
) -> Result<ActionDistribution, DistributionError> {
    let mut candidates: Vec<Action> = distribution.entries().iter().map(|&(a, _)| a).collect();
    for action in [Action::Fold, Action::Check, Action::Call, Action::AllIn] {
        if is_legal(action) && !candidates.contains(&action) {
            candidates.push(action);
        }
    }

    let count = u64::try_from(candidates.len()).unwrap_or(1).max(1);
    let uniform = u64::from(FULL) / count;
    let keep = u64::from(FULL - noise);

    let weights: Vec<(Action, u64)> = candidates
        .into_iter()
        .map(|action| {
            let weight = u64::from(distribution.weight_of(action));
            let blended =
                weight * keep / u64::from(FULL) + uniform * u64::from(noise) / u64::from(FULL);
            (action, blended)
        })
        .collect();
    ActionDistribution::from_weights(weights)
}
