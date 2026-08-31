// SPDX-License-Identifier: AGPL-3.0-or-later
//! Naming rules for generated fields and modules (M1 spec section 4.2).

use std::collections::BTreeSet;

const WORD_SEPARATOR: char = '_';
const PERCENT_WORD: &str = "_pct_";
const PLUS_WORD: &str = "_plus_";
const TRAILING_MINUS_WORD: &str = "_minus";
const DIGIT_PREFIX: &str = "n_";
const EMPTY_NAME: &str = "field";
/// Keywords that cannot be raw identifiers; they get [`KEYWORD_SUFFIX`] instead.
const NON_RAW_KEYWORDS: [&str; 3] = ["crate", "self", "super"];
/// Words reserved by Rust edition 2024 that `syn` still accepts as plain identifiers.
const EDITION_RESERVED_WORDS: [&str; 1] = ["gen"];
const KEYWORD_SUFFIX: char = '_';
/// The second field with the same name gets `_2`, the third `_3`, and so on.
const FIRST_COLLISION_SUFFIX: usize = 2;

/// A generated field name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldName {
    /// The name without `r#`.
    pub text: String,
    /// True when the name is a Rust keyword and must be written as `r#name`.
    pub raw: bool,
}

/// Converts `CamelCase`, `snake_case` and mixed names to `snake_case`.
pub fn snake_case(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() && index > 0 {
            let previous = chars[index - 1];
            let next_is_lower = chars.get(index + 1).is_some_and(char::is_ascii_lowercase);
            if previous.is_ascii_lowercase()
                || previous.is_ascii_digit()
                || (previous.is_ascii_uppercase() && next_is_lower)
            {
                out.push(WORD_SEPARATOR);
            }
        }
        out.push(ch.to_ascii_lowercase());
    }
    out.split(WORD_SEPARATOR)
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(&WORD_SEPARATOR.to_string())
}

/// The field name for one column, before collision handling.
pub fn field_base_name(column: &str) -> String {
    let trimmed = column.trim();
    let last = trimmed.chars().count().saturating_sub(1);
    let mut text = String::with_capacity(trimmed.len());
    for (index, ch) in trimmed.chars().enumerate() {
        match ch {
            '%' => text.push_str(PERCENT_WORD),
            '+' => text.push_str(PLUS_WORD),
            '-' if index == last => text.push_str(TRAILING_MINUS_WORD),
            ch if ch.is_ascii_alphanumeric() => text.push(ch),
            _ => text.push(WORD_SEPARATOR),
        }
    }
    let mut name = snake_case(&text);
    if name.is_empty() {
        name = EMPTY_NAME.to_owned();
    }
    if name.starts_with(|ch: char| ch.is_ascii_digit()) {
        name.insert_str(0, DIGIT_PREFIX);
    }
    if NON_RAW_KEYWORDS.contains(&name.as_str()) {
        name.push(KEYWORD_SUFFIX);
    }
    name
}

/// Field names for columns given in sorted order. Equal names get `_2`, `_3`, ... in that order.
pub fn field_names<'a>(columns: impl IntoIterator<Item = &'a str>) -> Vec<FieldName> {
    let mut used = BTreeSet::new();
    columns
        .into_iter()
        .map(|column| {
            let base = field_base_name(column);
            let mut text = base.clone();
            let mut suffix = FIRST_COLLISION_SUFFIX;
            while !used.insert(text.clone()) {
                text = format!("{base}{WORD_SEPARATOR}{suffix}");
                suffix += 1;
            }
            let raw = is_keyword(&text);
            FieldName { text, raw }
        })
        .collect()
}

/// True when `name` can be used as a Rust type name as it is.
pub fn is_valid_type_name(name: &str) -> bool {
    !EDITION_RESERVED_WORDS.contains(&name) && syn::parse_str::<syn::Ident>(name).is_ok()
}

/// `syn` refuses keywords as plain identifiers.
fn is_keyword(name: &str) -> bool {
    !is_valid_type_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(columns: &[&str]) -> Vec<String> {
        field_names(columns.iter().copied())
            .into_iter()
            .map(|name| name.text)
            .collect()
    }

    #[test]
    fn snake_case_splits_words_and_acronyms() {
        assert_eq!(snake_case("ItemID"), "item_id");
        assert_eq!(snake_case("AITargetingData"), "ai_targeting_data");
        assert_eq!(
            snake_case("MasterItemDefinitions"),
            "master_item_definitions"
        );
        assert_eq!(
            snake_case("GatheringVFX_Offzet_Z"),
            "gathering_vfx_offzet_z"
        );
        assert_eq!(snake_case("Idle_VFX"), "idle_vfx");
        assert_eq!(snake_case("IdleVFX"), "idle_vfx");
        assert_eq!(snake_case("Tier2Bonus"), "tier2_bonus");
    }

    #[test]
    fn symbols_become_words_or_separators() {
        assert_eq!(field_base_name("%Increase"), "pct_increase");
        assert_eq!(field_base_name("Dungeon-"), "dungeon_minus");
        assert_eq!(field_base_name("Dungeon+"), "dungeon_plus");
        assert_eq!(
            field_base_name("ADSFriction-KBAM-PVP"),
            "ads_friction_kbam_pvp"
        );
        assert_eq!(
            field_base_name(" UseAttackerPosForReaction"),
            "use_attacker_pos_for_reaction"
        );
    }

    #[test]
    fn leading_digit_and_empty_names_are_fixed() {
        assert_eq!(field_base_name("2HandDamage"), "n_2_hand_damage");
        assert_eq!(field_base_name("--"), "minus");
        assert_eq!(field_base_name("??"), "field");
    }

    #[test]
    fn non_raw_keywords_get_a_suffix() {
        assert_eq!(field_base_name("Self"), "self_");
        assert_eq!(field_base_name("Crate"), "crate_");
    }

    #[test]
    fn colliding_names_get_numbered_suffixes() {
        assert_eq!(
            texts(&["GatherableEntryID", "GatherableEntryId"]),
            vec!["gatherable_entry_id", "gatherable_entry_id_2"]
        );
        assert_eq!(texts(&["A", "A_2", "a"]), vec!["a", "a_2", "a_3"]);
        assert_eq!(
            texts(&["Dungeon", "Dungeon+", "Dungeon-"]),
            vec!["dungeon", "dungeon_plus", "dungeon_minus"]
        );
    }

    #[test]
    fn edition_reserved_words_are_marked_raw() {
        assert_eq!(
            field_names(["Gen"]),
            vec![FieldName {
                text: "gen".into(),
                raw: true
            }]
        );
        assert!(!is_valid_type_name("gen"));
    }

    #[test]
    fn keywords_are_marked_raw() {
        assert_eq!(
            field_names(["Type"]),
            vec![FieldName {
                text: "type".into(),
                raw: true
            }]
        );
        assert_eq!(
            field_names(["Tier"]),
            vec![FieldName {
                text: "tier".into(),
                raw: false
            }]
        );
    }

    #[test]
    fn type_names_must_be_identifiers() {
        assert!(is_valid_type_name("MasterItemDefinitions"));
        assert!(!is_valid_type_name("Bad-Name"));
        assert!(!is_valid_type_name("type"));
        assert!(!is_valid_type_name(""));
    }
}
