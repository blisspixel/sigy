use std::collections::BTreeMap;

use serde::Serialize;

pub const SIGNATURE: &str = "chrF2++|case:mixed|eff:yes|nc:6|nw:2|space:no|raw-text-v1";
const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct Counts {
    pub hypothesis: u32,
    pub reference: u32,
    pub matched: u32,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct Statistics {
    pub orders: [Counts; 8],
}

impl Statistics {
    pub fn add(&mut self, other: &Self) {
        for (target, source) in self.orders.iter_mut().zip(&other.orders) {
            target.hypothesis += source.hypothesis;
            target.reference += source.reference;
            target.matched += source.matched;
        }
    }

    #[must_use]
    pub fn score(&self) -> f64 {
        let (mut precision, mut recall, mut effective) = (0.0, 0.0, 0_u32);
        for counts in &self.orders {
            if counts.hypothesis > 0 && counts.reference > 0 {
                precision += f64::from(counts.matched) / f64::from(counts.hypothesis);
                recall += f64::from(counts.matched) / f64::from(counts.reference);
                effective += 1;
            }
        }
        if effective == 0 || precision + recall == 0.0 {
            return 0.0;
        }
        precision /= f64::from(effective);
        recall /= f64::from(effective);
        500.0 * precision * recall / (4.0 * precision + recall)
    }
}

// The fixed Python str.split whitespace used by the reviewed upstream metric.
// This policy is independent of the ASR scorer's NFC normalization.
fn whitespace(character: char) -> bool {
    matches!(character, '\u{0009}'..='\u{000d}' | '\u{001c}'..='\u{0020}'
        | '\u{0085}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    for word in text.split(whitespace).filter(|word| !word.is_empty()) {
        if word.chars().count() > 1 {
            if let Some(last) = word.chars().last()
                && PUNCTUATION.contains(last)
            {
                words.push(word[..word.len() - last.len_utf8()].into());
                words.push(last.to_string());
                continue;
            }
            if let Some(first) = word.chars().next()
                && PUNCTUATION.contains(first)
            {
                words.push(first.to_string());
                words.push(word[first.len_utf8()..].into());
                continue;
            }
        }
        words.push(word.into());
    }
    words
}

fn ngrams<T: Clone + Ord>(units: &[T], order: usize) -> BTreeMap<Vec<T>, u32> {
    let mut counts = BTreeMap::new();
    if units.len() >= order {
        for window in units.windows(order) {
            *counts.entry(window.to_vec()).or_default() += 1;
        }
    }
    counts
}

fn counts<T: Clone + Ord>(hypothesis: &[T], reference: &[T], order: usize) -> Counts {
    let hypothesis = ngrams(hypothesis, order);
    let reference = ngrams(reference, order);
    Counts {
        // Upstream ignores hypothesis counts for an absent reference order.
        hypothesis: if reference.is_empty() {
            0
        } else {
            hypothesis.values().sum()
        },
        reference: reference.values().sum(),
        matched: hypothesis
            .iter()
            .map(|(gram, count)| (*count).min(reference.get(gram).copied().unwrap_or(0)))
            .sum(),
    }
}

// Caller admits at most 1024 scalars per string and 70 pairs per partition.
// Each aggregate order therefore remains below 71680, well inside u32.
#[must_use]
pub fn statistics(hypothesis: &str, reference: &str) -> Statistics {
    let hypothesis_chars: Vec<_> = hypothesis.chars().filter(|c| !whitespace(*c)).collect();
    let reference_chars: Vec<_> = reference.chars().filter(|c| !whitespace(*c)).collect();
    let hypothesis_words = words(hypothesis);
    let reference_words = words(reference);
    let mut statistics = Statistics::default();
    for (index, target) in statistics.orders.iter_mut().enumerate() {
        *target = if index < 6 {
            counts(&hypothesis_chars, &reference_chars, index + 1)
        } else {
            counts(&hypothesis_words, &reference_words, index - 5)
        };
    }
    statistics
}

#[cfg(test)]
mod tests {
    use super::{statistics, words};

    #[test]
    fn effective_order_and_hand_computed_word_penalty() {
        assert!((statistics("a", "a").score() - 100.0).abs() < 1e-12);
        assert!(statistics("", "reference").score().abs() < 1e-12);
        assert!(statistics("", "").score().abs() < 1e-12);
        assert!(statistics("dog", "cat").score().abs() < 1e-12);
        // Character orders match, but the unigram word order does not.
        assert!((statistics("a b c", "abc").score() - 75.0).abs() < 1e-12);
    }

    #[test]
    fn upstream_punctuation_and_fixed_whitespace() {
        assert_eq!(words("(hi) ! a,b"), ["(hi", ")", "!", "a,b"]);
        assert_eq!(words("a\u{001c}b\u{00a0}c"), ["a", "b", "c"]);
        assert!(statistics("é", "e\u{0301}").score().abs() < 1e-12);
        assert!(statistics("A", "a").score().abs() < 1e-12);
    }

    #[test]
    fn corpus_aggregates_counts_instead_of_sentence_means() {
        let mut aggregate = statistics("a", "a");
        aggregate.add(&statistics("abcdefgh", "xxxxxxxx"));
        assert!(aggregate.score() < 50.0);
        assert_eq!(aggregate.orders[0].hypothesis, 9);
        assert_eq!(aggregate.orders[0].reference, 9);
        assert_eq!(aggregate.orders[0].matched, 1);
    }

    #[test]
    fn hand_computed_all_eight_order_match() {
        // Character precision/recall: 3/6, 2/5, 1/4, 0, 0, 0.
        // Word orders: 1/2 and 0. All eight orders contribute: 1.65/8.
        assert!((statistics("the cat", "the dog").score() - 20.625).abs() < 1e-12);
    }

    #[test]
    fn largest_admitted_partition_counts_stay_exact() {
        let text = "x".repeat(1024);
        let pair = statistics(&text, &text);
        let mut total = super::Statistics::default();
        for _ in 0..70 {
            total.add(&pair);
        }
        assert_eq!(total.orders[0].hypothesis, 71680);
        assert_eq!(total.orders[0].matched, 71680);
        assert!((total.score() - 100.0).abs() < 1e-12);
    }
}
