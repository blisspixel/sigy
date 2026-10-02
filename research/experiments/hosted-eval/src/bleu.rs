//! Corpus BLEU as the pinned sacrebleu 2.x defines it (13a tokenizer, maximum
//! order 4, exponential smoothing, no effective order for corpus scores).
//!
//! The frozen language scorer implements chrF++ and CER only. This module ports
//! the BLEU part of the disposable `mtscore` study binary, whose SHA-256 was
//! `c3425513996c4c20f362e4d185f637a6d34cac497dde6a6895161bc704a1b1a3` and whose
//! values matched sacrebleu commit `c596d9d2072a8f84200574a7a5c56c618e8d37e8`.
//! Its tests repeat that commit's published expected values.

use std::collections::HashMap;

/// Smoothing parameters. Reports use [`EXPONENTIAL`]; tests also reproduce the
/// upstream floor and add-k expected values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smoothing {
    pub exponential: bool,
    pub floor: f64,
    pub add_k: f64,
}

pub const EXPONENTIAL: Smoothing = Smoothing {
    exponential: true,
    floor: 0.0,
    add_k: 0.0,
};

/// Python `str.split()` whitespace: Unicode `White_Space` plus 0x1c to 0x1f.
fn python_space(character: char) -> bool {
    character.is_whitespace() || matches!(u32::from(character), 0x1c..=0x1f)
}

fn python_split(text: &str) -> Vec<&str> {
    text.split(python_space)
        .filter(|token| !token.is_empty())
        .collect()
}

fn isolate_symbols(line: &str) -> String {
    let mut output = String::with_capacity(line.len() * 2);
    for character in line.chars() {
        if matches!(character, '{'..='~' | '['..='`' | ' '..='&' | '('..='+' | ':'..='@' | '/') {
            output.push(' ');
            output.push(character);
            output.push(' ');
        } else {
            output.push(character);
        }
    }
    output
}

/// Apply one left-to-right, non-overlapping two-character rewrite.
fn pairs(
    text: &str,
    matches: impl Fn(char, char) -> bool,
    rewrite: impl Fn(char, char, &mut String),
) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len() * 2);
    let mut index = 0;
    while let Some(&current) = characters.get(index) {
        match characters.get(index + 1) {
            Some(&next) if matches(current, next) => {
                rewrite(current, next, &mut output);
                index += 2;
            }
            _ => {
                output.push(current);
                index += 1;
            }
        }
    }
    output
}

/// sacrebleu `Tokenizer13a`.
#[must_use]
pub fn tokenize_13a(line: &str) -> String {
    let mut text = line
        .replace("<skipped>", "")
        .replace("-\n", "")
        .replace('\n', " ");
    if text.contains('&') {
        text = text
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
    }
    let text = isolate_symbols(&format!(" {text} "));
    let text = pairs(
        &text,
        |current, next| !current.is_ascii_digit() && matches!(next, '.' | ','),
        |current, next, output| {
            output.push(current);
            output.push(' ');
            output.push(next);
            output.push(' ');
        },
    );
    let text = pairs(
        &text,
        |current, next| matches!(current, '.' | ',') && !next.is_ascii_digit(),
        |current, next, output| {
            output.push(' ');
            output.push(current);
            output.push(' ');
            output.push(next);
        },
    );
    let text = pairs(
        &text,
        |current, next| current.is_ascii_digit() && next == '-',
        |current, next, output| {
            output.push(current);
            output.push(' ');
            output.push(next);
            output.push(' ');
        },
    );
    python_split(&text).join(" ")
}

fn prepare(text: &str, thirteen: bool) -> String {
    let text = text.trim_end_matches(python_space);
    if thirteen {
        tokenize_13a(text)
    } else {
        text.to_owned()
    }
}

fn ngrams(line: &str) -> (HashMap<Vec<String>, u32>, u32) {
    let tokens: Vec<String> = python_split(line).into_iter().map(str::to_owned).collect();
    let mut counts = HashMap::new();
    for order in 1..=4 {
        for window in tokens.windows(order) {
            *counts.entry(window.to_vec()).or_insert(0) += 1;
        }
    }
    (counts, u32::try_from(tokens.len()).unwrap_or(u32::MAX))
}

/// Sufficient statistics: `[hypothesis length, reference length, matches x4, totals x4]`.
/// `thirteen` selects the 13a tokenizer; otherwise text is split on whitespace.
#[must_use]
pub fn statistics(hypothesis: &str, references: &[&str], thirteen: bool) -> [u32; 10] {
    let mut reference_counts: HashMap<Vec<String>, u32> = HashMap::new();
    let mut lengths = Vec::new();
    for reference in references {
        let (counts, length) = ngrams(&prepare(reference, thirteen));
        lengths.push(length);
        for (gram, count) in counts {
            let entry = reference_counts.entry(gram).or_insert(0);
            *entry = (*entry).max(count);
        }
    }
    let (counts, hypothesis_length) = ngrams(&prepare(hypothesis, thirteen));
    let mut closest: Option<(u32, u32)> = None;
    for length in lengths {
        let difference = hypothesis_length.abs_diff(length);
        closest = match closest {
            Some((best, best_length))
                if difference > best || (difference == best && length >= best_length) =>
            {
                Some((best, best_length))
            }
            _ => Some((difference, length)),
        };
    }
    let mut output = [0_u32; 10];
    output[0] = hypothesis_length;
    output[1] = closest.map_or(0, |(_, length)| length);
    for (gram, count) in &counts {
        let order = gram.len() - 1;
        output[6 + order] += count;
        if let Some(reference) = reference_counts.get(gram) {
            output[2 + order] += (*count).min(*reference);
        }
    }
    output
}

/// BLEU from summed sufficient statistics, on a 0 to 100 scale.
#[must_use]
pub fn score(statistics: &[u32; 10], smoothing: Smoothing, effective_order: bool) -> f64 {
    let system = f64::from(statistics[0]);
    let reference = f64::from(statistics[1]);
    let brevity = if system < reference {
        if system > 0.0 {
            (1.0 - reference / system).exp()
        } else {
            0.0
        }
    } else {
        1.0
    };
    if statistics[2..6].iter().all(|count| *count == 0) {
        return 0.0;
    }
    let mut precision = [0.0_f64; 4];
    let mut mteval = 1.0;
    let mut used = 4_u32;
    for (order, slot) in precision.iter_mut().enumerate() {
        let added = if order > 0 && smoothing.add_k > 0.0 {
            smoothing.add_k
        } else {
            0.0
        };
        let smoothed = added > 0.0;
        let correct = f64::from(statistics[2 + order]) + added;
        let total = f64::from(statistics[6 + order]) + added;
        if statistics[6 + order] == 0 && !smoothed {
            break;
        }
        if effective_order {
            used = u32::try_from(order + 1).unwrap_or(4);
        }
        if statistics[2 + order] == 0 && !smoothed {
            if smoothing.exponential {
                mteval *= 2.0;
                *slot = 100.0 / (mteval * total);
            } else {
                *slot = 100.0 * smoothing.floor / total;
            }
        } else {
            *slot = 100.0 * correct / total;
        }
    }
    let logarithm = |value: f64| {
        if value > 0.0 {
            value.ln()
        } else {
            -9_999_999_999.0
        }
    };
    let mean = precision
        .iter()
        .take(usize::try_from(used).unwrap_or(4))
        .map(|value| logarithm(*value))
        .sum::<f64>()
        / f64::from(used);
    brevity * mean.exp()
}

/// sacrebleu `corpus_bleu` defaults: 13a, exponential smoothing, no effective order.
#[must_use]
pub fn corpus(pairs: &[(&str, &str)]) -> f64 {
    let mut sums = [0_u32; 10];
    for (hypothesis, reference) in pairs {
        for (sum, value) in sums
            .iter_mut()
            .zip(statistics(hypothesis, &[reference], true))
        {
            *sum += value;
        }
    }
    score(&sums, EXPONENTIAL, false)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the sacrebleu test suite and README at commit
    //! c596d9d2072a8f84200574a7a5c56c618e8d37e8.
    use super::*;

    fn close(actual: f64, expected: f64, epsilon: f64) {
        assert!(
            (actual - expected).abs() < epsilon,
            "got {actual}, expected {expected}"
        );
    }

    const NONE: Smoothing = Smoothing {
        exponential: false,
        floor: 0.0,
        add_k: 0.0,
    };

    fn floor(value: f64) -> Smoothing {
        Smoothing {
            floor: value,
            ..NONE
        }
    }

    fn add_k(value: f64) -> Smoothing {
        Smoothing {
            add_k: value,
            ..NONE
        }
    }

    const REFERENCES: [[&str; 3]; 2] = [
        [
            "The dog bit the man.",
            "It was not unexpected.",
            "The man bit him first.",
        ],
        [
            "The dog had bit the man.",
            "No one was surprised.",
            "The man had bitten the dog.",
        ],
    ];
    const SYSTEM: [&str; 3] = [
        "The dog bit the man.",
        "It wasn't surprising.",
        "The man had just bitten him.",
    ];

    fn multi(first: &str, thirteen: bool, smoothing: Smoothing) -> f64 {
        let mut sums = [0_u32; 10];
        for index in 0..3 {
            let reference = if index == 0 {
                first
            } else {
                REFERENCES[0][index]
            };
            let values = statistics(SYSTEM[index], &[reference, REFERENCES[1][index]], thirteen);
            for (sum, value) in sums.iter_mut().zip(values) {
                *sum += value;
            }
        }
        score(&sums, smoothing, false)
    }

    #[test]
    fn readme_multi_reference_values() {
        close(multi(REFERENCES[0][0], true, EXPONENTIAL), 48.530_827, 1e-6);
        close(
            multi(REFERENCES[0][0], false, EXPONENTIAL),
            49.191_956_6,
            1e-6,
        );
        close(multi(REFERENCES[0][0], true, NONE), 48.530_827, 1e-6);
        close(multi("", true, EXPONENTIAL), 29.44, 0.005);
        close(corpus(&[("", "The dog bit the man.")]), 0.0, 1e-9);
        close(
            corpus(&[("The dog bit the man.", "The dog bit the man.")]),
            100.0,
            1e-9,
        );
    }

    #[test]
    fn raw_corpus_and_sentence_cases() {
        let raw = |hypothesis: &str, reference: &str, value: f64| {
            score(
                &statistics(hypothesis, &[reference], false),
                floor(value),
                true,
            ) / 100.0
        };
        close(raw("this is a test", "this is a test", 0.01), 1.0, 1e-8);
        close(
            raw("this is a fest", "this is a test", 0.01),
            0.223_606_797_749_979,
            1e-8,
        );
        close(raw("test", "a test", 0.01), 0.367_879_441_171_442_5, 1e-8);
        close(
            raw("a little test", "a test", 0.01),
            0.032_182_979_486_854_33,
            1e-8,
        );
        close(
            raw(
                "am I am a character sequence",
                "I am a symbol string sequence a a",
                0.1,
            ),
            0.155_572_218_2,
            1e-8,
        );
        close(
            raw(
                "am I am a character sequence",
                "I am a symbol string sequence a a",
                0.0,
            ),
            0.0,
            1e-8,
        );
        let values = statistics(
            "am I am a character sequence",
            &["I am a symbol string sequence a a"],
            false,
        );
        assert_eq!(&values[2..], &[4, 2, 1, 0, 6, 5, 4, 3]);
        close(
            score(&[11, 11, 9, 7, 5, 3, 10, 8, 6, 4], NONE, false) / 100.0,
            0.837_592_239_7,
            1e-8,
        );
        let reference = "producţia de zahăr brut se exprimă în zahăr alb;";
        let hypothesis =
            "Producția de zahăr primă va fi exprimată în ceea ce privește zahărul alb;";
        for (smoothing, thirteen, expected) in [
            (EXPONENTIAL, true, 8.493),
            (NONE, true, 0.0),
            (floor(0.1), true, 4.516_88),
            (floor(0.5), true, 10.10),
            (add_k(1.0), true, 14.882),
            (add_k(2.0), true, 21.389),
            (EXPONENTIAL, false, 7.347),
        ] {
            close(
                score(
                    &statistics(hypothesis, &[reference], thirteen),
                    smoothing,
                    true,
                ),
                expected,
                1e-3,
            );
        }
        close(
            score(
                &statistics("this is a cat", &["okay thanks"], true),
                EXPONENTIAL,
                true,
            ),
            0.0,
            1e-9,
        );
    }

    #[test]
    fn tokenizer_13a_examples() {
        assert_eq!(
            tokenize_13a("It wasn't surprising."),
            "It wasn't surprising ."
        );
        assert_eq!(tokenize_13a("on July 1, 2020\"."), "on July 1 , 2020 \" .");
        assert_eq!(
            tokenize_13a("3.5 and 1,000 in 1990-2000"),
            "3.5 and 1,000 in 1990 - 2000"
        );
        assert_eq!(tokenize_13a("a &amp; b <skipped>x-\ny"), "a & b xy");
    }
}
