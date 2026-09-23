use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

pub const NORMALIZATION_ID: &str = "nfc17-fixed-white-space-v1";
pub const MAX_TEXT_BYTES: usize = 8192;
pub const MAX_SCALARS: usize = 1024;
pub const MAX_TOKENS: usize = 512;
pub const MAX_ALIGNMENT_CELLS: usize = 32_000_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Rule {
    pub id: &'static str,
    pub unicode_normalization_version: &'static str,
    pub primary_metric: &'static str,
    pub transformation: &'static str,
    pub language_note: &'static str,
    pub wer_limitation: &'static str,
}

pub fn rule(config: &str) -> Result<Rule, String> {
    let (primary, note, limitation) = match config {
        "ar_eg" => (
            "wer_with_cer",
            "Keep Arabic harakat, tatweel, alef/hamza variants, punctuation and original script.",
            "Whitespace tokens do not split Arabic clitics; no morphological segmentation.",
        ),
        "cmn_hans_cn" => (
            "cer",
            "Keep Han script, Latin text, digits and punctuation; no simplified/traditional conversion.",
            "Diagnostic only: whitespace tokens are not Mandarin words; an unspaced sentence may be one token.",
        ),
        "en_us" => (
            "wer_with_cer",
            "Keep case, punctuation, apostrophes and digit spelling.",
            "Whitespace tokens retain punctuation and contractions; not a standard benchmark-normalized WER.",
        ),
        "es_419" => (
            "wer_with_cer",
            "Keep case, accents, n-tilde, inverted punctuation and digit spelling.",
            "Whitespace tokens retain punctuation and clitics; regional vocabulary is not normalized.",
        ),
        "fr_fr" => (
            "wer_with_cer",
            "Keep case, accents, ligatures, apostrophes and punctuation; no Canadian French substitution.",
            "Whitespace tokens retain elisions and hyphens; this subset does not measure Canadian French.",
        ),
        "hi_in" => (
            "wer_with_cer",
            "Keep Devanagari vowel signs, nukta, virama, joiners, punctuation and original script.",
            "Whitespace tokens are not a morphological analysis; CER counts scalars, not grapheme clusters.",
        ),
        "pt_br" => (
            "wer_with_cer",
            "Keep case, accents, cedilla, punctuation and digit spelling.",
            "Whitespace tokens retain contractions and hyphens; no spelling or regional normalization.",
        ),
        "sw_ke" => (
            "wer_with_cer",
            "Keep case, punctuation, apostrophes and original spelling.",
            "Whitespace tokens do not split Swahili morphology; no stemming or affix normalization.",
        ),
        _ => return Err("no frozen normalization rule for config".into()),
    };
    Ok(Rule {
        id: NORMALIZATION_ID,
        unicode_normalization_version: "17.0.0 via unicode-normalization 0.1.25",
        primary_metric: primary,
        transformation: "NFC; replace fixed Unicode White_Space runs with one ASCII space; trim those runs at both ends. CER includes remaining spaces and counts Unicode scalar values. WER splits on that ASCII space. Preserve case, accents, punctuation, scripts, joiners and format characters. No NFKC, case folding, transliteration, stemming or numeral rewriting.",
        language_note: note,
        wer_limitation: limitation,
    })
}

// Fixed Unicode White_Space property values; do not depend on changing std tables.
fn whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

pub fn normalize(text: &str) -> Result<String, String> {
    if text.len() > MAX_TEXT_BYTES || text.chars().count() > MAX_SCALARS {
        return Err("raw text exceeds scorer byte or scalar limit".into());
    }
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in text.nfc() {
        if whitespace(character) {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    if normalized.chars().count() > MAX_SCALARS || tokens(&normalized).len() > MAX_TOKENS {
        return Err("normalized text exceeds scorer scalar or token limit".into());
    }
    Ok(normalized)
}

fn tokens(text: &str) -> Vec<&str> {
    text.split(' ').filter(|token| !token.is_empty()).collect()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Edits {
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
}

impl Edits {
    pub const fn errors(self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }
}

// Unit costs; deterministic ties prefer diagonal, then deletion, then insertion.
// Two rows bound memory. Inputs are already bounded by normalize and score admission.
fn alignment<T: Eq>(reference: &[T], candidate: &[T]) -> Edits {
    let mut previous: Vec<_> = (0..=candidate.len())
        .map(|insertions| Edits {
            insertions,
            ..Edits::default()
        })
        .collect();
    let mut current = vec![Edits::default(); candidate.len() + 1];
    for (row, reference_unit) in reference.iter().enumerate() {
        current[0] = Edits {
            deletions: row + 1,
            ..Edits::default()
        };
        for (column, candidate_unit) in candidate.iter().enumerate() {
            let mut diagonal = previous[column];
            diagonal.substitutions += usize::from(reference_unit != candidate_unit);
            let mut deletion = previous[column + 1];
            deletion.deletions += 1;
            let mut insertion = current[column];
            insertion.insertions += 1;
            let mut best = diagonal;
            if deletion.errors() < best.errors() {
                best = deletion;
            }
            if insertion.errors() < best.errors() {
                best = insertion;
            }
            current[column + 1] = best;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[candidate.len()]
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Metric {
    pub edits: Edits,
    pub reference_units: usize,
    pub candidate_units: usize,
    pub errors: usize,
    pub rate: Option<Fraction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Fraction {
    pub numerator: usize,
    pub denominator: usize,
}

impl Metric {
    fn measured<T: Eq>(reference: &[T], candidate: &[T]) -> Self {
        let edits = alignment(reference, candidate);
        let mut metric = Self {
            edits,
            reference_units: reference.len(),
            candidate_units: candidate.len(),
            errors: edits.errors(),
            rate: None,
        };
        metric.refresh_rate();
        metric
    }

    fn refresh_rate(&mut self) {
        self.rate = (self.reference_units != 0).then_some(Fraction {
            numerator: self.errors,
            denominator: self.reference_units,
        });
    }

    fn add(&mut self, other: &Self) {
        self.edits.substitutions += other.edits.substitutions;
        self.edits.deletions += other.edits.deletions;
        self.edits.insertions += other.edits.insertions;
        self.reference_units += other.reference_units;
        self.candidate_units += other.candidate_units;
        self.errors += other.errors;
        self.refresh_rate();
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Metrics {
    pub cer: Metric,
    pub whitespace_wer: Metric,
}

impl Metrics {
    pub fn measured(reference: &str, candidate: &str) -> Self {
        Self {
            cer: Metric::measured(
                &reference.chars().collect::<Vec<_>>(),
                &candidate.chars().collect::<Vec<_>>(),
            ),
            whitespace_wer: Metric::measured(&tokens(reference), &tokens(candidate)),
        }
    }

    pub fn add(&mut self, other: &Self) {
        self.cer.add(&other.cer);
        self.whitespace_wer.add(&other.whitespace_wer);
    }
}

pub fn alignment_cells(reference: &str, candidate: &str) -> usize {
    reference.chars().count() * candidate.chars().count()
        + tokens(reference).len() * tokens(candidate).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omission_insertion_and_substitution_have_exact_counts() {
        let omission = Metrics::measured("one two three", "one three");
        assert_eq!(omission.whitespace_wer.edits.deletions, 1);
        assert_eq!(omission.whitespace_wer.reference_units, 3);
        let insertion = Metrics::measured("one two", "one new two");
        assert_eq!(insertion.whitespace_wer.edits.insertions, 1);
        let substitution = Metrics::measured("one two", "one three");
        assert_eq!(substitution.whitespace_wer.edits.substitutions, 1);
        assert_eq!(Metrics::measured("kitten", "sitting").cer.errors, 3);
    }

    #[test]
    fn combining_forms_match_but_accents_scripts_and_format_chars_remain() -> Result<(), String> {
        let composed = normalize("café")?;
        let decomposed = normalize("cafe\u{301}")?;
        assert_eq!(composed, decomposed);
        assert_eq!(Metrics::measured(&composed, &decomposed).cer.errors, 0);
        for text in ["عَرَبِيّ", "हिन्दी", "汉字", "ǹ", "a\u{200d}b", "Ａ"]
        {
            assert!(!normalize(text)?.is_ascii());
        }
        assert_ne!(normalize("café")?, normalize("cafe")?);
        assert_ne!(normalize("A")?, normalize("a")?);
        assert_ne!(normalize("a\u{200b}b")?, normalize("ab")?);
        Ok(())
    }

    #[test]
    fn fixed_whitespace_is_collapsed_trimmed_and_counted_in_cer() -> Result<(), String> {
        assert_eq!(normalize("\t a\u{00a0}\n b\u{3000}")?, "a b");
        let metrics = Metrics::measured("a b", "ab");
        assert_eq!(metrics.cer.reference_units, 3);
        assert_eq!(metrics.cer.edits.deletions, 1);
        Ok(())
    }

    #[test]
    fn empty_references_never_claim_zero_rate() {
        let empty = Metrics::measured("", "");
        assert_eq!(empty.cer.errors, 0);
        assert_eq!(empty.cer.rate, None);
        let inserted = Metrics::measured("", "hi");
        assert_eq!(inserted.cer.edits.insertions, 2);
        assert_eq!(inserted.cer.rate, None);
        assert_eq!(inserted.whitespace_wer.edits.insertions, 1);
        assert_eq!(inserted.whitespace_wer.rate, None);
        let omitted = Metrics::measured("hi", "");
        assert_eq!(omitted.cer.edits.deletions, 2);
    }

    #[test]
    fn edit_ties_and_rates_over_one_are_stable() {
        let tied = Metrics::measured("ab", "ba");
        assert_eq!(tied.cer.edits.substitutions, 2);
        let inserted = Metrics::measured("a", "aaaa");
        assert_eq!(
            inserted.cer.rate,
            Some(Fraction {
                numerator: 3,
                denominator: 1
            })
        );
    }

    #[test]
    fn mandarin_uses_cer_and_every_config_has_a_rule() -> Result<(), String> {
        for config in crate::selection::CONFIGS {
            assert_eq!(rule(config)?.id, NORMALIZATION_ID);
        }
        assert_eq!(rule("cmn_hans_cn")?.primary_metric, "cer");
        assert!(
            rule("cmn_hans_cn")?
                .wer_limitation
                .contains("Diagnostic only")
        );
        let metrics = Metrics::measured("你好世界", "你好世间");
        assert_eq!(metrics.cer.reference_units, 4);
        assert_eq!(metrics.cer.errors, 1);
        assert_eq!(metrics.whitespace_wer.reference_units, 1);
        assert_eq!(metrics.whitespace_wer.errors, 1);
        assert!(rule("fr_ca").is_err());
        Ok(())
    }

    #[test]
    fn work_limits_fail_closed() {
        assert!(normalize(&"a".repeat(MAX_SCALARS + 1)).is_err());
        assert!(normalize(&" a".repeat(MAX_TOKENS + 1)).is_err());
        assert!(normalize(&"é".repeat(MAX_TEXT_BYTES)).is_err());
    }

    // Independent exhaustive recursive oracle for short binary strings.
    fn oracle(reference: &[u8], candidate: &[u8]) -> usize {
        match (reference.split_first(), candidate.split_first()) {
            (None, _) => candidate.len(),
            (_, None) => reference.len(),
            (Some((left, reference_tail)), Some((right, candidate_tail))) => {
                let diagonal = oracle(reference_tail, candidate_tail) + usize::from(left != right);
                let deletion = oracle(reference_tail, candidate) + 1;
                let insertion = oracle(reference, candidate_tail) + 1;
                diagonal.min(deletion).min(insertion)
            }
        }
    }

    #[test]
    fn two_row_alignment_matches_independent_short_string_oracle() {
        let sequences = [
            "", "a", "b", "aa", "ab", "ba", "bb", "aaa", "aab", "aba", "abb", "baa", "bab", "bba",
            "bbb",
        ];
        for reference in sequences {
            for candidate in sequences {
                let edits = alignment(reference.as_bytes(), candidate.as_bytes());
                assert_eq!(
                    edits.errors(),
                    oracle(reference.as_bytes(), candidate.as_bytes())
                );
                assert_eq!(
                    reference.len() + edits.insertions,
                    candidate.len() + edits.deletions
                );
            }
        }
    }
}
