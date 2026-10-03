//! Full legal cache characterization, with no host latency qualification.

use super::*;

fn fill(store: &mut Store) -> Result<()> {
    let labels = (0..32)
        .map(|index| format!("label{index:02}"))
        .collect::<Vec<_>>()
        .join(",");
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
        let body=(0..500).map(|row| {
            let index=batch*500+row;
            let uuid=format!("00000000-0000-0000-0000-{:012x}",index*7919%10000);
            serde_json::json!({"stationuuid":uuid,"name":format!("Station{index:05}"),"url":"https://station.example/audio","countrycode":"CA","language":"english,french","tags":labels,"lastcheckok":1})
        }).collect::<Vec<_>>();
        let parsed = radio_browser::parse(
            &serde_json::to_vec(&body)?,
            500,
            "https://directory.example".into(),
        )?;
        store.finish_refresh(&request_id, parsed)?;
    }
    Ok(())
}

fn bounded_outcome(result: &Result<OrderedStationPage>) -> bool {
    match result {
        Ok(_) | Err(Error::Analysis("query-work-exhausted")) => true,
        Err(Error::Database(rusqlite::Error::SqliteFailure(error, _))) => {
            error.code == rusqlite::ErrorCode::OperationInterrupted
        }
        _ => false,
    }
}

#[test]
fn full_cache_index_sparse_labels_and_wrapper_characterization() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("catalog"))?;
    fill(&mut store)?;
    assert_eq!(store.directory_status()?.cached_stations, 10000);
    let mut explain = store
        .connection
        .prepare(&format!("EXPLAIN QUERY PLAN {SELECT}"))?;
    let plan = explain
        .query_map(
            params![b"".as_slice(), "", "", "", "", "", false, false, 17],
            |row| row.get::<_, String>(3),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert!(
        plan.iter()
            .any(|detail| detail.contains("directory_name_order"))
    );
    assert!(!plan.iter().any(|detail| detail.contains("TEMP B-TREE")));
    drop(explain);
    let began = std::time::Instant::now();
    let response = crate::control::apply(
        &mut store,
        crate::control::DirectoryOperation::SearchOrdered {
            filter: StationFilter::default(),
            favorites_only: false,
            after: None,
            limit: 16,
        }
        .into(),
    )?;
    let wrapper = began.elapsed();
    assert_eq!(
        response.ordered_station_page.ok_or("page")?.entries.len(),
        16
    );
    store
        .connection
        .busy_timeout(std::time::Duration::from_millis(37))?;
    for filter in [
        StationFilter {
            name: "station09999".into(),
            ..StationFilter::default()
        },
        StationFilter {
            tag: "absent-label".into(),
            ..StationFilter::default()
        },
    ] {
        let work = QueryWork::start(&store.connection, Limits::TASK_EVIDENCE)?;
        let began = std::time::Instant::now();
        let result = store
            .ordered_stations_in_work(&filter, false, None, 16)
            .and_then(|page| {
                work.check()?;
                Ok(page)
            });
        let elapsed = began.elapsed();
        let ops = work.checkpoint_ops();
        assert!(bounded_outcome(&result), "{result:?}");
        assert!(ops <= Limits::TASK_EVIDENCE.vm_ops);
        if let Ok(page) = &result {
            assert_eq!(page.entries.len(), usize::from(filter.tag.is_empty()));
            if let Some(station) = page.entries.first() {
                assert_eq!(station.name, "Station09999");
            }
        }
        eprintln!(
            "10k directory: wrapper={wrapper:?}, filter={filter:?}, elapsed={elapsed:?}, checkpoint_ops={ops}, result={:?}",
            result.as_ref().map(|page| page.entries.len())
        );
        // Either wall or VM exhaustion is legal; the count identifies which was reached.
        work.finish()?;
        let wait: u32 = store
            .connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
        assert_eq!(wait, 37);
        assert_eq!(
            store
                .search_stations_ordered(&StationFilter::default(), false, None, 16)?
                .entries
                .len(),
            16
        );
    }
    Ok(())
}
