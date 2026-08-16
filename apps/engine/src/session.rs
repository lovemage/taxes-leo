//! 桌次（table instance）生命週期：破產離桌、自動補位、桌次串接。
//!
//! 規格來源：`德州撲克規則細則.md` 8.1、8.1.1、8.3、8.4。
//!
//! 核心規格 2.1 的**座位數不變量**：任何一手開始時在桌人數必須 ≥ 6。
//! 引擎在任何情況下不得發出 5 人以下的手牌，這條由
//! [`TableInstance::prepare_next_hand`] 強制。

use crate::chips::Chips;
use crate::hand::{play_hand, ActionProvider, HandResult, HandSetup};
use crate::position::{advance_big_blind, resolve, Positions};
use crate::rng::{Rng, RngDomain};
use crate::table::TableConfig;

/// 產品下限：不做 6-max 以下桌型（核心規格 1.2）。
pub const MIN_PLAYERS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    pub table: TableConfig,
    /// 開桌人數，6～9（核心規格 2.1）
    pub players: usize,
    /// 每座桌次開局的起始深度
    pub starting_stacks: Vec<Chips>,
    /// 自動補位目標人數；`None` 表示關閉補位
    pub auto_refill: Option<usize>,
    /// 使用者座位。統計主體只有這一座（核心規格 5.0）
    pub hero_seat: usize,
    /// run 的手數上限
    pub hand_limit: u64,
    pub master_seed: u64,
}

impl SessionConfig {
    /// 驗證設定是否符合核心規格 2.1。
    ///
    /// # Errors
    /// 人數越界、補位目標越界或起始籌碼數不符時回傳錯誤。
    pub fn validate(&self) -> Result<(), SessionError> {
        if !(MIN_PLAYERS..=9).contains(&self.players) {
            return Err(SessionError::PlayersOutOfRange(self.players));
        }
        if self.starting_stacks.len() != self.players {
            return Err(SessionError::StackCountMismatch);
        }
        if let Some(target) = self.auto_refill {
            if !(MIN_PLAYERS..=self.players).contains(&target) {
                return Err(SessionError::RefillTargetOutOfRange(target));
            }
        }
        if self.hero_seat >= self.players {
            return Err(SessionError::HeroSeatOutOfRange(self.hero_seat));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    PlayersOutOfRange(usize),
    RefillTargetOutOfRange(usize),
    StackCountMismatch,
    HeroSeatOutOfRange(usize),
}

/// 桌次結束原因（規則細則 8.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceEnd {
    /// 使用者破產。使用者是統計主體，沒有使用者的牌局不產生有效資料
    HeroBusted,
    /// 自動補位關閉且在桌人數不足 6
    NotEnoughPlayers,
    /// 達到 run 的手數上限
    HandLimitReached,
}

/// 補位事件，寫入 log 供重播還原（規則細則 8.1.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefillEvent {
    pub hand_index: u64,
    pub seat: usize,
    pub buy_in: Chips,
}

/// 一個桌次的執行摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceSummary {
    pub index: u64,
    /// 桌次存活手數。本身是策略強弱的觀測值（規則細則 8.6）
    pub hands: u64,
    pub end: InstanceEnd,
    pub refills: Vec<RefillEvent>,
}

/// 單一桌次的狀態。
#[derive(Debug, Clone)]
pub struct TableInstance {
    occupied: Vec<bool>,
    stacks: Vec<Chips>,
    big_blind_seat: usize,
    hands: u64,
    refills: Vec<RefillEvent>,
}

impl TableInstance {
    /// 開新桌次：所有座位回到設定人數與起始深度（規則細則 8.3）。
    #[must_use]
    pub fn open(config: &SessionConfig) -> Self {
        let n = config.players;
        Self {
            occupied: vec![true; n],
            stacks: config.starting_stacks.clone(),
            // 首手按鈕在座位 0，因此 BB 位為座位 2
            big_blind_seat: 2 % n,
            hands: 0,
            refills: Vec::new(),
        }
    }

    #[must_use]
    pub fn seated(&self) -> usize {
        self.occupied.iter().filter(|&&o| o).count()
    }

    #[must_use]
    pub fn stacks(&self) -> &[Chips] {
        &self.stacks
    }

    #[must_use]
    pub fn occupied(&self) -> &[bool] {
        &self.occupied
    }

    /// 移除籌碼歸零的玩家（規則細則 8.1）。
    fn remove_busted(&mut self) {
        for seat in 0..self.occupied.len() {
            if self.occupied[seat] && self.stacks[seat].is_zero() {
                self.occupied[seat] = false;
            }
        }
    }

    /// 補位至目標人數（規則細則 8.1.1）。
    ///
    /// 新 Bot 沿用該座位既有的 persona × level 設定與起始深度。人格設定是
    /// 座位的屬性而非玩家的屬性，因此這裡只需回填籌碼與在座旗標；
    /// Bot 組合在整個 run 中維持不變，`RunManifest` 的快照因此仍然成立。
    fn refill(&mut self, config: &SessionConfig, target: usize, hand_index: u64) {
        for seat in 0..self.occupied.len() {
            if self.seated() >= target {
                break;
            }
            if !self.occupied[seat] {
                self.occupied[seat] = true;
                self.stacks[seat] = config.starting_stacks[seat];
                self.refills.push(RefillEvent {
                    hand_index,
                    seat,
                    buy_in: config.starting_stacks[seat],
                });
            }
        }
    }

    /// 手與手之間的處理：移除破產者、補位、推進 BB 位。
    ///
    /// 回傳 `None` 表示桌次應結束。
    fn prepare_next_hand(
        &mut self,
        config: &SessionConfig,
        hand_index: u64,
        first_hand: bool,
    ) -> Option<InstanceEnd> {
        if !first_hand {
            self.remove_busted();

            if !self.occupied[config.hero_seat] {
                return Some(InstanceEnd::HeroBusted);
            }
            if let Some(target) = config.auto_refill {
                self.refill(config, target, hand_index);
            }
            if self.seated() < MIN_PLAYERS {
                return Some(InstanceEnd::NotEnoughPlayers);
            }
            self.big_blind_seat = advance_big_blind(&self.occupied, self.big_blind_seat);
        }

        // 座位數不變量（核心規格 2.1）：任一手開始時在桌人數必須 ≥ 6
        assert!(
            self.seated() >= MIN_PLAYERS,
            "座位數不變量破壞：本手僅 {} 人在桌",
            self.seated()
        );
        None
    }

    /// 依目前在座狀況解析本手位置。
    #[must_use]
    pub fn positions(&self) -> Positions {
        resolve(&self.occupied, self.big_blind_seat)
    }

    fn setup(&self) -> HandSetup {
        let positions = self.positions();
        HandSetup {
            stacks: self.stacks.clone(),
            occupied: self.occupied.clone(),
            button: positions.button,
            small_blind_seat: positions.small_blind_seat,
            big_blind_seat: positions.big_blind_seat,
        }
    }
}

/// 一手牌在 run 中的結果，含位置脈絡。
#[derive(Debug, Clone)]
pub struct PlayedHand {
    pub hand_index: u64,
    pub instance_index: u64,
    pub positions: Positions,
    pub result: HandResult,
    /// 本手在桌人數。逐位置統計必須依此切片（核心規格 5.3）
    pub seated: usize,
}

/// 整個 run 的結果。
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub hands_played: u64,
    pub instances: Vec<InstanceSummary>,
}

/// 執行一個 run：多桌次串接直到達成手數上限（規則細則 8.3）。
///
/// `on_hand` 於每手結束後呼叫，供上層落 log 與聚合統計。
///
/// # Panics
/// 設定不合法時 panic；請先呼叫 [`SessionConfig::validate`]。
pub fn run_session(
    config: &SessionConfig,
    provider: &mut dyn ActionProvider,
    mut on_hand: impl FnMut(&PlayedHand),
) -> RunSummary {
    config.validate().expect("SessionConfig 不合法");

    let mut hands_played = 0u64;
    let mut instances = Vec::new();
    let mut instance_index = 0u64;

    while hands_played < config.hand_limit {
        let mut instance = TableInstance::open(config);
        let mut first_hand = true;
        let end;

        loop {
            if hands_played >= config.hand_limit {
                end = InstanceEnd::HandLimitReached;
                break;
            }
            if let Some(reason) = instance.prepare_next_hand(config, hands_played, first_hand) {
                end = reason;
                break;
            }
            first_hand = false;

            let setup = instance.setup();
            let positions = instance.positions();
            let mut rng = Rng::derive(config.master_seed, hands_played, RngDomain::Deal);
            let result = play_hand(&config.table, &setup, &mut rng, provider);

            instance.stacks.clone_from(&result.final_stacks);
            instance.hands += 1;

            let played = PlayedHand {
                hand_index: hands_played,
                instance_index,
                positions,
                result,
                seated: instance.seated(),
            };
            on_hand(&played);
            hands_played += 1;
        }

        instances.push(InstanceSummary {
            index: instance_index,
            hands: instance.hands,
            end,
            refills: instance.refills.clone(),
        });
        instance_index += 1;

        // 未打任何一手就結束代表設定無法產生有效牌局，避免無限開桌
        assert!(
            instance.hands > 0 || hands_played >= config.hand_limit,
            "桌次未能打出任何一手：設定可能無法滿足最低人數"
        );
    }

    RunSummary {
        hands_played,
        instances,
    }
}
