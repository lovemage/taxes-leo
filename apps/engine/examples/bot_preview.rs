//! 快速看一眼 Bot 實際打出什麼。
//!
//! 用途是回答「參數改了到底有沒有差」這個問題，不是統計依據——
//! 取樣數與手數都刻意壓小。

use std::collections::BTreeMap;

use poker_engine::betting::Action;
use poker_engine::bot::{BotAgent, BotConfig, ParamValue};
use poker_engine::chips::Chips;
use poker_engine::hand::{ActionProvider, Street};
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::strategy::baseline::BaselineRules;
use poker_engine::strategy::DecisionView;
use poker_engine::table::TableConfig;

struct Counter {
    agent: BotAgent,
    counts: BTreeMap<&'static str, u32>,
}

impl ActionProvider for Counter {
    fn choose(&mut self, view: &DecisionView) -> Action {
        let action = self.agent.choose(view);
        if view.street == Street::Preflop {
            let key = match action {
                Action::Fold => "fold",
                Action::Check => "check",
                Action::Call => "call",
                Action::RaiseTo(_) => "raise",
                Action::AllIn => "allIn",
            };
            *self.counts.entry(key).or_default() += 1;
        }
        action
    }
}

fn run(label: &str, seats: Vec<BotConfig>) {
    let config = SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![Chips::new(400); 9],
        auto_refill: Some(9),
        hero_seat: 0,
        hand_limit: 2_000,
        master_seed: 20_260_821,
    };
    let mut counter = Counter {
        agent: BotAgent::new(
            BaselineRules::engineering_placeholder(),
            BotAgent::rankings(20_000),
            seats,
            config.master_seed,
        ),
        counts: BTreeMap::new(),
    };
    run_session(&config, &mut counter, |_| {});

    let total: u32 = counter.counts.values().sum();
    let share = |key: &str| {
        f64::from(counter.counts.get(key).copied().unwrap_or(0)) / f64::from(total.max(1)) * 100.0
    };
    println!(
        "{label:<10} 翻前 {total:>6} 次決策｜棄 {:>5.1}%  過 {:>5.1}%  跟 {:>5.1}%  加 {:>5.1}%  全下 {:>4.1}%",
        share("fold"),
        share("check"),
        share("call"),
        share("raise"),
        share("allIn"),
    );
}

fn tuned(params: &[(&str, u32)]) -> BotConfig {
    let mut config = BotConfig::defaults("調整");
    for &(key, value) in params {
        config
            .set_persona(key, ParamValue::Myriad(value))
            .expect("參數合法");
    }
    config
}

fn main() {
    run("全部預設", vec![BotConfig::defaults("預設"); 9]);
    run("窄範圍 70%", vec![tuned(&[("rangeWidth", 7_000)]); 9]);
    run("寬範圍 140%", vec![tuned(&[("rangeWidth", 14_000)]); 9]);
    run(
        "緊凶",
        vec![tuned(&[
            ("rangeWidth", 8_000),
            ("preflopAggression", 14_000),
            ("foldDiscipline", 13_000),
        ]); 9],
    );
    run(
        "鬆被動",
        vec![tuned(&[
            ("rangeWidth", 14_000),
            ("preflopAggression", 6_000),
            ("callPersistence", 14_000),
        ]); 9],
    );
}
