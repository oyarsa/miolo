//! Substring search over column names and field content.
//!
//! Deliberately plain substring matching rather than fuzzy: `/ship` behaves
//! the way it does in vim, and a column list is short enough that fuzziness
//! would surprise more often than it would help.

/// Indices of every entry containing `term`, case-insensitively.
pub fn find(haystack: &[String], term: &str) -> Vec<usize> {
    if term.is_empty() {
        return Vec::new();
    }
    let needle = term.to_lowercase();
    haystack
        .iter()
        .enumerate()
        .filter(|(_, item)| item.to_lowercase().contains(&needle))
        .map(|(index, _)| index)
        .collect()
}

/// The match at or after `current`, wrapping around at the end.
///
/// `current` itself is skipped so repeated `n` presses advance rather than
/// sticking on the match already selected.
pub fn step(matches: &[usize], current: usize, forward: bool) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    if forward {
        matches
            .iter()
            .find(|&&m| m > current)
            .or_else(|| matches.first())
            .copied()
    } else {
        matches
            .iter()
            .rev()
            .find(|&&m| m < current)
            .or_else(|| matches.last())
            .copied()
    }
}

/// The first match at or after `from`, used when a search is first submitted.
pub fn first_from(matches: &[usize], from: usize) -> Option<usize> {
    matches
        .iter()
        .find(|&&m| m >= from)
        .or_else(|| matches.first())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<String> {
        ["order_id", "customer", "notes", "shipped_at", "ship_cost"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    #[test]
    fn finds_substrings_case_insensitively() {
        assert_eq!(find(&columns(), "ship"), [3, 4]);
        assert_eq!(find(&columns(), "SHIP"), [3, 4]);
        assert_eq!(find(&columns(), "Notes"), [2]);
    }

    #[test]
    fn empty_term_matches_nothing() {
        assert!(find(&columns(), "").is_empty());
    }

    #[test]
    fn missing_term_matches_nothing() {
        assert!(find(&columns(), "zzz").is_empty());
    }

    #[test]
    fn step_advances_past_the_current_match() {
        let matches = vec![3, 4];
        assert_eq!(step(&matches, 3, true), Some(4));
        assert_eq!(step(&matches, 0, true), Some(3));
    }

    #[test]
    fn step_wraps_at_both_ends() {
        let matches = vec![3, 4];
        assert_eq!(step(&matches, 4, true), Some(3), "wraps forward");
        assert_eq!(step(&matches, 3, false), Some(4), "wraps backward");
    }

    #[test]
    fn step_goes_backward() {
        let matches = vec![1, 3, 5];
        assert_eq!(step(&matches, 5, false), Some(3));
        assert_eq!(step(&matches, 3, false), Some(1));
    }

    #[test]
    fn step_on_no_matches_is_none() {
        assert_eq!(step(&[], 0, true), None);
    }

    #[test]
    fn first_from_prefers_at_or_after() {
        let matches = vec![2, 5];
        assert_eq!(first_from(&matches, 0), Some(2));
        assert_eq!(first_from(&matches, 2), Some(2), "inclusive");
        assert_eq!(first_from(&matches, 3), Some(5));
        assert_eq!(first_from(&matches, 9), Some(2), "wraps");
    }
}
