// SPDX-License-Identifier: AGPL-3.0-or-later
//! The profanity filter: the word lists the client ships (`dictionary/<locale>/profanity.csv` in
//! the game paks) with the rules their header states: one word per line, `#` starts a comment, a
//! leading `_` matches only at the start of a word and a trailing `_` only at its end, and a word
//! that is part of another comes first. Each match is replaced by one `*` per character, as the
//! retail lines show; the rest of the text stays. Letter case does not matter.
//!
//! Retail starred words that these lists do not hold (22 retail lines checked: none of their
//! starred words is in any shipped list), so the retail server used another list, which is
//! Unknown; this filter is the client's.

/// Starts a comment line.
const COMMENT: char = '#';
/// Marks a word boundary at the start or the end of a list word.
const BOUNDARY: char = '_';
/// Replaces each character of a match.
const STAR: char = '*';

/// One list word.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    /// The word, lowercase, as characters.
    text: Vec<char>,
    /// It matches only at the start of a word.
    at_start: bool,
    /// It matches only at the end of a word.
    at_end: bool,
}

/// The words of all lists, in list order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfanityFilter {
    words: Vec<Word>,
}

impl ProfanityFilter {
    /// The filter of the list files `lists` (each the bytes of one `profanity.csv`; UTF-8, or
    /// Latin-1 as most non-English lists are).
    pub fn of_lists<'a>(lists: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let words = lists
            .into_iter()
            .flat_map(|bytes| parse_list(&decode_list(bytes)))
            .collect();
        Self { words }
    }

    /// How many words the filter has.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Whether the filter has no word.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// `text` with every listed word starred; `None` when it has none.
    pub fn filter(&self, text: &str) -> Option<String> {
        let mut chars: Vec<char> = text.chars().collect();
        let lower: Vec<char> = chars
            .iter()
            .map(|letter| letter.to_lowercase().next().unwrap_or(*letter))
            .collect();
        let mut found = false;
        for word in &self.words {
            let len = word.text.len();
            if len == 0 || len > lower.len() {
                continue;
            }
            for start in 0..=lower.len() - len {
                if lower[start..start + len] != word.text[..] {
                    continue;
                }
                let starts = start == 0 || lower[start - 1].is_whitespace();
                let ends = start + len == lower.len() || lower[start + len].is_whitespace();
                if (!word.at_start || starts) && (!word.at_end || ends) {
                    chars[start..start + len].fill(STAR);
                    found = true;
                }
            }
        }
        found.then(|| chars.into_iter().collect())
    }
}

/// The text of a list file: UTF-8 (with or without its byte-order mark), else Latin-1.
fn decode_list(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.trim_start_matches('\u{feff}').to_owned(),
        Err(_) => bytes.iter().map(|byte| char::from(*byte)).collect(),
    }
}

fn parse_list(text: &str) -> Vec<Word> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(COMMENT))
        .filter_map(|line| {
            let at_start = line.starts_with(BOUNDARY);
            let at_end = line.ends_with(BOUNDARY) && line.len() > 1;
            let core = line.trim_matches(BOUNDARY);
            (!core.is_empty()).then(|| Word {
                text: core.chars().flat_map(char::to_lowercase).collect(),
                at_start,
                at_end,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list in the shipped form: a byte-order mark, comments, CRLF lines, boundary words.
    const LIST: &[u8] =
        "\u{feff}# comment\r\n# - ONLY ONE WORD PER LINE.\r\n_bad_\r\nrot\r\n_pre\r\n".as_bytes();

    fn filter() -> ProfanityFilter {
        ProfanityFilter::of_lists([LIST])
    }

    #[test]
    fn the_list_rules_of_the_shipped_header_hold() {
        let filter = filter();
        assert_eq!(filter.len(), 3);
        assert_eq!(filter.filter("a bad day"), Some("a *** day".to_owned()));
        assert_eq!(
            filter.filter("badge"),
            None,
            "a boundary word inside another"
        );
        assert_eq!(filter.filter("Carrots"), Some("Car***s".to_owned()));
        assert_eq!(
            filter.filter("prefix and unpre"),
            Some("***fix and unpre".to_owned())
        );
        assert_eq!(
            filter.filter("BAD"),
            Some("***".to_owned()),
            "any letter case"
        );
        assert_eq!(filter.filter("fine"), None);
    }

    #[test]
    fn a_latin1_list_reads() {
        let list = [b'm', 0xfc, b'l', b'l', b'\n'];
        let filter = ProfanityFilter::of_lists([list.as_slice()]);
        assert_eq!(filter.filter("Müll!"), Some("****!".to_owned()));
    }

    #[test]
    fn stars_keep_the_character_count_of_wide_letters() {
        let filter = ProfanityFilter::of_lists(["ü".as_bytes()]);
        assert_eq!(filter.filter("xüx"), Some("x*x".to_owned()));
    }
}
