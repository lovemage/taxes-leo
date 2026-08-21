//! 重播幀的正確性。
//!
//! 幀的金額是**另一條**算路：引擎在打牌時算一次，`frames::build` 由
//! log 事件再算一次。兩條必須對得起來，否則重播顯示的底池就是編出來的。

use poker_engine::chips::Chips;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::strategy::DecisionView;
use poker_engine::betting::Action;
use poker_engine::hand::ActionProvider;
use poker_engine::pot::RakeConfig;
use poker_engine::table::{MuckPolicy, TableConfig};
use poker_ipc::frames;
use poker_storage::codec::HandRecord;

struct Mixed(u64);

impl ActionProvider for Mixed {
    fn choose(&mut self, view: &DecisionView) -> Action {
        // 用手數當作偽隨機來源，讓 fold／call／raise 都出現，
        // 才驗得到跟注與加注兩種金額路徑
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let roll = (self.0 >> 33) % 10;
        let legal = &view.legal;
        if roll < 2 && !legal.can_check {
            return Action::Fold;
        }
        if roll < 8 {
            if legal.can_check {
                return Action::Check;
            }
            if legal.call_to.is_some() {
                return Action::Call;
            }
        }
        if let Some(range) = legal.raise {
            return Action::RaiseTo(range.min_to);
        }
        if legal.can_check {
            Action::Check
        } else if legal.call_to.is_some() {
            Action::Call
        } else {
            Action::AllIn
        }
    }
}

fn config(rake: RakeConfig, hands: u64) -> SessionConfig {
    SessionConfig {
        table: TableConfig {
            rake,
            muck: MuckPolicy::Realistic,
            ..TableConfig::simple(1, 2)
        },
        players: 9,
        starting_stacks: vec![Chips::new(400); 9],
        auto_refill: Some(9),
        hero_seat: 0,
        hand_limit: hands,
        master_seed: 20_260_821,
    }
}

fn collect(config: &SessionConfig) -> Vec<HandRecord> {
    let mut records = Vec::new();
    run_session(config, &mut Mixed(1), |played| {
        records.push(HandRecord::from_played(played));
    });
    records
}

/// 底池守恆：進池的每一分錢最後都成為 payout、退還或抽水。
///
/// 幀的底池是從 log 事件累加出來的，分配結果則是引擎寫下的。
/// 兩者相等，才代表幀顯示的金額不是自己編的。
#[test]
fn 幀的最終底池等於分配加退還加抽水() {
    for record in collect(&config(RakeConfig::NONE, 400)) {
        let pot = frames::total_pot(&record);
        let out: u64 = record.payouts.iter().map(|c| c.units()).sum::<u64>()
            + record.refunds.iter().map(|c| c.units()).sum::<u64>()
            + record.rake.units();
        assert_eq!(
            pot, out,
            "第 {} 手：幀算出的底池 {pot} 與分配總額 {out} 不符",
            record.hand_index
        );
    }
}

/// 有抽水時同樣要守恆——抽水是底池的一部分，不是憑空消失的錢。
#[test]
fn 有抽水時底池仍然守恆() {
    let rake = RakeConfig {
        basis_points: 450,
        cap: Chips::new(6),
        no_flop_no_drop: true,
    };
    for record in collect(&config(rake, 300)) {
        let pot = frames::total_pot(&record);
        let out: u64 = record.payouts.iter().map(|c| c.units()).sum::<u64>()
            + record.refunds.iter().map(|c| c.units()).sum::<u64>()
            + record.rake.units();
        assert_eq!(pot, out, "第 {} 手不守恆", record.hand_index);
    }
}

/// 籌碼不得為負：任一幀的剩餘籌碼都必須是合法的非負數。
///
/// 這條會抓到「把逐街的 `committed_to` 當成全手累計」這類錯誤——
/// 那樣算出來的投入會超過起始籌碼。
#[test]
fn 每一幀的籌碼都非負且投入不超過起始籌碼() {
    for record in collect(&config(RakeConfig::NONE, 200)) {
        let starting: Vec<u64> = record.starting_stacks.iter().map(|c| c.units()).collect();
        for (index, frame) in frames::build(&record).iter().enumerate() {
            for (seat, &committed) in frame.committed.iter().enumerate() {
                assert!(
                    committed <= starting[seat],
                    "第 {} 手第 {index} 幀：座位 {seat} 投入 {committed} 超過起始籌碼 {}",
                    record.hand_index,
                    starting[seat]
                );
            }
            assert_eq!(
                frame.pot,
                frame.committed.iter().sum::<u64>(),
                "底池必須等於各座投入之和"
            );
        }
    }
}

/// 公共牌逐街揭露：任一幀帶的公共牌不得多於該街應有的張數。
///
/// 這是隱藏資訊的結構性保證。互動對打共用同一組幀，
/// 那裡把未發的牌送出去就是實質作弊。
#[test]
fn 公共牌不得早於該街出現() {
    use poker_ipc::StreetView;

    for record in collect(&config(RakeConfig::NONE, 300)) {
        for frame in frames::build(&record) {
            let allowed = match frame.street {
                StreetView::Preflop => 0,
                StreetView::Flop => 3,
                StreetView::Turn => 4,
                StreetView::River => 5,
            };
            // settle 幀在攤牌後，此時整副 board 都是公開資訊
            if frame.kind == "settle" {
                continue;
            }
            assert!(
                frame.board.len() <= allowed,
                "第 {} 手：{:?} 街的幀帶了 {} 張公共牌",
                record.hand_index,
                frame.street,
                frame.board.len()
            );
        }
    }
}

/// 幀序列必須單調：底池只增不減，直到收池。
#[test]
fn 底池在收池前只增不減() {
    for record in collect(&config(RakeConfig::NONE, 200)) {
        let mut previous = 0u64;
        for frame in frames::build(&record) {
            assert!(
                frame.pot >= previous,
                "第 {} 手：底池從 {previous} 掉到 {}",
                record.hand_index,
                frame.pot
            );
            previous = frame.pot;
        }
    }
}
