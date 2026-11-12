//! The fuzzy search: the letters of the query, in order, in the text. A letter that follows the
//! last match, or that starts a word, gives more points.

/// A match: its score, and the positions (in chars) of the matched letters.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub score: i32,
    pub positions: Vec<usize>,
}

/// The match of a query in a text, or None. An empty query matches each text with a score of 0.
/// The search ignores the case.
pub fn find(query: &str, text: &str) -> Option<Match> {
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }
    let text: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut positions = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut start = 0;
    for wanted in &query {
        let found = (start..lower.len()).find(|&index| lower[index] == *wanted)?;
        let follows = positions
            .last()
            .is_some_and(|&last: &usize| last + 1 == found);
        score += if follows { 6 } else { 1 };
        if found == 0 || is_separator(text[found - 1]) {
            score += 4;
        }
        positions.push(found);
        start = found + 1;
    }
    // A short text with the same letters is a better match.
    score -= (text.len() / 20) as i32;
    Some(Match { score, positions })
}

fn is_separator(character: char) -> bool {
    character.is_whitespace() || "-_./·▸:".contains(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_letters_must_come_in_order() {
        assert!(find("atl", "atlas").is_some());
        assert!(find("lta", "atlas").is_none());
    }

    #[test]
    fn the_search_ignores_the_case() {
        assert_eq!(find("ATL", "atlas").unwrap().positions, [0, 1, 2]);
        assert_eq!(find("atl", "ATLAS").unwrap().positions, [0, 1, 2]);
    }

    #[test]
    fn letters_in_a_row_and_word_starts_give_more_points() {
        let together = find("fix", "fix-login").unwrap().score;
        let spread = find("fix", "far index").unwrap().score;
        assert!(together > spread);
        let word_start = find("lo", "fix-login").unwrap().score;
        let inside = find("lo", "follow").unwrap().score;
        assert!(word_start > inside);
    }

    #[test]
    fn an_empty_query_matches_all() {
        assert_eq!(find("", "anything").unwrap().score, 0);
    }

    #[test]
    fn the_positions_count_chars_not_bytes() {
        assert_eq!(find("fix", "atlas ▸ fix").unwrap().positions, [8, 9, 10]);
    }
}
