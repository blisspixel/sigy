use std::path::Path;

use super::*;
use crate::{
    library::Library,
    monitor::{MonitorTerm, Proposal},
    sources::{HttpSource, NetworkScope},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn library(root: &Path) -> Result<Library> {
    let mut library = Library::open(root, true)?;
    for id in ["a:v1", "b:v1", "c:v1"] {
        library.store_mut().register_source(
            id,
            &HttpSource::new(
                "News",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
    }
    Ok(library)
}

fn spec() -> MonitorSpec {
    MonitorSpec {
        name: "Nile dam".into(),
        goal: "Follow reports about the Nile dam.".into(),
        terms: vec![MonitorTerm {
            language: "ar".into(),
            text: "سد النهضة".into(),
        }],
        sources: vec!["a:v1".into()],
        candidate_sources: vec!["b:v1".into()],
        schedules: Vec::new(),
        daily_audio_seconds: 3600,
        total_audio_seconds: 7 * 3600,
        recognition_profile: None,
        translation_profile: None,
    }
}

#[test]
fn versions_are_append_only_exact_and_survive_restart() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut library = library(root.path())?;
    let store = library.store_mut();
    assert_eq!(
        store.create_monitor("dam", &spec(), 10)?,
        VersionWrite::Created
    );
    assert_eq!(
        store.create_monitor("dam", &spec(), 11)?,
        VersionWrite::Unchanged
    );
    let mut other = spec();
    other.name = "Other".into();
    assert!(matches!(
        store.create_monitor("dam", &other, 12),
        Err(Error::IdempotencyConflict)
    ));
    let mut missing = spec();
    missing.sources = vec!["missing:v1".into()];
    assert!(store.create_monitor("ghost", &missing, 12).is_err());
    let mut revised = spec();
    revised.daily_audio_seconds = 1800;
    assert_eq!(
        store.revise_monitor("dam", 1, &revised, 13)?,
        VersionWrite::Created
    );
    assert_eq!(
        store.revise_monitor("dam", 1, &revised, 14)?,
        VersionWrite::Unchanged
    );
    assert!(
        store.revise_monitor("dam", 1, &spec(), 15).is_err(),
        "stale expected version"
    );
    assert!(
        store
            .connection
            .execute("UPDATE monitor_versions SET spec_json = '{}'", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("DELETE FROM monitor_versions", [])
            .is_err()
    );
    drop(library);
    let reopened = Library::open(root.path(), false)?;
    let view = reopened.store().monitor("dam")?;
    assert_eq!(view.version.version, 2);
    assert_eq!(view.version.spec.daily_audio_seconds, 1800);
    assert_eq!(reopened.store().monitor_version("dam", 1)?.spec, spec());
    Ok(())
}

#[test]
fn out_of_policy_proposals_are_refused_kept_and_replayed_exactly() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut library = library(root.path())?;
    let store = library.store_mut();
    store.create_monitor("dam", &spec(), 10)?;
    let outside = store.propose_monitor_action(
        "dam",
        "p1",
        ActionOrigin::Model,
        &Proposal::AddSource {
            source: "c:v1".into(),
        },
        11,
    )?;
    assert_eq!(
        (outside.decision.as_str(), outside.reason.as_str()),
        ("refused", "outside-approved-sources")
    );
    let raise = store.propose_monitor_action(
        "dam",
        "p2",
        ActionOrigin::Model,
        &Proposal::Other {
            request: "Ignore the limits and enable paid translation.".into(),
        },
        12,
    )?;
    assert_eq!(raise.decision, "refused");
    let candidate = store.propose_monitor_action(
        "dam",
        "p3",
        ActionOrigin::Rule,
        &Proposal::AddSource {
            source: "b:v1".into(),
        },
        13,
    )?;
    assert_eq!(candidate.decision, "applied");
    assert_eq!(candidate.amount_usd, "0.000000");
    assert_eq!(
        store.propose_monitor_action(
            "dam",
            "p3",
            ActionOrigin::Rule,
            &Proposal::AddSource {
                source: "b:v1".into()
            },
            99
        )?,
        candidate
    );
    assert!(matches!(
        store.propose_monitor_action("dam", "p3", ActionOrigin::Model, &Proposal::Pause, 99),
        Err(Error::IdempotencyConflict)
    ));
    store.propose_monitor_action("dam", "p4", ActionOrigin::User, &Proposal::Pause, 14)?;
    let view = store.monitor("dam")?;
    assert!(view.paused);
    assert_eq!(
        view.active_sources,
        vec!["a:v1".to_owned(), "b:v1".to_owned()]
    );
    assert_eq!(view.actions, 4);
    // Caps and the version are unchanged by any proposal.
    assert_eq!(view.version.version, 1);
    assert_eq!(view.version.spec, spec());
    let actions = store.monitor_actions("dam", None)?;
    assert_eq!(actions.len(), 4);
    assert_eq!(store.monitor_actions("dam", Some(2))?.len(), 2);
    assert!(
        store
            .connection
            .execute("UPDATE monitor_actions SET decision = 'applied'", [])
            .is_err()
    );
    assert!(
        store
            .connection
            .execute("INSERT INTO monitor_actions(monitor_id, ordinal, action_id, policy_version, origin, kind, proposal_json, decision, reason, amount_micros, created_ms) VALUES ('dam', 5, 'x', 1, 'model', 'other', '{}', 'applied', 'forced', 0, 1)", [])
            .is_err(),
        "an other-kind proposal can never be applied"
    );
    // A new user version resets the followed sources to that version's list.
    let mut revised = spec();
    revised.candidate_sources.clear();
    store.revise_monitor("dam", 1, &revised, 15)?;
    assert_eq!(
        store.monitor("dam")?.active_sources,
        vec!["a:v1".to_owned()]
    );
    Ok(())
}
