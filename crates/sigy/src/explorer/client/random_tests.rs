use super::*;
use crate::explorer::state::{Key, Modes, Workspace};
use sigy_service::discovery::{Station, StationFilter, ordered::RandomStation};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn model(reduced_motion: bool) -> Explorer {
    let mut model = Explorer::new(
        Modes {
            reduced_motion,
            linear: false,
            monochrome: true,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        1000,
    );
    model.note_link(Link::LocalCatalog);
    model
}

fn query(model: &mut Explorer) -> SearchQuery {
    let Effect::ExploreRandom(query) = model.handle(Key::Char('e')) else {
        panic!("random effect");
    };
    query
}

pub(crate) fn result() -> Result<RandomStation, serde_json::Error> {
    let station: Station = serde_json::from_value(serde_json::json!({
        "provider":"radio_browser","id":"00000000-0000-0000-0000-000000000001",
        "name":"Radio Montréal", "country":"CA", "state":"Québec", "languages":["french"],
        "language_codes":[], "tags":["news"], "codec":"MP3", "bitrate_kbps":128, "hls":false,
        "last_check_ok":true, "latitude":45.5, "longitude":-73.6,
        "stream_origin":"https://station.example", "observed_ms":1000, "refresh_id":"fixture"
    }))?;
    Ok(RandomStation {
        station: Some(station),
        favorite: false,
        eligible_count: 37,
        catalog: sigy_service::control::DirectoryCatalog {
            namespace: "a".repeat(32),
            revision: 3,
            comparison: sigy_service::discovery::ordered::COMPARISON.into(),
        },
    })
}

pub(crate) fn snapshot(result: &RandomStation) -> Result<Snapshot, serde_json::Error> {
    serde_json::from_value(
        serde_json::json!({"schema_version":51,"sqlite_version":"fixture",
        "provider_dispatch_available":false,"budgets":[],"captures":{"dispatch_available":false,
        "scheduled":0,"active":0,"interrupted":0,"terminal":0},
        "directory_catalog":result.catalog, "random_station":result}),
    )
}

#[test]
fn random_jump_is_passive_and_cuts_to_coordinates_in_both_motion_modes() -> TestResult {
    for reduced in [false, true] {
        let mut model = model(reduced);
        model.observe_playback(PlaybackView {
            id: "existing-listen".into(),
            state: "running".into(),
            format: None,
            source_revision: "existing:v1".into(),
            failure: None,
        });
        let query = query(&mut model);
        assert!(matches!(
            operations_for(&Effect::ExploreRandom(query.clone()), &model).as_slice(),
            [Operation::Radio {
                command: DirectoryOperation::Explore {
                    favorites_only: false,
                    ..
                }
            }]
        ));
        assert!(adopt_random(
            &mut model,
            &query,
            Ok((false, snapshot(&result()?)?))
        ));
        assert_eq!(model.globe_center(), (-73.6, 45.5));
        assert_eq!(model.workspace(), Workspace::Globe);
        assert_eq!(model.random_count(), Some(37));
        assert_eq!(model.rows().len(), 1);
        assert_eq!(
            model.playback().map(|view| view.id.as_str()),
            Some("existing-listen")
        );
        assert!(!model.has_next_page());
        assert!(model.catalog_label().contains("random selection"));
        assert!(model.status().contains("approximate"));
        assert!(!model.result_label().contains("page"));
        assert_eq!(model.handle(Key::Enter), Effect::None);
        model.handle(Key::Escape);
        let Effect::Search(first) = model.handle(Key::Char('g')) else {
            panic!("return to normal results");
        };
        assert!(first.after.is_none());
    }
    Ok(())
}

#[test]
fn random_jump_stops_enabled_spin_before_replacing_the_camera() -> TestResult {
    let mut model = Explorer::new(
        Modes {
            reduced_motion: false,
            monochrome: false,
            linear: false,
            emphasis: crate::explorer::state::Emphasis::Standard,
        },
        1000,
    );
    model.note_link(Link::LocalCatalog);
    let now = std::time::Instant::now();
    model.configure_motion(true, false, now);
    model.handle(Key::Char('7'));
    model.handle(Key::Char('r'));
    assert!(model.spin_requested());
    let query = query(&mut model);
    assert!(adopt_random(
        &mut model,
        &query,
        Ok((false, snapshot(&result()?)?))
    ));
    assert!(!model.spin_requested());
    model.advance_spin(now + std::time::Duration::from_secs(1), true);
    assert_eq!(model.globe_center(), (-73.6, 45.5));
    Ok(())
}

#[test]
fn random_replies_are_fenced_by_generation_catalog_and_valid_coordinates() -> TestResult {
    let mut model = model(true);
    let old = query(&mut model);
    let current = query(&mut model);
    assert!(!adopt_random(
        &mut model,
        &old,
        Ok((false, snapshot(&result()?)?))
    ));
    assert!(adopt_random(
        &mut model,
        &current,
        Ok((false, snapshot(&result()?)?))
    ));
    let previous = model.globe_center();
    let current = query(&mut model);
    let mut stale = result()?;
    stale.catalog.revision = 2;
    assert!(!adopt_random(
        &mut model,
        &current,
        Ok((false, snapshot(&stale)?))
    ));
    let current = query(&mut model);
    let mut invalid = result()?;
    invalid.station.as_mut().ok_or("station")?.latitude = Some(95.0);
    assert!(!adopt_random(
        &mut model,
        &current,
        Ok((false, snapshot(&invalid)?))
    ));
    assert_eq!(model.globe_center(), previous);
    let current = query(&mut model);
    let mut mismatch = snapshot(&result()?)?;
    mismatch.directory_catalog = None;
    assert!(!adopt_random(&mut model, &current, Ok((false, mismatch))));
    let current = query(&mut model);
    let mut missing = snapshot(&result()?)?;
    missing.random_station = None;
    assert!(!adopt_random(&mut model, &current, Ok((false, missing))));
    let current = query(&mut model);
    assert!(!adopt_random(&mut model, &current, Err(Error::NotFound)));
    let current = query(&mut model);
    model.handle(Key::Char('g'));
    assert!(!adopt_random(
        &mut model,
        &current,
        Ok((false, snapshot(&result()?)?))
    ));
    Ok(())
}

#[tokio::test]
async fn empty_cache_uses_service_operation_without_network_or_mutations() -> TestResult {
    let directory = tempfile::tempdir()?;
    drop(Library::open(directory.path(), true)?);
    let mut model = model(true);
    model.initialize_search(
        StationFilter {
            country: "CA".into(),
            language: "french".into(),
            name: "news".into(),
            tag: "talk".into(),
            healthy_only: true,
        },
        directory.path(),
    )?;
    model.search.applied.favorites_only = true;
    let query = query(&mut model);
    assert!(query.favorites_only && query.filter.healthy_only);
    assert_eq!(query.filter.country, "CA");
    perform(directory.path(), &mut model, &Effect::ExploreRandom(query)).await?;
    assert_eq!(model.random_count(), Some(0));
    assert!(
        model
            .status()
            .contains("No cached stations with coordinates")
    );
    Ok(())
}
