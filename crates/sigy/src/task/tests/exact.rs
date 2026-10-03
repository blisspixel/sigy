//! Explicit frozen-evidence commands never imply collection or processing authority.

use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn exact_observations_and_publication_have_distinct_checked_selections() -> TestResult {
    assert!(matches!(
        parse(&[
            "sigy",
            "freeze",
            "water",
            "freeze-1",
            "--expected-snapshot",
            "0"
        ])?,
        Operation::Task {
            command: TaskOperation::FreezeEvidence {
                expected_snapshot: 0,
                ..
            }
        }
    ));
    assert!(matches!(
        parse(&["sigy", "snapshot", "water", "1"])?,
        Operation::Task {
            command: TaskOperation::ShowEvidenceSnapshot { ordinal: 1, .. }
        }
    ));
    let args = [
        "sigy",
        "publish",
        "water",
        "publish-1",
        "--snapshot",
        "1",
        "--max-findings",
        "64",
        "--expected-generation",
        "0",
    ];
    assert!(matches!(parse(&args)?, Operation::Task {
        command: TaskOperation::Publish { spec, expected_generation: 0, .. }
    } if spec.snapshot_ordinal == 1 && spec.maximum_findings == 64));
    for (index, bad) in [(5, "0"), (5, "129"), (7, "0"), (7, "65"), (9, "1")] {
        let mut invalid = args;
        invalid[index] = bad;
        assert!(parse(&invalid).is_err());
    }
    let mut hybrid = args.to_vec();
    hybrid.extend(["--checkpoint", "1"]);
    assert!(parse(&hybrid).is_err());
    assert!(matches!(
        parse(&["sigy", "briefing", "water"])?,
        Operation::Task {
            command: TaskOperation::EvidenceBriefing { .. }
        }
    ));
    Ok(())
}
