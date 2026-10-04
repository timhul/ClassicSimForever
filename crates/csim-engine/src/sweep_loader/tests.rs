//! Sweep expansion tests against the shipped data and `data/characters/warrior_fury_dw_orc.yaml`.

use std::sync::OnceLock;

use super::*;
use crate::race::Race;

fn data() -> &'static DataBundle {
    static DATA: OnceLock<DataBundle> = OnceLock::new();
    DATA.get_or_init(|| DataBundle::load(&DataBundle::repository_dir()).unwrap())
}

/// A sweep over `warrior_fury_dw_orc.yaml` from its YAML text, without `base`.
fn sweep(yaml: &str) -> SweepSetup {
    let base = DataBundle::repository_dir()
        .join("characters")
        .join("warrior_fury_dw_orc.yaml");
    let mut sweep: SweepSetup =
        serde_yaml::from_str(&format!("name: test\nbase: x\n{yaml}")).unwrap();
    sweep.base = Some(base);
    sweep
}

/// The 48 point build of 3 talent points left: 15 Arms, 33 Fury.
const BUILD_48: &str = "
overrides:
  talents:
    Arms:
      Improved Heroic Strike: 3
      Improved Rend: 3
      Improved Tactical Mastery: 5
      Anger Management: 1
      Deep Wounds: 3
    Fury:
      Cruelty: 5
      Unbridled Wrath: 5
      Furious Precision: 2
      Piercing Howl: 1
      Blood Craze: 2
      Raging Blows: 1
      Enrage: 5
      Death Wish: 1
      Dual Wield Specialization: 3
      Flurry: 5
      Bloodthirst: 1
      Improved Execute: 2
";

const LAST_3_POINTS: &str = "
variations:
  - talent_points:
      points: 3
      talents:
        Arms: [Impale, Improved Overpower]
        Fury: [Furious Precision, Dual Wield Specialization, Booming Voice, Improved Berserker Rage]
";

fn issue_contexts(error: SweepError) -> Vec<String> {
    match error {
        SweepError::Invalid { issues, .. } => issues.into_iter().map(|i| i.context).collect(),
        other => panic!("unexpected error {other}"),
    }
}

#[test]
fn distributions_spend_exactly_the_points_within_the_caps() {
    assert_eq!(
        distributions(2, &[1, 2]),
        vec![vec![1, 1], vec![0, 2]],
        "lexicographic from the first part's largest share"
    );
    assert_eq!(distributions(3, &[1, 1]), Vec::<Vec<u32>>::new());
    assert_eq!(distributions(0, &[2, 2]), vec![vec![0, 0]]);
    // Six talents with 3, 2, 2, 1, 2 and 2 ranks open: of the 56 ways to put 3 points in 6
    // talents, 4 put 3 in a 2-rank talent and 6 put 2 or 3 in the 1-rank one.
    let all = distributions(3, &[3, 2, 2, 1, 2, 2]);
    assert_eq!(all.len(), 46);
    assert!(all.iter().all(|d| d.iter().sum::<u32>() == 3));
}

#[test]
fn the_last_3_points_of_the_48_point_build_have_46_variants() {
    let expansion = sweep(&format!("{BUILD_48}{LAST_3_POINTS}"))
        .expand(data())
        .unwrap();
    assert_eq!(expansion.combinations(), 46);
    assert_eq!(expansion.variants.len(), 46, "{:?}", expansion.invalid);
    assert_eq!(expansion.points.len(), 1);
    assert!(
        expansion.points[0]
            .0
            .starts_with("3 talent points over Impale")
    );

    let base_points: u32 = expansion
        .base
        .as_ref()
        .unwrap()
        .talents
        .values()
        .flat_map(|t| t.values())
        .sum();
    assert_eq!(base_points, 48);
    for variant in &expansion.variants {
        let points: u32 = variant
            .setup
            .talents
            .values()
            .flat_map(|t| t.values())
            .sum();
        assert_eq!(points, 51, "{}", variant.label);
    }
    let booming = &expansion
        .variants
        .iter()
        .find(|v| v.label == "Furious Precision +1, Booming Voice +2")
        .unwrap()
        .setup
        .talents["Fury"];
    assert_eq!(booming["Furious Precision"], 3);
    assert_eq!(booming["Booming Voice"], 2);
    assert!(
        !expansion.base.as_ref().unwrap().talents["Arms"].contains_key("Impale"),
        "the override replaces the base's talents"
    );
}

#[test]
fn variants_beyond_the_talent_points_are_invalid() {
    // warrior_fury_dw_orc.yaml spends all 51 points already.
    let expansion = sweep(
        "
variations:
  - talent_points:
      points: 1
      talents:
        Arms: [Improved Overpower]
",
    )
    .expand(data())
    .unwrap();
    assert_eq!(expansion.combinations(), 1);
    assert!(expansion.variants.is_empty());
    assert_eq!(expansion.invalid.len(), 1);
    assert_eq!(expansion.invalid[0].0, "Improved Overpower +1");
    assert!(expansion.invalid[0].1.contains("could be spent"));
}

#[test]
fn variants_lacking_a_rotation_prerequisite_are_invalid() {
    // warrior_fury_dw_orc.yaml has no Mortal Strike, which the arms rotation cannot do without.
    let expansion = sweep(
        "
variations:
  - options:
      - { label: fury, rotation: DW Fury }
      - { label: arms, rotation: Mortal Strike }
",
    )
    .expand(data())
    .unwrap();
    assert_eq!(expansion.combinations(), 2);
    assert_eq!(expansion.variants.len(), 1);
    assert_eq!(expansion.variants[0].label, "fury");
    assert_eq!(expansion.invalid.len(), 1);
    assert_eq!(expansion.invalid[0].0, "arms");
    assert!(
        expansion.invalid[0]
            .1
            .contains("prerequisite \"Mortal Strike\""),
        "{}",
        expansion.invalid[0].1
    );
}

#[test]
fn options_and_talent_points_multiply() {
    let expansion = sweep(&format!(
        "{BUILD_48}{LAST_3_POINTS}
  - options:
      - {{ label: Orc, race: ORC }}
      - {{ race: TROLL, rotation: DW Fury }}
"
    ))
    .expand(data())
    .unwrap();
    assert_eq!(expansion.points[1], ("2 options".to_string(), 2));
    assert_eq!(expansion.combinations(), 92);
    assert_eq!(expansion.variants.len(), 92);
    let troll = expansion
        .variants
        .iter()
        .find(|v| v.label == "Booming Voice +3 | race: TROLL, rotation: DW Fury")
        .unwrap();
    assert_eq!(troll.setup.race, Race::Troll);
    assert_eq!(troll.setup.talents["Fury"]["Booming Voice"], 3);
    assert!(troll.setup.path.is_some(), "keeps the base's path");
}

#[test]
fn equipment_overrides_replace_and_empty_slots() {
    let expansion = sweep(
        "
overrides:
  talents: {}
  rotation: Protection
variations:
  - options:
      - { label: one hand, equipment: { OFFHAND: null } }
      - { label: rings swapped, equipment: { RING1: { item: 18821 }, RING2: { item: 19325 } } }
",
    )
    .expand(data())
    .unwrap();
    assert_eq!(expansion.variants.len(), 2, "{:?}", expansion.invalid);
    let base = &expansion.base.as_ref().unwrap().equipment;
    let one_hand = &expansion.variants[0].setup.equipment;
    assert!(!one_hand.contains_key(&crate::item::EquipmentSlot::Offhand));
    assert_eq!(one_hand.len(), base.len() - 1);
    let rings = &expansion.variants[1].setup.equipment;
    assert_eq!(rings[&crate::item::EquipmentSlot::Ring1].item, 18821);
    assert_eq!(rings[&crate::item::EquipmentSlot::Ring2].item, 19325);
    assert_eq!(
        rings[&crate::item::EquipmentSlot::Mainhand],
        base[&crate::item::EquipmentSlot::Mainhand]
    );
}

#[test]
fn without_variation_points_the_base_is_the_only_variant() {
    let expansion = sweep("").expand(data()).unwrap();
    assert_eq!(expansion.combinations(), 1);
    assert_eq!(expansion.variants.len(), 1);
    assert_eq!(expansion.variants[0].label, "base");
}

#[test]
fn unknown_talents_and_empty_points_are_errors() {
    let error = sweep(
        "
variations:
  - talent_points:
      points: 2
      talents:
        Fury: [Furious Precision, Flury]
        Holy: [Seal]
  - options: []
",
    )
    .expand(data())
    .unwrap_err();
    assert_eq!(
        issue_contexts(error),
        vec![
            "variations[0].talent_points.talents.Fury.Flury",
            "variations[0].talent_points.talents.Holy",
            "variations[1]",
        ]
    );

    let error = sweep("overrides: { race: GOBLIN }")
        .expand(data())
        .unwrap_err();
    assert_eq!(issue_contexts(error), vec!["overrides"]);
}

#[test]
fn the_base_is_relative_to_the_sweep_file() {
    let mut sweep = sweep("");
    sweep.base = Some(PathBuf::from("../characters/warrior_fury_dw_orc.yaml"));
    sweep.path = Some(PathBuf::from("data/sweeps/x.yaml"));
    assert_eq!(
        sweep.base_path().unwrap(),
        Path::new("data/characters/warrior_fury_dw_orc.yaml")
    );
}

/// A sweep from its YAML text as if it were `data/sweeps/x.yaml`, without `base`.
fn characters_sweep(yaml: &str) -> SweepSetup {
    let mut sweep: SweepSetup = serde_yaml::from_str(&format!("name: test\n{yaml}")).unwrap();
    sweep.path = Some(DataBundle::repository_dir().join("sweeps").join("x.yaml"));
    sweep
}

const THREE_CHARACTERS: &str = "
variations:
  - characters:
      - ../characters/warrior_fury_dw_orc.yaml
      - { path: ../characters/warrior_fury_dw_human.yaml, label: Human swords }
      - { path: ../characters/warrior_fury_2h_orc.yaml }
";

#[test]
fn characters_replace_the_whole_setup() {
    let expansion = characters_sweep(THREE_CHARACTERS).expand(data()).unwrap();
    assert!(expansion.base.is_none());
    assert_eq!(expansion.points, vec![("3 characters".to_string(), 3)]);
    let labels: Vec<&str> = expansion
        .variants
        .iter()
        .map(|v| v.label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["DW Fury Orc", "Human swords", "2h Fury Orc"],
        "the label, else the setup's name; {:?}",
        expansion.invalid
    );
    let human = &expansion.variants[1].setup;
    assert_eq!(human.race, Race::Human);
    assert_eq!(
        human.equipment[&crate::item::EquipmentSlot::Mainhand].item,
        12584,
        "Grand Marshal's Longsword"
    );
    assert!(
        human
            .path
            .as_ref()
            .unwrap()
            .ends_with("characters/warrior_fury_dw_human.yaml")
    );
    let two_hander = &expansion.variants[2].setup;
    assert!(
        !two_hander
            .equipment
            .contains_key(&crate::item::EquipmentSlot::Offhand)
    );
}

#[test]
fn overrides_and_options_apply_to_every_character() {
    let expansion = characters_sweep(&format!(
        "overrides: {{ race: TAUREN }}
{THREE_CHARACTERS}
  - options:
      - {{ label: as is }}
      - {{ label: one hand, equipment: {{ OFFHAND: null }} }}
"
    ))
    .expand(data())
    .unwrap();
    assert_eq!(expansion.combinations(), 6);
    assert_eq!(expansion.variants.len(), 6, "{:?}", expansion.invalid);
    assert!(
        expansion
            .variants
            .iter()
            .all(|v| v.setup.race == Race::Tauren)
    );
    let one_hand = expansion
        .variants
        .iter()
        .find(|v| v.label == "Human swords | one hand")
        .unwrap();
    assert!(
        !one_hand
            .setup
            .equipment
            .contains_key(&crate::item::EquipmentSlot::Offhand)
    );
    assert!(
        one_hand
            .setup
            .equipment
            .contains_key(&crate::item::EquipmentSlot::Mainhand)
    );
}

#[test]
fn the_setups_come_from_either_base_or_characters() {
    let error = characters_sweep("variations: [{ options: [{ race: ORC }] }]")
        .expand(data())
        .unwrap_err();
    assert_eq!(issue_contexts(error), vec!["base"]);

    let error = sweep(THREE_CHARACTERS).expand(data()).unwrap_err();
    assert_eq!(issue_contexts(error), vec!["base"], "not both");

    let error = characters_sweep(
        "
variations:
  - options: [{ race: ORC }]
  - characters: [../characters/warrior_fury_dw_orc.yaml]
  - talent_points: { points: 1, talents: { Arms: [Impale] } }
",
    )
    .expand(data())
    .unwrap_err();
    assert_eq!(
        issue_contexts(error),
        vec!["variations[1]", "variations[2]"],
        "characters first, talent points need a base"
    );
}

#[test]
fn a_missing_character_file_is_an_error() {
    let error = characters_sweep("variations: [{ characters: [../characters/nobody.yaml] }]")
        .expand(data())
        .unwrap_err();
    assert!(matches!(error, SweepError::Io { .. }), "{error}");
}

#[test]
fn wildcards_match_any_characters() {
    assert!(wildcard_match(
        "warrior_*.yaml",
        "warrior_arms_human_swords.yaml"
    ));
    assert!(wildcard_match("warrior_*.yaml", "warrior_.yaml"));
    assert!(wildcard_match("*", "anything"));
    assert!(wildcard_match("w*_*_orc*", "warrior_fury_dw_orc.yaml"));
    assert!(!wildcard_match(
        "warrior_*.yaml",
        "rogue_mutilate_undead.yaml"
    ));
    assert!(!wildcard_match(
        "warrior_*.yaml",
        "warrior_arms_human_swords.yml"
    ));
    assert!(
        !wildcard_match("a*a", "a"),
        "the prefix and suffix do not overlap"
    );
    assert!(!wildcard_match("plain.yaml", "plain.yaml.bak"));
}

#[test]
fn a_glob_stands_for_every_matching_character_file_in_name_order() {
    let expansion = characters_sweep(
        "variations: [{ characters: [../characters/warrior_fury_dw_*.yaml, ../characters/warrior_fury_2h_orc.yaml] }]",
    )
    .expand(data())
    .unwrap();
    let mut expected: Vec<PathBuf> = fs::read_dir(DataBundle::repository_dir().join("characters"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_str().unwrap();
            name.starts_with("warrior_fury_dw_") && name.ends_with(".yaml")
        })
        .collect();
    expected.sort();
    expected.push(DataBundle::repository_dir().join("characters/warrior_fury_2h_orc.yaml"));
    let paths: Vec<&Path> = expansion
        .variants
        .iter()
        .map(|v| v.setup.path.as_deref().unwrap())
        .collect();
    assert!(expected.len() > 2);
    assert_eq!(paths.len(), expected.len(), "{:?}", expansion.invalid);
    for (path, expected) in paths.iter().zip(&expected) {
        assert!(path.ends_with(expected.file_name().unwrap()), "{path:?}");
    }
    assert_eq!(
        expansion.points,
        vec![(format!("{} characters", expected.len()), expected.len())]
    );
}

#[test]
fn a_glob_matching_nothing_or_labelled_is_an_error() {
    let error = characters_sweep("variations: [{ characters: [../characters/nobody_*.yaml] }]")
        .expand(data())
        .unwrap_err();
    assert_eq!(issue_contexts(error), vec!["variations[0].characters[0]"]);
    let error = characters_sweep(
        "variations: [{ characters: [{ path: ../characters/warrior_*.yaml, label: All }] }]",
    )
    .expand(data())
    .unwrap_err();
    assert_eq!(issue_contexts(error), vec!["variations[0].characters[0]"]);
}
