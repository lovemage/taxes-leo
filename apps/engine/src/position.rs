//! 位置解析：BB／SB／按鈕的座位，以及每手的位置標籤。
//!
//! 規格來源：`德州撲克規則細則.md` 8.4（行動順序）與 8.4.1（位置標籤）。
//!
//! **行動順序與位置標籤是兩件事**，必須分別計算。dead button 那手沒有人
//! 持有按鈕，行動順序仍以按鈕「位置」為基準，但標籤層沒有任何玩家標 `BTN`。

use std::fmt;

/// 位置標籤。
///
/// 規則細則 8.4.1：「此組字串是引擎、策略內容（preflop baseline 的欄位鍵）
/// 與 UI 三方共用的唯一命名，不另設別名或顯示用轉換表。」
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PositionLabel {
    Utg,
    Utg1,
    Utg2,
    Utg3,
    Utg4,
    Lj,
    Hj,
    Co,
    Btn,
    Sb,
    Bb,
}

impl PositionLabel {
    /// 策略內容與 log 使用的唯一字串。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utg => "UTG",
            Self::Utg1 => "UTG+1",
            Self::Utg2 => "UTG+2",
            Self::Utg3 => "UTG+3",
            Self::Utg4 => "UTG+4",
            Self::Lj => "LJ",
            Self::Hj => "HJ",
            Self::Co => "CO",
            Self::Btn => "BTN",
            Self::Sb => "SB",
            Self::Bb => "BB",
        }
    }
}

impl fmt::Display for PositionLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一手牌的位置解析結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Positions {
    /// 按鈕**位置**。dead button 時該座位無人
    pub button: usize,
    /// SB 位的玩家；`None` 表示 dead small blind
    pub small_blind_seat: Option<usize>,
    /// BB 位的玩家。恆存在（BB 位定義為下一個在桌玩家）
    pub big_blind_seat: usize,
    pub dead_button: bool,
    pub dead_small_blind: bool,
    /// 逐座位標籤；未入座者為 `None`
    pub labels: Vec<Option<PositionLabel>>,
}

/// 由目前的 BB 座位解析整手位置。
///
/// 規則細則 8.4：
/// 1. BB 位每手前進到下一個仍在桌的玩家，任何玩家不得跳過 BB。
/// 2. SB 位為 BB 位的**前一個座位**；該座無人則為 dead small blind。
/// 3. 按鈕位為 SB 位的**前一個座位**；該座無人則為 dead button。
///
/// 注意第 2、3 點是「前一個**座位**」而非「前一個在桌玩家」，
/// 這正是 dead blind 與 dead button 得以出現的原因。
///
/// # Panics
/// `bb_seat` 未入座時 panic。
#[must_use]
pub fn resolve(occupied: &[bool], bb_seat: usize) -> Positions {
    let n = occupied.len();
    assert!(occupied[bb_seat], "BB 位必須有玩家");

    let small_blind_pos = (bb_seat + n - 1) % n;
    let button = (bb_seat + n - 2) % n;
    let dead_small_blind = !occupied[small_blind_pos];
    let dead_button = !occupied[button];

    let mut labels = vec![None; n];
    labels[bb_seat] = Some(PositionLabel::Bb);
    if !dead_small_blind {
        labels[small_blind_pos] = Some(PositionLabel::Sb);
    }
    if !dead_button {
        labels[button] = Some(PositionLabel::Btn);
    }

    // 其餘在桌玩家，自 BB 左側起順時針即為「由早到晚」
    let others: Vec<usize> = (1..=n)
        .map(|offset| (bb_seat + offset) % n)
        .filter(|&seat| occupied[seat] && labels[seat].is_none())
        .collect();

    for (label, &seat) in labels_for(others.len()).into_iter().zip(&others) {
        labels[seat] = Some(label);
    }

    Positions {
        button,
        small_blind_seat: (!dead_small_blind).then_some(small_blind_pos),
        big_blind_seat: bb_seat,
        dead_button,
        dead_small_blind,
        labels,
    }
}

/// k 位非盲注非按鈕玩家的標籤，由早到晚。
///
/// 規則細則 8.4.1 第 5 點採「按鈕端與最早位雙向錨定」：
/// 最靠近按鈕者為 `CO`，次為 `HJ`，再次為 `LJ`（僅當 k ≥ 4）；
/// 最早的一位恆為 `UTG`；中間依序補 `UTG+1`、`UTG+2`。
#[must_use]
fn labels_for(k: usize) -> Vec<PositionLabel> {
    use PositionLabel::{Co, Hj, Lj, Utg, Utg1, Utg2, Utg3, Utg4};

    if k == 0 {
        return Vec::new();
    }
    // 按鈕端（由早到晚）
    let tail: &[PositionLabel] = match k {
        1 => &[],
        2 => &[Co],
        3 => &[Hj, Co],
        _ => &[Lj, Hj, Co],
    };
    // 早位：UTG 恆為最早，其後依序補 UTG+n
    let early = k - tail.len();
    let heads = [Utg, Utg1, Utg2, Utg3, Utg4];
    let mut labels: Vec<PositionLabel> = heads.iter().copied().take(early).collect();
    debug_assert_eq!(labels.len(), early, "早位標籤不足以填滿 k={k}");
    labels.extend_from_slice(tail);
    labels
}

/// 找出下一個仍在桌的 BB 座位。
///
/// 規則細則 8.4 第 1、5 點：BB 嚴格往前移動到下一個在桌玩家，
/// 因此同一玩家不會因他人離桌而連續兩手付 BB。
///
/// # Panics
/// 桌上無人時 panic。
#[must_use]
pub fn advance_big_blind(occupied: &[bool], current_bb: usize) -> usize {
    let n = occupied.len();
    (1..=n)
        .map(|offset| (current_bb + offset) % n)
        .find(|&seat| occupied[seat])
        .expect("桌上至少須有一名玩家")
}

#[cfg(test)]
mod tests {
    use super::PositionLabel::{Bb, Btn, Co, Hj, Lj, Sb, Utg, Utg1, Utg2};
    use super::*;

    fn labels_in_order(p: &Positions, occupied: &[bool]) -> Vec<PositionLabel> {
        // 自 BB 左側起順時針列出（由早到晚），BB 排最後
        let n = occupied.len();
        (1..=n)
            .map(|offset| (p.big_blind_seat + offset) % n)
            .filter_map(|seat| p.labels[seat])
            .collect()
    }

    #[test]
    fn 九人滿桌的標籤與規格表一致() {
        let occupied = vec![true; 9];
        let p = resolve(&occupied, 2);
        assert_eq!(p.button, 0);
        assert_eq!(p.small_blind_seat, Some(1));
        assert!(!p.dead_button && !p.dead_small_blind);
        assert_eq!(
            labels_in_order(&p, &occupied),
            vec![Utg, Utg1, Utg2, Lj, Hj, Co, Btn, Sb, Bb]
        );
    }

    #[test]
    fn 六人桌無_lj() {
        // 規則細則 8.4.1：k=3 時無 LJ
        let occupied = vec![true; 6];
        let p = resolve(&occupied, 2);
        assert_eq!(
            labels_in_order(&p, &occupied),
            vec![Utg, Hj, Co, Btn, Sb, Bb]
        );
    }

    #[test]
    fn 七人與八人桌的標籤與規格表一致() {
        let occupied = vec![true; 7];
        let p = resolve(&occupied, 2);
        assert_eq!(
            labels_in_order(&p, &occupied),
            vec![Utg, Lj, Hj, Co, Btn, Sb, Bb]
        );

        let occupied = vec![true; 8];
        let p = resolve(&occupied, 2);
        assert_eq!(
            labels_in_order(&p, &occupied),
            vec![Utg, Utg1, Lj, Hj, Co, Btn, Sb, Bb]
        );
    }
}
