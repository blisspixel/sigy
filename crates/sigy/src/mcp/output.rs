use serde_json::{Value, json};

fn source(name: &str) -> Option<&'static str> {
    match name {
        "doctor" => Some(include_str!("../../../../docs/schemas/doctor.schema.json")),
        "analysis_search" => Some(include_str!(
            "../../../../docs/schemas/analysis_search.schema.json"
        )),
        "monitor_matches" => Some(include_str!(
            "../../../../docs/schemas/monitor_matches.schema.json"
        )),
        "monitor_coverage" => Some(include_str!(
            "../../../../docs/schemas/monitor_coverage.schema.json"
        )),
        "analysis_transcript" => Some(include_str!(
            "../../../../docs/schemas/analysis_transcript.schema.json"
        )),
        "analysis_translation" => Some(include_str!(
            "../../../../docs/schemas/analysis_translation.schema.json"
        )),
        _ => None,
    }
}

pub(super) fn schema(name: &str) -> Result<Option<Value>, String> {
    source(name)
        .map(|text| {
            serde_json::from_str(text)
                .map_err(|_| format!("invalid embedded output schema for {name}"))
        })
        .transpose()
}

/// Keep CLI snapshots verbatim, including a failed doctor's check report.
/// Failures without JSON stdout use the CLI's existing `{error: string}` shape.
pub(super) fn complete(name: &str, mut result: Value) -> Value {
    if source(name).is_none() || result.get("structuredContent").is_some() {
        return result;
    }
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or("command failed");
    let structured = if result["isError"] == true {
        serde_json::from_str::<Value>(text)
            .ok()
            .filter(|value| {
                value.as_object().is_some_and(|object| {
                    object.len() == 1 && object.get("error").is_some_and(Value::is_string)
                })
            })
            .unwrap_or_else(|| json!({"error": text}))
    } else {
        json!({"error": "sigy command returned no JSON output"})
    };
    result["structuredContent"] = structured;
    result["isError"] = json!(true);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_preserve_cli_error_json_and_adapter_messages() {
        for (text, error) in [
            (r#"{"error":"missing transcript"}"#, "missing transcript"),
            ("could not start sigy", "could not start sigy"),
            ("command timed out", "command timed out"),
            (r#"{"error":7}"#, r#"{"error":7}"#),
        ] {
            let result = complete(
                "analysis_transcript",
                json!({"content":[{"type":"text","text":text}], "isError":true}),
            );
            assert_eq!(result["structuredContent"], json!({"error":error}));
            assert_eq!(result["content"][0]["text"], text);
            assert_eq!(result["isError"], true);
        }
    }

    #[test]
    fn existing_snapshots_and_other_tools_are_unchanged() {
        for is_error in [true, false] {
            let original = json!({"structuredContent":{"doctor":{"checks":[]}},
                "content":[{"type":"text","text":"report"}], "isError":is_error});
            assert_eq!(complete("doctor", original.clone()), original);
        }
        let original = json!({"content":[{"text":"command failed"}], "isError":true});
        assert_eq!(complete("library_status", original.clone()), original);
    }

    #[test]
    fn missing_success_json_is_an_explicit_tool_error() {
        let result = complete(
            "doctor",
            json!({"content":[{"text":"plain output"}], "isError":false}),
        );
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["structuredContent"]["error"],
            "sigy command returned no JSON output"
        );
    }
}
