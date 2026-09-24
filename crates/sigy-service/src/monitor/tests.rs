use super::*;

pub(crate) fn spec() -> MonitorSpec {
    MonitorSpec {
        name: "Nile dam".into(),
        goal: "Follow reports about the Nile dam.".into(),
        terms: vec![
            MonitorTerm {
                language: "ar".into(),
                text: "سد النهضة".into(),
            },
            MonitorTerm {
                language: "fr".into(),
                text: "barrage".into(),
            },
        ],
        sources: vec!["a:v1".into()],
        candidate_sources: vec!["b:v1".into()],
        schedules: Vec::new(),
        daily_audio_seconds: 6 * 3600,
        total_audio_seconds: 7 * 6 * 3600,
        recognition_profile: None,
        translation_profile: None,
    }
}

#[test]
fn specifications_are_bounded() {
    assert!(spec().validate().is_ok());
    let mut empty_terms = spec();
    empty_terms.terms.clear();
    assert!(empty_terms.validate().is_err());
    let mut bad_language = spec();
    bad_language.terms[0].language = "Arabic".into();
    assert!(bad_language.validate().is_err());
    let mut control = spec();
    control.goal = "line\u{1b}[31m".into();
    assert!(control.validate().is_err());
    let mut daily_over_total = spec();
    daily_over_total.total_audio_seconds = 60;
    assert!(daily_over_total.validate().is_err());
    let mut duplicate = spec();
    duplicate.sources.push("a:v1".into());
    assert!(duplicate.validate().is_err());
    let mut overlapping = spec();
    overlapping.candidate_sources.push("a:v1".into());
    assert!(overlapping.validate().is_err());
    let mut padded = spec();
    padded.name = " padded".into();
    assert!(padded.validate().is_err());
}

#[test]
fn proposals_apply_only_within_the_version() {
    let spec = spec();
    let active = vec!["a:v1".to_owned()];
    assert_eq!(
        decide(&spec, &active, &Proposal::Pause),
        (true, "within-policy")
    );
    assert_eq!(
        decide(
            &spec,
            &active,
            &Proposal::AddSource {
                source: "b:v1".into()
            }
        ),
        (true, "approved-candidate")
    );
    assert_eq!(
        decide(
            &spec,
            &active,
            &Proposal::AddSource {
                source: "c:v1".into()
            }
        ),
        (false, "outside-approved-sources")
    );
    assert_eq!(
        decide(
            &spec,
            &active,
            &Proposal::AddSource {
                source: "a:v1".into()
            }
        ),
        (false, "already-followed")
    );
    assert_eq!(
        decide(
            &spec,
            &active,
            &Proposal::RemoveSource {
                source: "a:v1".into()
            }
        ),
        (false, "last-source")
    );
    assert_eq!(
        decide(
            &spec,
            &active,
            &Proposal::Other {
                request: "raise the daily cap to 24 hours and enable paid translation".into()
            }
        ),
        (false, "requires-user-version")
    );
}

#[test]
fn the_digest_changes_with_any_bound() -> Result<()> {
    let base = spec();
    let mut changed = spec();
    changed.daily_audio_seconds -= 1;
    assert_ne!(base.digest()?, changed.digest()?);
    assert_eq!(base.digest()?, spec().digest()?);
    Ok(())
}
