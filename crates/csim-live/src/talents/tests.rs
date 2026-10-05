//! Talent calculator tests, on the shipped warrior tree.

use super::*;
use crate::session::tests::empty_app;

const ARMS: u32 = 26;
const IMPROVED_HEROIC_STRIKE: u32 = 105958;
const DEFLECTION: u32 = 105957;
const IMPROVED_REND: u32 = 105956;
const IMPROVED_TACTICAL_MASTERY: u32 = 105954;
const ANGER_MANAGEMENT: u32 = 105951;
const DEEP_WOUNDS: u32 = 105950;

fn request(ranks: &[(u32, u32)], op: Op, node: Option<u32>) -> EditRequest {
    EditRequest {
        class: PlayerClass::Warrior,
        ranks: ranks.iter().copied().collect(),
        op,
        node,
        tab: None,
    }
}

fn apply(ranks: &[(u32, u32)], op: Op, node: u32) -> State {
    edit(empty_app().data(), &request(ranks, op, Some(node))).unwrap()
}

fn ranks(state: &State) -> Vec<(u32, u32)> {
    state.ranks.iter().map(|(&n, &r)| (n, r)).collect()
}

#[test]
fn the_layout_of_the_warrior_tree() {
    let app = empty_app();
    let layout = Layout::of(app.data().talents.get(PlayerClass::Warrior).unwrap());
    assert_eq!((layout.points, layout.points_per_tier), (51, 5));
    let tabs: Vec<(&str, u32)> = layout
        .tabs
        .iter()
        .map(|tab| (tab.name.as_str(), tab.talent_tab))
        .collect();
    assert_eq!(tabs, [("Arms", 161), ("Fury", 164), ("Protection", 163)]);
    assert!(layout.tabs.iter().all(|tab| tab.icon.is_some()));
    let deep_wounds = layout.tabs[0]
        .talents
        .iter()
        .find(|talent| talent.node == DEEP_WOUNDS)
        .unwrap();
    assert_eq!(deep_wounds.requires, Some(IMPROVED_REND));
    assert_eq!((deep_wounds.tier, deep_wounds.max_ranks), (2, 3));
    assert_eq!(deep_wounds.descriptions.len(), 3);
    assert!(deep_wounds.descriptions[2].contains("60%"));
    assert!(deep_wounds.icon.is_some());
    let json = serde_json::to_value(&layout).unwrap();
    assert_eq!(json["class"], "WARRIOR");
}

#[test]
fn a_point_in_an_empty_tree() {
    let state = apply(&[], Op::Increment, IMPROVED_HEROIC_STRIKE);
    assert_eq!(ranks(&state), [(IMPROVED_HEROIC_STRIKE, 1)]);
    assert_eq!(state.tab_points, [1, 0, 0]);
    assert_eq!((state.points_left, state.required_level), (50, 10));
    assert!(state.available.contains(&DEFLECTION));
    assert!(!state.available.contains(&IMPROVED_TACTICAL_MASTERY));

    let empty = apply(&[], Op::None, IMPROVED_HEROIC_STRIKE);
    assert_eq!((empty.points_left, empty.required_level), (51, 1));
}

#[test]
fn tiers_and_prerequisites_refuse_points() {
    // Tier 2 is locked at no points.
    let state = apply(&[], Op::Increment, ANGER_MANAGEMENT);
    assert!(state.ranks.is_empty());

    // 10 points unlock tier 2, but Deep Wounds needs Improved Rend maxed.
    let ten = [
        (IMPROVED_HEROIC_STRIKE, 3),
        (IMPROVED_REND, 2),
        (IMPROVED_TACTICAL_MASTERY, 5),
    ];
    let state = apply(&ten, Op::Increment, DEEP_WOUNDS);
    assert_eq!(state.tab_points[0], 10, "refused");
    assert!(state.available.contains(&ANGER_MANAGEMENT));
    assert!(!state.available.contains(&DEEP_WOUNDS));
    let state = apply(&ten, Op::Increment, IMPROVED_REND);
    assert!(state.available.contains(&DEEP_WOUNDS));

    // Improved Rend cannot lose a point while Deep Wounds has one.
    let with_deep_wounds = [
        (IMPROVED_HEROIC_STRIKE, 3),
        (IMPROVED_REND, 3),
        (IMPROVED_TACTICAL_MASTERY, 5),
        (DEEP_WOUNDS, 1),
    ];
    let state = apply(&with_deep_wounds, Op::Decrement, IMPROVED_REND);
    assert_eq!(ranks(&state).len(), 4);
    assert_eq!(state.ranks[&IMPROVED_REND], 3);
    let state = apply(&with_deep_wounds, Op::Decrement, DEEP_WOUNDS);
    assert!(!state.ranks.contains_key(&DEEP_WOUNDS));
}

#[test]
fn max_min_and_clearing() {
    let state = apply(&[], Op::Max, DEFLECTION);
    assert_eq!(ranks(&state), [(DEFLECTION, 5)]);
    let state = apply(&[(DEFLECTION, 5), (IMPROVED_REND, 1)], Op::Min, DEFLECTION);
    assert_eq!(ranks(&state), [(IMPROVED_REND, 1)]);

    let data = empty_app().data().clone();
    let full = [
        (IMPROVED_HEROIC_STRIKE, 3),
        (IMPROVED_REND, 3),
        (IMPROVED_TACTICAL_MASTERY, 5),
        (DEEP_WOUNDS, 1),
    ];
    let clear_tab = EditRequest {
        tab: Some(ARMS),
        ..request(&full, Op::ClearTab, None)
    };
    assert!(edit(&data, &clear_tab).unwrap().ranks.is_empty());
    let clear_all = request(&full, Op::ClearAll, None);
    assert_eq!(edit(&data, &clear_all).unwrap().points_left, 51);
}

#[test]
fn bad_requests() {
    let data = empty_app().data().clone();
    let bad = [
        request(&[(1, 1)], Op::None, None),
        request(&[(DEEP_WOUNDS, 1)], Op::None, None),
        request(&[(DEFLECTION, 6)], Op::None, None),
        request(&[], Op::Increment, None),
        request(&[], Op::ClearTab, None),
        EditRequest {
            class: PlayerClass::Mage,
            ..request(&[], Op::None, None)
        },
    ];
    for request in bad {
        assert!(edit(&data, &request).is_err(), "{request:?}");
    }
    let parsed: EditRequest = serde_json::from_str(
        r#"{"class": "WARRIOR", "ranks": {"105957": 2}, "op": "increment", "node": 105957}"#,
    )
    .unwrap();
    assert_eq!(edit(&data, &parsed).unwrap().ranks[&DEFLECTION], 3);
}
