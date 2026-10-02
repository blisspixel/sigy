//! Interpretation of untrusted provider responses into ledger outcomes.
//!
//! Only a successful response carrying both a generation ID and a usage cost
//! settles. Every other response keeps the full reservation as an uncertain
//! liability; only a failure before connection releases it.

use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};

use crate::{Result, client::Sent, money};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub prompt: Option<u64>,
    pub completion: Option<u64>,
    pub reasoning: Option<u64>,
    pub audio_prompt: Option<u64>,
    pub cached_prompt: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settlement {
    pub micro: u64,
    pub cost_text: String,
    pub generation_id: String,
    pub provider: String,
    pub served_model: String,
    pub usage: Usage,
    pub content: Option<String>,
    pub finish_reason: Option<String>,
    pub routing: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Interpreted {
    Settled(Box<Settlement>),
    Uncertain {
        reason: String,
        generation_id: Option<String>,
        status: Option<u16>,
    },
    NotSent(&'static str),
}

#[derive(Deserialize)]
struct Envelope<'a> {
    id: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    #[serde(borrow)]
    usage: Option<UsageWire<'a>>,
    choices: Option<Vec<Choice>>,
    openrouter_metadata: Option<Value>,
}

#[derive(Deserialize)]
struct UsageWire<'a> {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    #[serde(borrow)]
    cost: Option<&'a RawValue>,
    completion_tokens_details: Option<CompletionDetails>,
    prompt_tokens_details: Option<PromptDetails>,
}

#[derive(Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct PromptDetails {
    cached_tokens: Option<u64>,
    audio_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct Choice {
    message: Option<Message>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

/// Bound and sanitize untrusted provider text for receipts.
#[must_use]
pub fn label(text: &str, limit: usize) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(limit)
        .collect()
}

fn routing_summary(metadata: Option<&Value>) -> Option<String> {
    let metadata = metadata?;
    let mut summary = metadata
        .get("summary")
        .and_then(Value::as_str)
        .map(|text| label(text, 200))
        .unwrap_or_default();
    if let Some(attempt) = metadata.get("attempt").and_then(Value::as_u64) {
        summary.push_str("; attempt=");
        summary.push_str(&attempt.to_string());
    }
    if let Some(stages) = metadata.get("pipeline").and_then(Value::as_array) {
        let names: Vec<String> = stages
            .iter()
            .filter_map(|stage| stage.get("type").and_then(Value::as_str))
            .map(|name| label(name, 40))
            .collect();
        summary.push_str("; pipeline=[");
        summary.push_str(&names.join(","));
        summary.push(']');
    }
    Some(summary)
}

fn settle(body: &[u8]) -> Result<Settlement> {
    let envelope: Envelope<'_> = serde_json::from_slice(body)?;
    let generation_id = envelope
        .id
        .filter(|id| {
            !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .ok_or("missing generation id")?;
    let usage = envelope.usage.ok_or("missing usage")?;
    let cost_text = usage.cost.ok_or("missing usage cost")?.get().to_owned();
    let (micro, _) = money::reported(&cost_text)?;
    let choice = envelope
        .choices
        .and_then(|choices| choices.into_iter().next());
    let (content, finish_reason) = match choice {
        Some(choice) => (
            choice.message.and_then(|message| message.content),
            choice.finish_reason.map(|reason| label(&reason, 64)),
        ),
        None => (None, None),
    };
    Ok(Settlement {
        micro,
        cost_text,
        generation_id,
        provider: label(envelope.provider.as_deref().unwrap_or("unreported"), 128),
        served_model: label(envelope.model.as_deref().unwrap_or("unreported"), 128),
        usage: Usage {
            prompt: usage.prompt_tokens,
            completion: usage.completion_tokens,
            reasoning: usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens),
            audio_prompt: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.audio_tokens),
            cached_prompt: usage
                .prompt_tokens_details
                .and_then(|details| details.cached_tokens),
        },
        content,
        finish_reason,
        routing: routing_summary(envelope.openrouter_metadata.as_ref()),
    })
}

fn generation_id(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .map(str::to_owned)
}

#[must_use]
pub fn interpret(sent: &Sent) -> Interpreted {
    match sent {
        Sent::Unsent(reason) => Interpreted::NotSent(reason),
        Sent::Ambiguous(reason) => Interpreted::Uncertain {
            reason: (*reason).into(),
            generation_id: None,
            status: None,
        },
        Sent::Completed { status, body, .. } if *status == 200 => match settle(body) {
            Ok(settlement) => Interpreted::Settled(Box::new(settlement)),
            Err(error) => Interpreted::Uncertain {
                reason: label(&format!("unsettled-success: {error}"), 160),
                generation_id: generation_id(body),
                status: Some(*status),
            },
        },
        Sent::Completed { status, body, .. } => Interpreted::Uncertain {
            reason: format!("http-{status}"),
            generation_id: generation_id(body),
            status: Some(*status),
        },
    }
}

/// Bounded error message from a provider error envelope, for receipts only.
#[must_use]
pub fn error_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let message = value.get("error")?.get("message")?.as_str()?;
    Some(label(message, 300))
}

/// Reconcile one generation lookup into a settlement amount.
pub fn generation_cost(body: &[u8], expected_id: &str) -> Result<(u64, String, String)> {
    #[derive(Deserialize)]
    struct Lookup<'a> {
        #[serde(borrow)]
        data: Data<'a>,
    }
    #[derive(Deserialize)]
    struct Data<'a> {
        id: String,
        #[serde(borrow)]
        total_cost: &'a RawValue,
        provider_name: Option<String>,
    }
    let lookup: Lookup<'_> = serde_json::from_slice(body)?;
    if lookup.data.id != expected_id {
        return Err("generation lookup returned a different id".into());
    }
    let text = lookup.data.total_cost.get().to_owned();
    let (micro, _) = money::reported(&text)?;
    Ok((
        micro,
        text,
        label(
            lookup.data.provider_name.as_deref().unwrap_or("unreported"),
            128,
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed(status: u16, body: &str) -> Sent {
        Sent::Completed {
            status,
            body: body.as_bytes().to_vec(),
            elapsed_ms: 1,
        }
    }

    #[test]
    fn successful_usage_settles_with_provider_and_routing_evidence() {
        let body = r#"{"id":"gen-1","provider":"Google","model":"google/x","choices":[{"message":{"content":"{\"a\":1}"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"cost":1.5e-5,"completion_tokens_details":{"reasoning_tokens":3},"prompt_tokens_details":{"cached_tokens":0,"audio_tokens":4}},
            "openrouter_metadata":{"summary":"available=1, selected=Google","attempt":1,"pipeline":[{"type":"guardrail"}]}}"#;
        let Interpreted::Settled(settlement) = interpret(&completed(200, body)) else {
            panic!("expected settlement");
        };
        assert_eq!(settlement.micro, 15);
        assert_eq!(settlement.cost_text, "1.5e-5");
        assert_eq!(settlement.provider, "Google");
        assert_eq!(settlement.usage.reasoning, Some(3));
        assert_eq!(settlement.usage.audio_prompt, Some(4));
        assert_eq!(settlement.content.as_deref(), Some("{\"a\":1}"));
        assert_eq!(settlement.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            settlement.routing.as_deref(),
            Some("available=1, selected=Google; attempt=1; pipeline=[guardrail]")
        );
        let minimal = r#"{"id":"gen-2","usage":{"cost":0}}"#;
        let Interpreted::Settled(settlement) = interpret(&completed(200, minimal)) else {
            panic!("expected minimal settlement");
        };
        assert_eq!((settlement.micro, settlement.content), (0, None));
        assert_eq!(settlement.provider, "unreported");
    }

    #[test]
    fn missing_usage_errors_and_failures_keep_liability() {
        for (status, body, id) in [
            (200, r#"{"id":"gen-3","choices":[]}"#, Some("gen-3")),
            (
                200,
                r#"{"id":"gen-4","usage":{"cost":"0.1"}}"#,
                Some("gen-4"),
            ),
            (200, r#"{"id":"gen-5","usage":{"cost":-1}}"#, Some("gen-5")),
            (200, r#"{"usage":{"cost":0.1}}"#, None),
            (200, "not json", None),
            (
                402,
                r#"{"error":{"code":402,"message":"Insufficient credits"}}"#,
                None,
            ),
            (500, r#"{"id":"gen-6"}"#, Some("gen-6")),
        ] {
            let Interpreted::Uncertain {
                generation_id,
                status: seen,
                ..
            } = interpret(&completed(status, body))
            else {
                panic!("expected uncertainty for {body}");
            };
            assert_eq!(generation_id.as_deref(), id);
            assert_eq!(seen, Some(status));
        }
        assert!(matches!(
            interpret(&Sent::Ambiguous("timeout-after-send")),
            Interpreted::Uncertain { status: None, .. }
        ));
        assert_eq!(
            interpret(&Sent::Unsent("connect-failed")),
            Interpreted::NotSent("connect-failed")
        );
        assert_eq!(
            error_message(br#"{"error":{"message":"bad\nthing"}}"#).as_deref(),
            Some("bad thing")
        );
        assert_eq!(error_message(b"{}"), None);
    }

    #[test]
    fn generation_lookup_settles_only_the_matching_record() -> Result<()> {
        let body = br#"{"data":{"id":"gen-7","total_cost":0.0015,"provider_name":"Azure"}}"#;
        assert_eq!(
            generation_cost(body, "gen-7")?,
            (1500, "0.0015".into(), "Azure".into())
        );
        assert!(generation_cost(body, "gen-8").is_err());
        assert!(generation_cost(br#"{"data":{"id":"gen-7","total_cost":"1"}}"#, "gen-7").is_err());
        Ok(())
    }
}
