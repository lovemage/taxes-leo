use poker_engine::bot::{BotAgent, BotConfig, ParamValue};
use poker_engine::strategy::baseline::BaselineRules;
use poker_engine::strategy::decision::{OpponentPublic, PublicAction};
use poker_engine::strategy::hand_strength::HandStrengthConfig;
use poker_engine::strategy::postflop::{
    Continuation as C, DecisionPhase as D, FacingSize as F, RelativePosition as R,
};
use poker_engine::strategy::postflop_view::{facing_ratio, postflop_node};
use poker_engine::strategy::{DecisionView, StackBucket};
use poker_engine::{
    betting::{Action, LegalActions, RaiseRange},
    card::Card,
    chips::Chips,
    hand::{ActionProvider, Street},
    position::PositionLabel as P,
};
use poker_ipc::postflop::*;

fn card(s: &str) -> Card {
    Card::parse(s).unwrap()
}
fn action(street: Street, seat: usize, a: Action) -> PublicAction {
    PublicAction {
        street,
        seat,
        position: if seat == 0 { P::Bb } else { P::Btn },
        action: a,
        raised: matches!(a, Action::RaiseTo(_)),
        committed_to: match a {
            Action::RaiseTo(n) => n,
            _ => Chips::ZERO,
        },
    }
}
fn view() -> DecisionView {
    DecisionView {
        seat: 0,
        position: P::Bb,
        street: Street::Flop,
        hole_cards: [card("As"), card("Ad")],
        board: vec![card("Ah"), card("7d"), card("2c")],
        seated: 6,
        effective_stack_bucket: StackBucket::Deep,
        pot: Chips::new(120),
        to_call: Chips::ZERO,
        big_blind: Chips::new(2),
        legal: LegalActions {
            seat: 0,
            can_fold: true,
            can_check: true,
            call_to: None,
            raise: Some(RaiseRange {
                min_to: Chips::new(2),
                max_to: Chips::new(400),
            }),
            all_in_to: Some(Chips::new(400)),
        },
        history: vec![action(Street::Preflop, 0, Action::RaiseTo(Chips::new(6)))],
        opponents: vec![OpponentPublic {
            seat: 1,
            position: P::Btn,
            stack: Chips::new(400),
            committed: Chips::ZERO,
            folded: false,
            all_in: false,
        }],
    }
}
fn context(v: &DecisionView) -> poker_engine::strategy::postflop::PostflopContext {
    postflop_node(v, &HandStrengthConfig::ENGINEERING)
        .unwrap()
        .context
}
#[test]
fn exact_facing_boundaries_and_overbets() {
    for (bet, pot, want) in [
        (99, 300, F::UpToThird),
        (100, 300, F::UpToThird),
        (101, 300, F::ThirdToHalf),
        (150, 300, F::ThirdToHalf),
        (151, 300, F::HalfToTwoThirds),
        (200, 300, F::HalfToTwoThirds),
        (201, 300, F::TwoThirdsToPot),
        (299, 300, F::TwoThirdsToPot),
        (300, 300, F::PotOrMore),
        (900, 300, F::PotOrMore),
    ] {
        assert_eq!(facing_ratio(bet, pot), want);
    }
    assert_eq!(facing_ratio(u64::MAX, u64::MAX), F::PotOrMore);
}
#[test]
fn position_and_check_raise_are_from_history() {
    let mut v = view();
    assert_eq!(context(&v).relative_position, R::First);
    v.history.push(action(Street::Flop, 0, Action::Check));
    v.history
        .push(action(Street::Flop, 1, Action::RaiseTo(Chips::new(40))));
    v.to_call = Chips::new(40);
    assert_eq!(context(&v).decision_phase, D::Checked);
    let mut before = v.opponents[0];
    before.seat = 2;
    before.position = P::Sb;
    v.opponents.push(before);
    assert_eq!(context(&v).relative_position, R::Middle);
    v.opponents[0].all_in = true;
    assert_eq!(context(&v).relative_position, R::Last);
}
#[test]
fn continuation_does_not_misname_check_raise_or_delayed_bets() {
    let mut v = view();
    assert_eq!(context(&v).continuation, C::FlopCbet);
    v.street = Street::Turn;
    v.board.push(card("Ts"));
    assert_eq!(context(&v).continuation, C::DelayedCbet);
    v.history
        .push(action(Street::Flop, 0, Action::RaiseTo(Chips::new(30))));
    assert_eq!(context(&v).continuation, C::DoubleBarrel);
    v.history
        .push(action(Street::Turn, 0, Action::RaiseTo(Chips::new(60))));
    v.street = Street::River;
    v.board.push(card("3s"));
    assert_eq!(context(&v).continuation, C::TripleBarrel);
    v.history
        .insert(1, action(Street::Flop, 1, Action::RaiseTo(Chips::new(10))));
    assert_eq!(context(&v).continuation, C::Other);
}
fn query(scope: &str) -> PostflopRuleQuery {
    PostflopRuleQuery {
        scope: scope.into(),
        street: "flop".into(),
        situation: "facing-bet".into(),
        line: "facing-cbet".into(),
        surface: "rainbow".into(),
        connectivity: "dry".into(),
        hand_strength: "strong-made".into(),
        facing_size: "up-to-third".into(),
    }
}
fn weights(kind: &str) -> Vec<PostflopWeightInput> {
    vec![PostflopWeightInput {
        kind: kind.into(),
        myriad: 10000,
    }]
}
#[test]
fn scoped_check_raise_overrides_shared_rule_and_preserves_other_positions() {
    let q = query("BB/first/checked/*");
    let empty = PostflopOverridesView::default();
    let base = postflop_rule(&query(""), &empty).unwrap().node_key;
    let key = postflop_rule(&q, &empty).unwrap().node_key;
    let data = PostflopOverridesView {
        nodes: vec![
            PostflopNodeOverrideView {
                node_key: base,
                weights: weights("call"),
            },
            PostflopNodeOverrideView {
                node_key: key,
                weights: weights("pot"),
            },
        ],
        rules: vec![],
    };
    let result = postflop_rule(&q, &data).unwrap();
    assert!(result.restore.enabled);
    assert_eq!(
        result
            .weights
            .iter()
            .find(|w| w.kind == "pot")
            .unwrap()
            .myriad,
        10000
    );
    for scope in ["BTN/last/checked/*", "BB/first/unacted/*"] {
        let result = postflop_rule(&query(scope), &data).unwrap();
        assert!(!result.restore.enabled);
        assert_eq!(
            result
                .weights
                .iter()
                .find(|w| w.kind == "call")
                .unwrap()
                .myriad,
            10000
        );
    }
    assert!(postflop_diagnostics(&data).can_save);
    let rules = to_rule_set(&data).unwrap();
    let snapshot = poker_ipc::snapshot::postflop(&rules);
    let restored = poker_ipc::snapshot::rebuild_postflop(&snapshot).unwrap();
    assert_eq!(rules.rules()[0].condition, restored.rules()[0].condition);
    assert!(postflop_rule(&query("BB/bogus/checked/*"), &empty).is_err());
}
#[test]
fn open_sizes_apply_per_seat_and_position_and_survive_legal_clamp() {
    let rankings = poker_ipc::rankings::load().unwrap();
    let mut custom = BotConfig::defaults("custom");
    custom
        .set_behavior("openSizeCentiBb", ParamValue::Count(300))
        .unwrap();
    for position in P::ALL
        .into_iter()
        .filter(|p| !matches!(p, P::Utg3 | P::Utg4 | P::Bb))
    {
        custom
            .set_behavior(
                &format!("openSizeCentiBb:{}", position.as_str()),
                ParamValue::Count(400),
            )
            .unwrap();
    }
    let mut other = BotConfig::defaults("other");
    other
        .set_behavior("openSizeCentiBb", ParamValue::Count(500))
        .unwrap();
    let mut agent = BotAgent::new(
        BaselineRules::engineering_placeholder(),
        rankings.table().clone(),
        vec![custom, other],
        42,
    );
    for position in P::ALL
        .into_iter()
        .filter(|p| !matches!(p, P::Utg3 | P::Utg4 | P::Bb))
    {
        let mut v = view();
        v.street = Street::Preflop;
        v.board.clear();
        v.history.clear();
        v.position = position;
        v.legal.can_check = false;
        v.legal.call_to = Some(Chips::new(2));
        v.to_call = Chips::new(2);
        v.legal.raise.as_mut().unwrap().min_to = Chips::new(4);
        // AA's chart action is a raise at these depths.
        assert_eq!(
            agent.choose(&v),
            Action::RaiseTo(Chips::new(8)),
            "{position:?}"
        );
        v.seat = 1;
        v.legal.seat = 1;
        assert_eq!(agent.choose(&v), Action::RaiseTo(Chips::new(10)));
        v.legal.raise.as_mut().unwrap().min_to = Chips::new(12);
        assert_eq!(agent.choose(&v), Action::RaiseTo(Chips::new(12)));
    }
}
#[test]
fn response_size_tracks_actual_open_and_configuration_snapshot() {
    let rankings = poker_ipc::rankings::load().unwrap();
    let respond = |open| {
        let mut agent = BotAgent::new(
            BaselineRules::engineering_placeholder(),
            rankings.table().clone(),
            vec![BotConfig::defaults("hero")],
            19,
        );
        let mut v = view();
        v.street = Street::Preflop;
        v.board.clear();
        v.position = P::Btn;
        v.history = vec![PublicAction {
            position: P::Utg,
            ..action(Street::Preflop, 1, Action::RaiseTo(Chips::new(open)))
        }];
        v.to_call = Chips::new(open);
        v.pot = Chips::new(open + 3);
        v.legal.can_check = false;
        v.legal.call_to = Some(Chips::new(open));
        v.legal.raise.as_mut().unwrap().min_to = Chips::new(open * 2 - 2);
        agent.choose(&v)
    };
    let Action::RaiseTo(small) = respond(6) else {
        panic!("AA should raise")
    };
    let Action::RaiseTo(large) = respond(12) else {
        panic!("AA should raise")
    };
    assert!((large.units() as i128 - small.units() as i128 * 2).abs() <= 1);
    let mut dto = poker_ipc::bots::BotSeatConfig {
        name: "Hero".into(),
        params: std::collections::BTreeMap::from([("openSizeCentiBb".into(), 320)]),
    };
    let config = dto.to_bot_config().unwrap();
    let rules =
        poker_engine::bot::rules_for_bot(&BaselineRules::engineering_placeholder(), &config);
    assert_eq!(
        poker_ipc::snapshot::baseline(&rules)["openOverrideCentiBb"],
        320
    );
    dto.params.insert("openSizeCentiBb".into(), -1);
    assert!(dto.to_bot_config().is_err());
    dto.params.insert("openSizeCentiBb".into(), 150);
    assert!(dto.to_bot_config().is_err());
}

#[test]
fn broad_editor_query_never_inherits_a_narrow_representative_seat() {
    let empty = PostflopOverridesView::default();
    let narrow = postflop_rule(&query("BTN/last/unacted/other"), &empty).unwrap();
    let data = PostflopOverridesView {
        nodes: vec![PostflopNodeOverrideView {
            node_key: narrow.node_key,
            weights: weights("pot"),
        }],
        rules: vec![],
    };
    let common = postflop_rule(&query(""), &data).unwrap();
    assert_ne!(common.source, "user-node-override");
    assert!(!common.restore.enabled);
    let mut v = view();
    v.to_call = Chips::new(40);
    v.pot = Chips::new(140);
    let c = context(&v);
    assert_eq!(c.facing_size, F::ThirdToHalf);
    assert_eq!(c.legacy_facing_size, F::Third);
    let condition = poker_engine::strategy::postflop::PostflopCondition {
        facing_size: Some(F::Third),
        ..Default::default()
    };
    assert!(condition.matches(&c));
}
