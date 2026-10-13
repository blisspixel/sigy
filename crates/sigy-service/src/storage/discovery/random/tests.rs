use super::*;
use crate::{
    control::{DirectoryOperation, Operation},
    discovery::{RefreshRequest, radio_browser},
    sources::NetworkScope,
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn publish(store: &mut Store) -> Result<()> {
    let request = RefreshRequest {
        filter: StationFilter::default(),
        limit: 500,
        offset: 0,
        mirror: None,
        network: NetworkScope::PublicInternet {},
    };
    store.begin_refresh_at("fixture", &request, 1)?;
    let stations = (0..40)
        .map(|index| {
            serde_json::json!({
                "stationuuid":format!("00000000-0000-0000-0000-{index:012x}"),
                "name":format!("Station {index:02}"), "url":"https://station.example/audio",
                "countrycode":if index%2 == 0 {"CA"} else {"US"},
                "language":"french,english", "tags":"news", "lastcheckok":1,
                "geo_lat": if index < 38 { Some(45.0) } else { None }, "geo_long":-73.0
            })
        })
        .collect::<Vec<_>>();
    store.finish_refresh(
        "fixture",
        radio_browser::parse(
            &serde_json::to_vec(&stations)?,
            500,
            "https://directory.example".into(),
        )?,
    )
}

#[test]
fn seeded_ranks_cover_all_eligible_rows_beyond_the_first_page() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    publish(&mut store)?;
    for rank in 0..38_u64 {
        let pick = store.guarded_directory_read(|store| {
            store.random_station_with(&StationFilter::default(), false, || Ok(38 + rank))
        })?;
        assert_eq!(pick.eligible_count, 38);
        assert_eq!(
            pick.station.ok_or("missing station")?.name,
            format!("Station {rank:02}")
        );
        assert!(!pick.favorite);
        assert_eq!(pick.catalog, store.directory_catalog()?);
    }
    Ok(())
}

#[test]
fn filters_favorites_empty_and_service_authority_are_shared() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    let empty = store.guarded_directory_read(|store| {
        store.random_station_with(&StationFilter::default(), false, || {
            Err(Error::RequestState)
        })
    })?;
    assert_eq!(empty.eligible_count, 0);
    assert!(empty.station.is_none());
    publish(&mut store)?;
    let id = "00000000-0000-0000-0000-000000000024";
    store.set_station_favorite(id, true)?;
    let filter = StationFilter {
        name: "STATION 3".into(),
        country: "CA".into(),
        language: "FRENCH".into(),
        tag: "NEWS".into(),
        healthy_only: true,
    };
    let pick = store
        .guarded_directory_read(|store| store.random_station_with(&filter, false, || Ok(7)))?;
    assert_eq!(pick.eligible_count, 4);
    let operation = Operation::Radio {
        command: DirectoryOperation::Explore {
            filter: filter.clone(),
            favorites_only: true,
        },
    };
    let before = store.directory_catalog()?;
    let view = crate::control::apply(&mut store, operation)?;
    let pick = view.random_station.ok_or("missing result")?;
    assert_eq!(pick.station.ok_or("missing favorite")?.id, id);
    assert!(pick.favorite);
    assert_eq!(pick.eligible_count, 1);
    assert_eq!(before, store.directory_catalog()?);
    assert!(view.station_page.is_none() && view.ordered_station_page.is_none());
    for filter in [
        StationFilter {
            name: "absent".into(),
            ..filter.clone()
        },
        StationFilter {
            country: "US".into(),
            ..filter.clone()
        },
        StationFilter {
            language: "absent".into(),
            ..filter.clone()
        },
        StationFilter {
            tag: "absent".into(),
            ..filter
        },
    ] {
        assert_eq!(
            store
                .guarded_directory_read(|store| store.random_station_with(&filter, true, || Ok(1)))?
                .eligible_count,
            0
        );
    }
    Ok(())
}

#[test]
fn rejection_sampling_is_bounded_and_never_uses_modulo_bias() -> TestResult {
    let mut values = [0, 1, 5].into_iter();
    assert_eq!(
        uniform_rank(3, || values.next().ok_or(Error::RequestState))?,
        1
    );
    assert_eq!(values.next(), Some(5));
    assert_eq!(uniform_rank(1, || Ok(u64::MAX))?, 0);
    assert!(uniform_rank(0, || Ok(0)).is_err());
    assert!(uniform_rank(3, || Ok(0)).is_err());
    assert!(uniform_rank(3, || Err(Error::RequestState)).is_err());
    Ok(())
}

#[test]
fn corrupt_selected_metadata_is_refused_and_read_guard_releases() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    publish(&mut store)?;
    store.connection.execute(
        "UPDATE directory_stations SET metadata_json=json_set(metadata_json,'$.id','wrong')",
        [],
    )?;
    assert!(
        store
            .guarded_directory_read(|store| store.random_station_with(
                &StationFilter::default(),
                false,
                || Ok(38)
            ))
            .is_err()
    );
    assert!(store.directory_catalog().is_ok());
    let invalid = StationFilter {
        country: "invalid".into(),
        ..StationFilter::default()
    };
    assert!(
        store
            .guarded_directory_read(|store| store.random_station_with(&invalid, false, || Ok(1)))
            .is_err()
    );
    Ok(())
}

#[test]
fn full_cache_pick_and_exhausted_work_never_return_a_partial_population() -> TestResult {
    use crate::storage::query_work::{Limits, QueryWork};
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    for batch in 0..20 {
        let request = RefreshRequest {
            filter: StationFilter::default(),
            limit: 500,
            offset: 0,
            mirror: None,
            network: NetworkScope::PublicInternet {},
        };
        let request_id = format!("capacity-{batch}");
        store.begin_refresh_at(&request_id, &request, 1 + i64::from(batch) * 3000)?;
        let body = (0..500)
            .map(|row| {
                serde_json::json!({
                    "stationuuid":format!("00000000-0000-0000-0000-{:012x}", batch*500+row),
                    "name":"Station", "url":"https://station.example/audio", "countrycode":"CA",
                    "language":"french", "tags":"news", "geo_lat":45.0, "geo_long":-73.0
                })
            })
            .collect::<Vec<_>>();
        store.finish_refresh(
            &request_id,
            radio_browser::parse(
                &serde_json::to_vec(&body)?,
                500,
                "https://directory.example".into(),
            )?,
        )?;
    }
    // Measure SQL work separately from the interactive wall-clock refusal under CI load.
    let work = QueryWork::start(
        &store.connection,
        Limits {
            wall: std::time::Duration::from_secs(10),
            ..Limits::TASK_EVIDENCE
        },
    )?;
    let started = std::time::Instant::now();
    let pick = store.random_station_with(&StationFilter::default(), false, || Ok(19999))?;
    work.check()?;
    assert_eq!(pick.eligible_count, 10000);
    assert_eq!(
        pick.station.ok_or("station")?.id,
        "00000000-0000-0000-0000-00000000270f"
    );
    eprintln!(
        "random full cache: {:?}, {} checkpoint SQL operations",
        started.elapsed(),
        work.checkpoint_ops()
    );
    work.finish()?;
    let work = QueryWork::start_for_test(
        &store.connection,
        Limits {
            vm_ops: 1000,
            ..Limits::TASK_EVIDENCE
        },
        1,
    )?;
    assert!(
        store
            .random_station_with(&StationFilter::default(), false, || Ok(19999))
            .is_err()
    );
    assert!(work.exhausted());
    work.finish()?;
    assert_eq!(store.directory_status()?.cached_stations, 10000);
    Ok(())
}
