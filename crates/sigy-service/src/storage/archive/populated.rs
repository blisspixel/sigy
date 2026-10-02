//! A populated catalog. One real recognition and translation is published, then cloned
//! under new transcript IDs on the same recording. Triggers and foreign keys are switched
//! off in this throwaway catalog only, so many rows can be inserted in one transaction.

use std::time::{Duration, Instant};

use super::*;

const CUES: u32 = 16;

/// Copy every row of `table` that `seed` selects once per clone, with replaced columns.
fn clone_rows(
    connection: &Connection,
    table: &str,
    seed: &str,
    replace: &[(&str, &str)],
    clones: u32,
) -> Result<()> {
    let mut statement =
        connection.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let select = columns
        .iter()
        .map(|column| {
            replace
                .iter()
                .find(|(name, _)| name == column)
                .map_or_else(|| format!("x.{column}"), |(_, value)| (*value).to_owned())
        })
        .collect::<Vec<_>>()
        .join(", ");
    connection.execute(
        &format!(
            "WITH RECURSIVE n(v) AS (VALUES(1) UNION ALL SELECT v + 1 FROM n WHERE v < ?1) INSERT INTO {table}({}) SELECT {select} FROM {table} x, n WHERE {seed}",
            columns.join(", ")
        ),
        [clones],
    )?;
    Ok(())
}

/// A catalog with `clones + 1` transcript revisions of 16 cues, each translated.
fn populate(path: &Path, clones: u32) -> Result<Store> {
    let mut store = library(path)?;
    record(&mut store, "seed", "radio:v1")?;
    let scripts: Vec<String> = (0..CUES)
        .map(|n| format!("Cue {n}: el caudal de la presa sube en la cuenca alta"))
        .collect();
    let english: Vec<String> = (0..CUES)
        .map(|n| format!("Cue {n}: the flow at the dam rises in the upper basin"))
        .collect();
    recognize(
        &mut store,
        "seed",
        &scripts.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    translate(
        &mut store,
        "mt-seed",
        "seed",
        1,
        &english
            .iter()
            .map(|text| Some(text.as_str()))
            .collect::<Vec<_>>(),
    )?;
    let triggers = store
        .connection
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'trigger'")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for name in triggers {
        store
            .connection
            .execute_batch(&format!("DROP TRIGGER \"{name}\""))?;
    }
    store
        .connection
        .pragma_update(None, "foreign_keys", false)?;
    let pin = "printf('pin-%06d', n.v)";
    let transaction = store.connection.transaction()?;
    clone_rows(
        &transaction,
        "transcripts",
        "x.id = 'seed'",
        &[
            ("id", pin),
            ("analysis_id", pin),
            ("job_id", "x.job_id || '-' || n.v"),
        ],
        clones,
    )?;
    clone_rows(
        &transaction,
        "transcript_cues",
        "x.transcript_id = 'seed'",
        &[("transcript_id", pin), ("script", "x.script || ' ' || n.v")],
        clones,
    )?;
    clone_rows(
        &transaction,
        "translations",
        "x.transcript_id = 'seed'",
        &[("transcript_id", pin), ("job_id", "x.job_id || '-' || n.v")],
        clones,
    )?;
    clone_rows(
        &transaction,
        "translation_cues",
        "x.transcript_id = 'seed'",
        &[("transcript_id", pin)],
        clones,
    )?;
    transaction.commit()?;
    Ok(store)
}

fn bounded(store: &Store, ask: ArchiveQuery) -> Result<ArchivePage> {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(120))
        .ok_or(Error::StorageIntegrity)?;
    search(&store.connection, ask.validate()?, deadline)
}

#[test]
fn a_populated_catalog_is_read_within_the_row_budget() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = populate(&directory.path().join("catalog"), 255)?;
    let transcripts: u32 =
        store
            .connection
            .query_row("SELECT count(*) FROM transcripts", [], |row| row.get(0))?;
    assert_eq!(transcripts, 256);
    let before = writes(&store)?;
    let mut miss = query("no such term");
    miss.scan_rows = Some(crate::archive::ARCHIVE_MAX_ROWS);
    let page = bounded(&store, miss.clone())?;
    assert_eq!(page.stopped, None);
    assert_eq!(page.transcripts_scanned, 256);
    assert_eq!(page.rows_scanned, 256 * (CUES + 1));
    miss.scan_rows = Some(1_000);
    let page = bounded(&store, miss.clone())?;
    assert_eq!(page.stopped, Some(ArchiveStop::Rows));
    assert_eq!(page.rows_scanned, 1_000);
    // History reads the same rows here: each revision has one translation.
    miss.revisions = ArchiveRevisions::All;
    miss.scan_rows = Some(crate::archive::ARCHIVE_MAX_ROWS);
    assert_eq!(bounded(&store, miss)?.rows_scanned, 256 * (CUES + 1));
    let mut hit = query("PRESA");
    hit.limit = Some(64);
    let page = bounded(&store, hit)?;
    assert_eq!(page.hits.len(), 64);
    assert_eq!(page.stopped, Some(ArchiveStop::Results));
    assert!(page.rows_scanned < 128);
    assert!(serde_json::to_vec(&page)?.len() <= ARCHIVE_PAGE_BYTES);
    assert_eq!(writes(&store)?, before);
    Ok(())
}

/// Wall time on this host. Run explicitly:
/// `cargo test -p sigy-service archive_measurement -- --ignored --nocapture`.
#[test]
#[ignore = "measurement; run explicitly and record host load"]
fn archive_measurement() -> TestResult {
    let directory = tempfile::tempdir()?;
    let built = Instant::now();
    let store = populate(&directory.path().join("catalog"), 8_191)?;
    let build_ms = built.elapsed().as_millis();
    let (transcripts, cues, english): (u32, u32, u32) = store.connection.query_row(
        "SELECT (SELECT count(*) FROM transcripts), (SELECT count(*) FROM transcript_cues), (SELECT count(*) FROM translation_cues)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    println!(
        "catalog transcripts={transcripts} cues={cues} english_cues={english} build_ms={build_ms}"
    );
    let cases: [(&str, &str, Option<u32>, ArchiveRevisions); 4] = [
        (
            "miss-max-rows",
            "no such term",
            Some(200_000),
            ArchiveRevisions::Current,
        ),
        (
            "miss-default",
            "no such term",
            None,
            ArchiveRevisions::Current,
        ),
        ("hit-limit-64", "PRESA", None, ArchiveRevisions::Current),
        (
            "miss-history",
            "no such term",
            Some(200_000),
            ArchiveRevisions::All,
        ),
    ];
    for (name, term, rows, revisions) in cases {
        for run in 0..5 {
            let mut ask = query(term);
            ask.scan_rows = rows;
            ask.revisions = revisions;
            ask.limit = Some(64);
            ask.deadline_ms = Some(2_000);
            let started = Instant::now();
            let page = store.archive_search(ask)?;
            let elapsed = started.elapsed();
            println!(
                "case={name} run={run} elapsed_us={} rows={} transcripts={} hits={} stopped={:?} bytes={}",
                elapsed.as_micros(),
                page.rows_scanned,
                page.transcripts_scanned,
                page.hits.len(),
                page.stopped,
                serde_json::to_vec(&page)?.len()
            );
        }
    }
    Ok(())
}
