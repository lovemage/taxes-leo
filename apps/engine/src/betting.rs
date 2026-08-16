//! 單一街的下注回合與合法行動產生器。
//!
//! 規格來源：`德州撲克規則細則.md` 第一章。這裡是整個引擎最容易寫錯的地方，
//! 因為「短額全下不重開加注權」不能只看當前下注額，必須逐座追蹤
//! 「該座上次行動時面對的下注額」與「其後是否發生過完整加注」。
//!
//! 核心不變量：合法行動一律由本模組產生，UI 與策略層不得自行推導
//! （核心規格 2.2）。

use crate::chips::Chips;

/// 座位在本手的狀態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatState {
    /// 仍在牌局且有籌碼可行動
    Active,
    /// 已棄牌
    Folded,
    /// 已全下，不再行動但仍有分池資格
    AllIn,
}

/// 玩家可選擇的行動。金額一律以「本街累計投入總額」表示，不是增量，
/// 避免 UI 與引擎對「raise 5」的解讀分歧。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Fold,
    Check,
    Call,
    /// 加注到本街累計投入 `to`
    RaiseTo(Chips),
    AllIn,
}

/// 引擎回傳的合法行動集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegalActions {
    pub seat: usize,
    pub can_fold: bool,
    pub can_check: bool,
    /// 跟注需補足到的本街累計額；`None` 表示無需跟注（可過牌）
    pub call_to: Option<Chips>,
    /// 加注區間 `[min_to, max_to]`（本街累計額）。
    /// `None` 表示不得加注——可能是加注權未重開（規則細則 1.3），
    /// 也可能是籌碼不足以做出完整加注（此時仍可 `AllIn`）。
    pub raise: Option<RaiseRange>,
    /// 全下後的本街累計額；籌碼為 0 時為 `None`
    pub all_in_to: Option<Chips>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RaiseRange {
    pub min_to: Chips,
    pub max_to: Chips,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BettingError {
    /// 該座位不是當前行動者
    NotToAct,
    /// 回合已結束
    RoundComplete,
    /// 該行動不在合法集合內
    IllegalAction,
    /// 加注金額不在合法區間
    RaiseOutOfRange { min_to: Chips, max_to: Chips },
}

/// 一條街的下注回合。
#[derive(Debug, Clone)]
pub struct BettingRound {
    /// 每座剩餘籌碼
    stacks: Vec<Chips>,
    /// 每座本街已投入
    committed: Vec<Chips>,
    state: Vec<SeatState>,
    /// 本街目前最高的單人累計投入額
    current_bet: Chips,
    /// 最近一次「完整加注」造成的增額
    raise_increment: Chips,
    /// 最近一次完整加注後的累計額。
    /// 判斷加注權是否重開的依據：某座上次行動時面對的額度若小於此值，
    /// 代表其行動後發生過完整加注，加注權重開。
    last_full_raise_to: Chips,
    /// 每座上次行動時面對的當前下注額；`None` = 本街尚未行動
    last_acted_at: Vec<Option<Chips>>,
    to_act: Option<usize>,
}

impl BettingRound {
    /// 建立翻後回合：無人投入，首次下注最小為 1 BB。
    #[must_use]
    pub fn new_postflop(
        stacks: Vec<Chips>,
        state: Vec<SeatState>,
        first_to_act: usize,
        big_blind: Chips,
    ) -> Self {
        let n = stacks.len();
        let mut round = Self {
            stacks,
            committed: vec![Chips::ZERO; n],
            state,
            current_bet: Chips::ZERO,
            raise_increment: big_blind,
            last_full_raise_to: Chips::ZERO,
            last_acted_at: vec![None; n],
            to_act: None,
        };
        round.to_act = round.find_next_actor(first_to_act);
        round
    }

    /// 建立翻前回合。
    ///
    /// `committed` 為已投入的強制下注（盲注與 straddle；ante 屬 dead money，
    /// 不計入當街投入，見規則細則 2.2，因此不放進這裡）。
    /// `current_bet` 為最大的強制下注額，加注增額初始等於它（規則細則 1.2）。
    ///
    /// 強制下注的投入者 `last_acted_at` 維持 `None`，因此 BB（或最大 straddle 位）
    /// 在無人加注時仍保有 option。
    #[must_use]
    pub fn new_preflop(
        stacks: Vec<Chips>,
        state: Vec<SeatState>,
        committed: Vec<Chips>,
        first_to_act: usize,
        largest_forced_bet: Chips,
    ) -> Self {
        let n = stacks.len();
        let mut round = Self {
            stacks,
            committed,
            state,
            current_bet: largest_forced_bet,
            raise_increment: largest_forced_bet,
            last_full_raise_to: largest_forced_bet,
            last_acted_at: vec![None; n],
            to_act: None,
        };
        round.to_act = round.find_next_actor(first_to_act);
        round
    }

    #[must_use]
    pub fn to_act(&self) -> Option<usize> {
        self.to_act
    }

    #[must_use]
    pub fn current_bet(&self) -> Chips {
        self.current_bet
    }

    #[must_use]
    pub fn raise_increment(&self) -> Chips {
        self.raise_increment
    }

    #[must_use]
    pub fn committed(&self, seat: usize) -> Chips {
        self.committed[seat]
    }

    #[must_use]
    pub fn stack(&self, seat: usize) -> Chips {
        self.stacks[seat]
    }

    #[must_use]
    pub fn state(&self, seat: usize) -> SeatState {
        self.state[seat]
    }

    #[must_use]
    pub fn committed_all(&self) -> &[Chips] {
        &self.committed
    }

    /// 未棄牌的座位數（含全下）。少於 2 人時該手結束。
    #[must_use]
    pub fn contenders(&self) -> usize {
        self.state
            .iter()
            .filter(|s| !matches!(s, SeatState::Folded))
            .count()
    }

    /// 本街是否已結束。
    ///
    /// 規則細則 1.5：行動持續到「所有未棄牌且未全下的玩家，對當前下注額
    /// 都已行動且投入相等」為止。
    #[must_use]
    pub fn is_complete(&self) -> bool {
        if self.contenders() < 2 {
            return true;
        }
        self.state
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s, SeatState::Active))
            .all(|(i, _)| self.last_acted_at[i] == Some(self.current_bet))
    }

    /// 該座的加注權是否開放。
    ///
    /// 規則細則 1.3：短額全下不重開已行動者的加注權，且多個短額全下的
    /// 增額不累加。實作方式是比對「上次行動時面對的額度」與
    /// 「最近一次完整加注後的額度」。
    fn can_reopen(&self, seat: usize) -> bool {
        match self.last_acted_at[seat] {
            None => true,
            Some(faced) => faced < self.last_full_raise_to,
        }
    }

    /// 產生當前行動者的合法行動集合。
    #[must_use]
    pub fn legal_actions(&self) -> Option<LegalActions> {
        let seat = self.to_act?;
        let stack = self.stacks[seat];
        let committed = self.committed[seat];
        let to_call = self.current_bet.saturating_sub(committed);
        let max_to = committed + stack;

        let can_check = to_call.is_zero();
        // 籌碼不足以跟注全額時，跟注即為全下，統一由 AllIn 表達
        let call_to = if can_check || stack <= to_call {
            None
        } else {
            Some(self.current_bet)
        };

        // 完整加注門檻（規則細則 1.1）
        let min_raise_to = self.current_bet + self.raise_increment;
        let raise = if self.can_reopen(seat) && max_to >= min_raise_to {
            Some(RaiseRange {
                min_to: min_raise_to,
                max_to,
            })
        } else {
            None
        };

        Some(LegalActions {
            seat,
            // 無需跟注時仍允許棄牌（現實規則允許，雖然不理性）
            can_fold: true,
            can_check,
            call_to,
            raise,
            all_in_to: (!stack.is_zero()).then_some(max_to),
        })
    }

    /// 套用一個行動。
    ///
    /// # Errors
    /// 行動不在合法集合內、金額越界，或回合已結束時回傳錯誤。
    pub fn apply(&mut self, action: Action) -> Result<(), BettingError> {
        let legal = self.legal_actions().ok_or(BettingError::RoundComplete)?;
        let seat = legal.seat;

        match action {
            Action::Fold => {
                self.state[seat] = SeatState::Folded;
                self.last_acted_at[seat] = Some(self.current_bet);
            }
            Action::Check => {
                if !legal.can_check {
                    return Err(BettingError::IllegalAction);
                }
                self.last_acted_at[seat] = Some(self.current_bet);
            }
            Action::Call => {
                let target = legal.call_to.ok_or(BettingError::IllegalAction)?;
                self.move_to(seat, target);
                self.last_acted_at[seat] = Some(self.current_bet);
            }
            Action::RaiseTo(target) => {
                let range = legal.raise.ok_or(BettingError::IllegalAction)?;
                if target < range.min_to || target > range.max_to {
                    return Err(BettingError::RaiseOutOfRange {
                        min_to: range.min_to,
                        max_to: range.max_to,
                    });
                }
                self.raise_to(seat, target);
            }
            Action::AllIn => {
                let target = legal.all_in_to.ok_or(BettingError::IllegalAction)?;
                if target > self.current_bet {
                    // 可能是完整加注，也可能是短額全下；由 raise_to 判定
                    self.raise_to(seat, target);
                } else {
                    // 全下金額不足以跟注：部分跟注，不改變當前下注額
                    self.move_to(seat, target);
                    self.last_acted_at[seat] = Some(self.current_bet);
                }
            }
        }

        if matches!(self.state[seat], SeatState::Active) && self.stacks[seat].is_zero() {
            self.state[seat] = SeatState::AllIn;
        }

        self.to_act = if self.is_complete() {
            None
        } else {
            self.find_next_actor(self.next_seat(seat))
        };
        Ok(())
    }

    /// 把該座本街投入補到 `target`，扣減籌碼。
    fn move_to(&mut self, seat: usize, target: Chips) {
        let delta = target.saturating_sub(self.committed[seat]);
        debug_assert!(delta <= self.stacks[seat], "投入超過剩餘籌碼");
        self.stacks[seat] -= delta;
        self.committed[seat] += delta;
    }

    /// 提高當前下注額。依是否達到完整加注門檻，決定加注權是否重開。
    fn raise_to(&mut self, seat: usize, target: Chips) {
        let previous_bet = self.current_bet;
        let full_raise_threshold = previous_bet + self.raise_increment;

        self.move_to(seat, target);
        self.current_bet = target;

        if target >= full_raise_threshold {
            // 完整加注：更新增額並重開所有人的加注權
            self.raise_increment = target - previous_bet;
            self.last_full_raise_to = target;
        }
        // 短額全下：raise_increment 與 last_full_raise_to 皆不變，
        // 因此已行動者的 can_reopen 仍為 false（規則細則 1.3）

        self.last_acted_at[seat] = Some(self.current_bet);
    }

    fn next_seat(&self, seat: usize) -> usize {
        (seat + 1) % self.state.len()
    }

    /// 從 `start` 起（含）找出下一個仍需行動的座位。
    fn find_next_actor(&self, start: usize) -> Option<usize> {
        let n = self.state.len();
        (0..n)
            .map(|offset| (start + offset) % n)
            .find(|&s| {
                matches!(self.state[s], SeatState::Active)
                    && self.last_acted_at[s] != Some(self.current_bet)
            })
    }
}
