use crate::SeriesMatch;

/// Rank provider results against the parsed series guess: exact match first,
/// then prefix match, then substring match, then everything else — all
/// case-insensitive. Stable sort preserves the provider's own relevance
/// ordering within each tier.
pub fn rank_matches(guess: &str, matches: &[SeriesMatch]) -> Vec<SeriesMatch> {
    let guess_lower = guess.to_lowercase();
    let mut ranked: Vec<(u8, SeriesMatch)> = matches
        .iter()
        .cloned()
        .map(|m| {
            let name_lower = m.name.to_lowercase();
            let score = if name_lower == guess_lower {
                0
            } else if name_lower.starts_with(&guess_lower) {
                1
            } else if name_lower.contains(&guess_lower) {
                2
            } else {
                3
            };
            (score, m)
        })
        .collect();
    ranked.sort_by_key(|(score, _)| *score);
    ranked.into_iter().map(|(_, m)| m).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;

    fn m(name: &str) -> SeriesMatch {
        SeriesMatch {
            id: name.to_string(),
            name: name.to_string(),
            year: None,
            poster_url: None,
            provider: Provider::Tvmaze,
        }
    }

    #[test]
    fn exact_match_ranks_first() {
        let matches = vec![m("The Office (US)"), m("The Office"), m("Office Space")];
        let ranked = rank_matches("The Office", &matches);
        assert_eq!(ranked[0].name, "The Office");
    }
}
