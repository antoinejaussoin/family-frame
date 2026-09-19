/// Shared family-friendly skip lists for jokes and on-this-day facts.
pub fn is_family_friendly(text: &str, skip_terms: &[&str], skip_words: &[&str]) -> bool {
    let lower = text.to_ascii_lowercase();
    if skip_terms.iter().any(|term| lower.contains(term)) {
        return false;
    }
    !skip_words.iter().any(|word| contains_word(&lower, word))
}

fn contains_word(hay: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = hay[from..].find(word) {
        let at = from + rel;
        let before_ok = at == 0 || !hay.as_bytes()[at - 1].is_ascii_alphabetic();
        let end = at + word.len();
        let after_ok = end >= hay.len() || !hay.as_bytes()[end].is_ascii_alphabetic();
        if before_ok && after_ok {
            return true;
        }
        from = at + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_terms_and_whole_words() {
        assert!(!is_family_friendly("a battle began", &["battle"], &[]));
        assert!(is_family_friendly(
            "Marie Curie receives an award",
            &[],
            &["war"]
        ));
        assert!(!is_family_friendly("the war ended", &[], &["war"]));
    }
}
