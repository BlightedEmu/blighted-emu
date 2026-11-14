// SPDX-License-Identifier: AGPL-3.0-or-later
//! The privacy scan of a scrubbed pack table. A hit stops the make; a review run goes into the
//! review report for the maintainer to read before publishing.
//!
//! Measured 2026-11-13 on the retail tables: runs of two or more words (the shape of a company or
//! guild name) showed only in owner states, and this rule found all three company names there and
//! nothing else; one-word runs of 5 or more letters showed about 80 times, almost all the bytes of
//! numbers, so they are reported, not refused.

use std::fmt;

use serde_json::Value;

use super::PackError;
use super::owner::{IdentityState, collect_identities, has_identity};
use super::strings::GameWords;

/// Parts of a field name that name a person or an account.
const NAMED_FIELD_PARTS: [&str; 10] = [
    "player",
    "account",
    "character",
    "owner",
    "guild",
    "steam",
    "email",
    "login",
    "token",
    "session",
];
/// A field name ending that is an index into an entity's components, not a name.
const COMPONENT_KEY_SUFFIX: &str = "_component";
/// Byte offset fields of `ftue-entities` (where the player's facet and GDE ref sit in a body).
const ALLOWED_FIELD_KEYS: [&str; 2] = ["player_facet_at", "player_gde_ref_at"];
/// The start of every Steam ID and the digits after it.
const STEAM_ID_PREFIX: &str = "7656119";
const STEAM_ID_REST_DIGITS: usize = 10;
/// A capture file name.
const LEDGER_FILE: &str = "ledger.bin";
/// The hex digits after the `-` of a capture folder name.
const CAPTURE_FOLDER_HEX_DIGITS: usize = 16;
/// A string of at least this many hex digits (and an even count) is read as bytes too.
const MIN_HEX_CHARS: usize = 16;
/// The shape of an ID written as text (`8-4-4-4-12` hex digits).
const ID_TEXT_GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
/// The shortest readable run in bytes that is looked at.
const MIN_RUN_CHARS: usize = 5;
/// A name-shaped run: at least this many words split by spaces (punctuation at a word's ends
/// trimmed), one of them with at least [`MIN_LONG_WORD_LETTERS`] letters, and at least
/// [`MIN_LETTER_PERCENT`] percent letters and spaces.
const MIN_NAME_WORDS: usize = 2;
const MIN_LONG_WORD_LETTERS: usize = 4;
/// The letters of all words of a name-shaped run together (random bytes read as UTF-8 give
/// short runs such as one letter, one CJK sign and a space).
const MIN_NAME_LETTERS: usize = 6;
const MIN_LETTER_PERCENT: usize = 70;
const PERCENT: usize = 100;
/// Punctuation trimmed from a word's ends.
const WORD_PUNCTUATION: &[char] = &['.', ',', '-', '_', '!', '?', '(', ')', '[', ']'];
/// The letters of a one-word run that goes into the review report.
const MIN_REVIEW_LETTERS: usize = 5;

/// Which rule a hit broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanRule {
    /// A field name that names a person or an account.
    NamedField,
    /// An owner state that still names a company, crest, owner or raid.
    OwnerIdentity,
    /// A Steam ID.
    SteamId,
    /// An email address.
    Email,
    /// A capture file name.
    LedgerFile,
    /// A capture folder name (a name, `-` and 16 hex digits).
    CaptureFolder,
    /// A name-shaped run of two or more words inside replicated bytes.
    TwoWordText,
    /// An ID written as text (`8-4-4-4-12` hex digits) inside replicated bytes, such as the
    /// character ids of a house plot.
    IdText,
    /// The `GdeRef` or facet id of the capturing player, or a turret state that names a player.
    PlayerId,
    /// A war roster with players.
    WarRoster,
}

/// One hit of the scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanHit {
    /// The table.
    pub table: String,
    /// The JSON path.
    pub path: String,
    /// The rule.
    pub rule: ScanRule,
}

impl fmt::Display for ScanHit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}: {:?}", self.table, self.path, self.rule)
    }
}

/// One readable word in replicated bytes that is no game string, for the review report (first
/// place it was seen).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRun {
    /// The table.
    pub table: String,
    /// The JSON path.
    pub path: String,
    /// The word.
    pub text: String,
}

/// Scans `value`, the JSON of scrubbed table `table`. `player_ids` are the capturing player's ids
/// the scrub zeroed; they must not show anywhere else.
///
/// # Errors
///
/// Returns [`PackError::MemberDecode`] when a replicated state does not decode.
pub fn scan_table(
    table: &str,
    value: &Value,
    words: &GameWords,
    player_ids: &[Vec<u8>],
) -> Result<(Vec<ScanHit>, Vec<ReviewRun>), PackError> {
    let mut scan = Scan {
        table,
        words,
        player_ids,
        hits: Vec::new(),
        review: Vec::new(),
    };
    let (_, identities) = collect_identities(table, value)?;
    for (path, state) in identities {
        if has_identity(&state) {
            let rule = match state {
                IdentityState::Owner(_) => ScanRule::OwnerIdentity,
                IdentityState::Turret(_) => ScanRule::PlayerId,
                IdentityState::Roster(_) => ScanRule::WarRoster,
            };
            scan.hit(&path, rule);
        }
    }
    scan.value(value, &mut String::new());
    Ok((scan.hits, scan.review))
}

struct Scan<'a> {
    table: &'a str,
    words: &'a GameWords,
    player_ids: &'a [Vec<u8>],
    hits: Vec<ScanHit>,
    review: Vec<ReviewRun>,
}

impl Scan<'_> {
    fn hit(&mut self, path: &str, rule: ScanRule) {
        self.hits.push(ScanHit {
            table: self.table.to_owned(),
            path: path.to_owned(),
            rule,
        });
    }

    /// Walks `value`; every string that is hex is read as bytes too.
    fn value(&mut self, value: &Value, path: &mut String) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    let len = path.len();
                    path.push('/');
                    path.push_str(key);
                    if is_named_field(key) {
                        self.hit(path, ScanRule::NamedField);
                    }
                    self.value(child, path);
                    path.truncate(len);
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    let len = path.len();
                    path.push('/');
                    path.push_str(&index.to_string());
                    self.value(item, path);
                    path.truncate(len);
                }
            }
            Value::String(text) => {
                self.text(text, path);
                if text.len() >= MIN_HEX_CHARS
                    && let Ok(bytes) = hex::decode(text)
                {
                    self.bytes(&bytes, path);
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str, path: &str) {
        if has_steam_id(text) {
            self.hit(path, ScanRule::SteamId);
        }
        if has_email(text) {
            self.hit(path, ScanRule::Email);
        }
        if text.contains(LEDGER_FILE) {
            self.hit(path, ScanRule::LedgerFile);
        }
        if has_capture_folder(text) {
            self.hit(path, ScanRule::CaptureFolder);
        }
    }

    fn bytes(&mut self, bytes: &[u8], path: &str) {
        if self.player_ids.iter().any(|id| {
            bytes
                .windows(id.len())
                .any(|window| window == id.as_slice())
        }) {
            self.hit(path, ScanRule::PlayerId);
        }
        for run in readable_runs(bytes) {
            if has_id_text(&run) {
                self.hit(path, ScanRule::IdText);
            }
            if has_two_words(&run) {
                self.hit(path, ScanRule::TwoWordText);
                continue;
            }
            let word = run.trim_matches(|c: char| !c.is_alphabetic());
            if word.chars().count() >= MIN_REVIEW_LETTERS
                && word.chars().all(char::is_alphabetic)
                && !self.words.contains(word)
            {
                self.review.push(ReviewRun {
                    table: self.table.to_owned(),
                    path: path.to_owned(),
                    text: word.to_owned(),
                });
            }
        }
    }
}

fn is_named_field(key: &str) -> bool {
    if key.ends_with(COMPONENT_KEY_SUFFIX) || ALLOWED_FIELD_KEYS.contains(&key) {
        return false;
    }
    let lower = key.to_ascii_lowercase();
    NAMED_FIELD_PARTS.iter().any(|part| lower.contains(part))
}

fn has_steam_id(text: &str) -> bool {
    text.match_indices(STEAM_ID_PREFIX).any(|(at, _)| {
        let before_is_digit = text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_digit());
        let rest = &text[at + STEAM_ID_PREFIX.len()..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        !before_is_digit && digits == STEAM_ID_REST_DIGITS
    })
}

fn is_email_local(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._%+-".contains(c)
}

fn is_email_domain(c: char) -> bool {
    c.is_ascii_alphanumeric() || ".-".contains(c)
}

fn has_email(text: &str) -> bool {
    text.match_indices('@').any(|(at, _)| {
        let local = text[..at]
            .chars()
            .rev()
            .take_while(|c| is_email_local(*c))
            .count();
        let domain: String = text[at + 1..]
            .chars()
            .take_while(|c| is_email_domain(*c))
            .collect();
        let top_level = domain
            .rsplit_once('.')
            .map(|(_, end)| end)
            .filter(|end| end.len() >= 2 && end.chars().all(|c| c.is_ascii_alphabetic()));
        local > 0 && top_level.is_some()
    })
}

fn has_capture_folder(text: &str) -> bool {
    text.match_indices('-').any(|(at, _)| {
        let rest = &text[at + 1..];
        let hex = rest.chars().take_while(char::is_ascii_hexdigit).count();
        hex == CAPTURE_FOLDER_HEX_DIGITS
            && rest[hex..].chars().next().is_none_or(|next| next == '/')
    })
}

/// Runs of 5 or more printable characters, as UTF-8 (ASCII and other scripts) and as UTF-16LE
/// (ASCII).
fn readable_runs(bytes: &[u8]) -> Vec<String> {
    let mut runs = Vec::new();
    let mut push = |run: &mut String| {
        if run.chars().count() >= MIN_RUN_CHARS {
            runs.push(std::mem::take(run));
        }
        run.clear();
    };
    let mut run = String::new();
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if c.is_control() {
                push(&mut run);
            } else {
                run.push(c);
            }
        }
        if !chunk.invalid().is_empty() {
            push(&mut run);
        }
    }
    push(&mut run);
    let printable = |byte: u8| (b' '..=b'~').contains(&byte);
    for start in 0..2 {
        for pair in bytes[start.min(bytes.len())..].chunks(2) {
            if let [low, 0] = pair
                && printable(*low)
            {
                run.push(char::from(*low));
            } else {
                push(&mut run);
            }
        }
        push(&mut run);
    }
    runs
}

/// Whether `run` has the shape of a company or guild name (see [`MIN_NAME_WORDS`]).
fn has_two_words(run: &str) -> bool {
    let words: Vec<&str> = run
        .split(' ')
        .map(|token| token.trim_matches(WORD_PUNCTUATION))
        .filter(|word| !word.is_empty() && word.chars().all(char::is_alphabetic))
        .collect();
    let letters = run
        .chars()
        .filter(|c| c.is_alphabetic() || *c == ' ')
        .count();
    words.len() >= MIN_NAME_WORDS
        && words
            .iter()
            .any(|word| word.chars().count() >= MIN_LONG_WORD_LETTERS)
        && words.iter().map(|word| word.chars().count()).sum::<usize>() >= MIN_NAME_LETTERS
        && letters * PERCENT >= MIN_LETTER_PERCENT * run.chars().count()
}

/// Whether `run` holds an ID written as text (`8-4-4-4-12` hex digits).
fn has_id_text(run: &str) -> bool {
    let chars: Vec<char> = run.chars().collect();
    let len = ID_TEXT_GROUPS.iter().sum::<usize>() + ID_TEXT_GROUPS.len() - 1;
    chars.windows(len).any(|window| {
        let mut at = 0;
        ID_TEXT_GROUPS.iter().enumerate().all(|(index, group)| {
            let digits = window[at..at + group].iter().all(char::is_ascii_hexdigit);
            at += group;
            let dash = index + 1 == ID_TEXT_GROUPS.len() || window[at] == '-';
            at += 1;
            digits && dash
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::owner::tests::{owned_state, owner_member_hex};
    use super::*;

    const TABLE: &str = "town-props";

    fn rules(value: &Value) -> Vec<ScanRule> {
        let (hits, _) = scan_table(TABLE, value, &GameWords::default(), &[]).unwrap();
        hits.into_iter().map(|hit| hit.rule).collect()
    }

    fn hex_of(text: &str) -> String {
        let mut bytes = vec![0x01, 0x02];
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0x00);
        hex::encode(bytes)
    }

    #[test]
    fn a_player_id_anywhere_is_a_hit() {
        let id = vec![7; 16];
        let mut bytes = vec![1, 2, 3];
        bytes.extend_from_slice(&id);
        let table = json!({"forts": [{"position": hex::encode(bytes)}]});
        let (hits, _) = scan_table(TABLE, &table, &GameWords::default(), &[id]).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].rule, hits[0].path.as_str()),
            (ScanRule::PlayerId, "/forts/0/position")
        );
    }

    #[test]
    fn id_text_in_bytes_is_a_hit() {
        assert_eq!(
            rules(&json!({"plots": [{"body": hex_of("0123abcd-0000-1111-2222-333344445555")}]})),
            [ScanRule::IdText]
        );
        assert!(
            rules(&json!({"capital": "0123abcd-0000-1111-2222-333344445555"})).is_empty(),
            "plain JSON text is a world id"
        );
    }

    #[test]
    fn a_utf8_name_is_a_hit() {
        assert_eq!(
            rules(&json!({"props": [{"body": hex_of("Éclat Noir Ünion")}]})),
            [ScanRule::TwoWordText]
        );
    }

    #[test]
    fn any_hex_field_is_read() {
        assert_eq!(
            rules(&json!({"forts": [{"position": hex_of("Made Up Company")}]})),
            [ScanRule::TwoWordText]
        );
    }

    #[test]
    fn a_two_word_run_in_replicated_bytes_is_a_hit() {
        assert_eq!(
            rules(&json!({"props": [{"body": hex_of("I Made Up Company I")}]})),
            [ScanRule::TwoWordText]
        );
    }

    #[test]
    fn short_and_punctuated_words_still_make_a_name() {
        assert!(has_two_words("II Made II"));
        assert!(has_two_words("I. Madeup .I"));
        assert!(!has_two_words("xXIf&"));
        assert!(!has_two_words("C(DHSk "));
        assert!(!has_two_words("E\u{8821}D P"), "random bytes read as UTF-8");
    }

    #[test]
    fn a_utf16_two_word_run_is_a_hit() {
        let wide: Vec<u8> = "Made Up Company"
            .bytes()
            .flat_map(|byte| [byte, 0])
            .collect();
        assert_eq!(
            rules(&json!({"props": [{"body": hex::encode(wide)}]})),
            [ScanRule::TwoWordText]
        );
    }

    #[test]
    fn a_steam_id_is_a_hit() {
        assert_eq!(
            rules(&json!({"note": "76561190000000000"})),
            [ScanRule::SteamId]
        );
        assert!(
            rules(&json!({"note": "765611900000000001"})).is_empty(),
            "too long"
        );
    }

    #[test]
    fn an_email_is_a_hit() {
        assert_eq!(
            rules(&json!({"note": "someone@example.org"})),
            [ScanRule::Email]
        );
    }

    #[test]
    fn a_ledger_file_and_a_capture_folder_are_hits() {
        assert_eq!(
            rules(&json!({"note": "run-0123456789abcdef/x/ledger.bin"})),
            [ScanRule::LedgerFile, ScanRule::CaptureFolder]
        );
        assert_eq!(
            rules(&json!({"note": "run-0123456789abcdef"})),
            [ScanRule::CaptureFolder]
        );
    }

    #[test]
    fn a_named_field_is_a_hit() {
        assert_eq!(rules(&json!({"player_name": 1})), [ScanRule::NamedField]);
    }

    #[test]
    fn component_keys_are_not_hits() {
        assert!(rules(&json!({"ownership_component": 4})).is_empty());
    }

    #[test]
    fn ftue_offset_keys_are_not_hits() {
        assert!(rules(&json!({"player_facet_at": 4, "player_gde_ref_at": 8})).is_empty());
    }

    #[test]
    fn an_owner_state_with_identity_is_a_hit() {
        let table = json!({"props": [{"members": [owner_member_hex(owned_state())]}]});
        assert!(rules(&table).contains(&ScanRule::OwnerIdentity));
    }

    #[test]
    fn game_words_are_not_review_runs() {
        let words = GameWords::from_strings(["OutpostRushWood".to_owned()]);
        let table = json!({"props": [{"body": hex_of("OutpostRushWood")}]});
        let (hits, review) = scan_table(TABLE, &table, &words, &[]).unwrap();
        assert!(hits.is_empty());
        assert!(review.is_empty());
    }

    #[test]
    fn random_word_goes_to_review() {
        let table = json!({"props": [{"body": hex_of("Vszefq")}]});
        let (hits, review) = scan_table(TABLE, &table, &GameWords::default(), &[]).unwrap();
        assert!(hits.is_empty());
        assert_eq!(review.len(), 1);
        assert_eq!(review[0].text, "Vszefq");
        assert_eq!(review[0].path, "/props/0/body");
    }
}
