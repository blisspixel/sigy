use scorer_probe::judge_records::{CalibrationControl, JudgmentOutcome, Quote};
use serde::Deserialize;

use crate::Result;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    status: String,
    source_quote: String,
    reference_quote: String,
    output_quote: String,
    reason: String,
}

pub fn literal_quote(text: &str, excerpt: &str) -> Result<Quote> {
    if excerpt.is_empty() {
        return Err("empty quote".into());
    }
    let mut matches = text.match_indices(excerpt);
    let (start, _) = matches.next().ok_or("invented quote")?;
    let first_scalar = excerpt.chars().next().ok_or("empty quote")?.len_utf8();
    if text[start + first_scalar..].contains(excerpt) {
        return Err("ambiguous quote".into());
    }
    Ok(Quote {
        start_utf8_byte: u32::try_from(start)?,
        end_utf8_byte: u32::try_from(start + excerpt.len())?,
        text: excerpt.into(),
    })
}

pub fn parse(bytes: &[u8], control: &CalibrationControl) -> Result<JudgmentOutcome> {
    let raw = std::str::from_utf8(bytes)?.trim();
    let raw = raw.strip_suffix("[end of text]").unwrap_or(raw).trim();
    let answer: Answer = serde_json::from_str(raw)?;
    if answer.reason.trim().is_empty()
        || answer.reason.len() > 512
        || answer.reason.chars().any(char::is_control)
        || [
            &answer.source_quote,
            &answer.reference_quote,
            &answer.output_quote,
        ]
        .iter()
        .any(|quote| quote.len() > 2048)
    {
        return Err("invalid bounded reason or quote".into());
    }
    match answer.status.as_str() {
        "critical" => Ok(JudgmentOutcome::Critical {
            source_quote: literal_quote(&control.source_text, &answer.source_quote)?,
            reference_quote: literal_quote(&control.english_reference, &answer.reference_quote)?,
            output_quote: if answer.output_quote.is_empty() {
                None
            } else {
                Some(literal_quote(&control.output_text, &answer.output_quote)?)
            },
            reason: answer.reason,
        }),
        "acceptable" | "abstained"
            if answer.source_quote.is_empty()
                && answer.reference_quote.is_empty()
                && answer.output_quote.is_empty() =>
        {
            Ok(if answer.status == "acceptable" {
                JudgmentOutcome::Acceptable {}
            } else {
                JudgmentOutcome::Abstained {
                    reason: answer.reason,
                }
            })
        }
        _ => Err("invalid status or extraneous quotes".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scorer_probe::judge_records::{Category, LabelBasis};

    fn control() -> CalibrationControl {
        CalibrationControl {
            control_id: "synthetic parser fixture".into(),
            clip_id: "not a frozen clip".into(),
            category: Category::Quantity,
            source_text: "L'année est 2020.".into(),
            english_reference: "The year is 2020.".into(),
            output_text: "The year is 2030.".into(),
            untrusted_context: String::new(),
            label_basis: LabelBasis::Unchanged {},
        }
    }

    fn answer(
        status: &str,
        source: &str,
        reference: &str,
        output: &str,
        reason: &str,
    ) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(
            &serde_json::json!({"status":status,"source_quote":source,"reference_quote":reference,"output_quote":output,"reason":reason}),
        )?)
    }

    #[test]
    fn response_schema_preserves_exact_unicode_citations_and_all_outcomes() -> Result<()> {
        let control = control();
        let bytes = answer(
            "critical",
            "2020",
            "2020",
            "2030",
            "Synthetic date-change parser fixture.",
        )?;
        let JudgmentOutcome::Critical {
            source_quote,
            reference_quote,
            output_quote,
            ..
        } = parse(&bytes, &control)?
        else {
            return Err("missing critical result".into());
        };
        source_quote.validate(&control.source_text)?;
        reference_quote.validate(&control.english_reference)?;
        output_quote
            .ok_or("missing output citation")?
            .validate(&control.output_text)?;
        assert_eq!(source_quote.start_utf8_byte, 13);
        assert!(matches!(
            parse(
                &answer(
                    "critical",
                    "2020",
                    "2020",
                    "",
                    "Synthetic omission fixture."
                )?,
                &control
            )?,
            JudgmentOutcome::Critical {
                output_quote: None,
                ..
            }
        ));
        assert!(matches!(
            parse(
                &answer("acceptable", "", "", "", "Synthetic accepted fixture.")?,
                &control
            )?,
            JudgmentOutcome::Acceptable {}
        ));
        let mut suffix = answer("abstained", "", "", "", "Synthetic uncertainty fixture.")?;
        suffix.extend_from_slice(b" [end of text]\r\n");
        assert!(matches!(
            parse(&suffix, &control)?,
            JudgmentOutcome::Abstained { .. }
        ));
        Ok(())
    }

    #[test]
    fn malformed_and_fabricated_results_never_become_accepted_outcomes() -> Result<()> {
        let control = control();
        for bytes in [
            answer("unknown", "", "", "", "Unknown status.")?,
            answer("acceptable", "2020", "", "", "Extraneous source quote.")?,
            answer("abstained", "", "2020", "", "Extraneous reference quote.")?,
            answer(
                "critical",
                "invented",
                "2020",
                "2030",
                "Invented source citation.",
            )?,
            answer(
                "critical",
                "2020",
                "invented",
                "2030",
                "Invented reference citation.",
            )?,
            answer(
                "critical",
                "2020",
                "2020",
                "invented",
                "Invented output citation.",
            )?,
            answer("critical", "", "2020", "2030", "Missing source evidence.")?,
            answer("abstained", "", "", "", " ")?,
            answer("abstained", "", "", "", "line\nbreak")?,
            answer("abstained", "", "", "", &"é".repeat(257))?,
        ] {
            assert!(parse(&bytes, &control).is_err());
        }
        let mut prose = answer("acceptable", "", "", "", "Complete object.")?;
        prose.extend_from_slice(b" and extra prose");
        assert!(parse(&prose, &control).is_err());
        assert!(parse(&[255], &control).is_err());
        Ok(())
    }
    #[test]
    fn quote_conversion_rejects_ambiguous_or_fabricated_evidence() -> Result<()> {
        let quote = literal_quote("é and 年", "年")?;
        assert_eq!(quote.start_utf8_byte, 7);
        quote.validate("é and 年")?;
        assert!(literal_quote("repeat repeat", "repeat").is_err());
        assert!(literal_quote("aaa", "aa").is_err());
        assert!(literal_quote("actual", "invented").is_err());
        assert!(literal_quote("actual", "").is_err());
        Ok(())
    }
    #[test]
    fn strict_answer_schema_rejects_duplicates_unknowns_and_trailing_prose() {
        for value in [
            "{\"status\":\"acceptable\",\"status\":\"critical\"}",
            "{\"status\":\"acceptable\",\"source_quote\":\"\",\"reference_quote\":\"\",\"output_quote\":\"\",\"reason\":\"ok\",\"hidden\":1}",
        ] {
            assert!(serde_json::from_str::<Answer>(value).is_err());
        }
    }
}
