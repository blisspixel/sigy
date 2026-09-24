//! Analysis reads a published checksum and its media clock. It does not receive a source URL.

use super::{Store, validate_key};
use crate::{Error, Result, storage::dvr::Recording};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

const MAX_ANALYSIS_INPUTS: i64 = 256;
const UNCOVERED_ORDINAL: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmitOutcome {
    Created,
    Unchanged,
    Replaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisSpan {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisHole {
    pub ordinal: u32,
    pub cause: String,
    pub start_us: u64,
    pub end_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Timeline {
    planned_us: u64,
    intervals: Vec<AnalysisSpan>,
    gaps: Vec<AnalysisHole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AnalysisRecord {
    pub id: String,
    pub revision: i64,
    pub recording_id: String,
    pub media_sha256: String,
    pub state: String,
    pub planned_us: u64,
    pub intervals: Vec<AnalysisSpan>,
    pub gaps: Vec<AnalysisHole>,
}

struct StoredRevision {
    revision: i64,
    recording_id: String,
    media_sha256: String,
    timeline_json: String,
    state: String,
}

impl Store {
    pub(crate) fn admit_analysis(
        &mut self,
        id: &str,
        recording_id: &str,
        replace_worker: bool,
        now: i64,
    ) -> Result<(AdmitOutcome, AnalysisRecord)> {
        validate_key(id, "analysis input ID")?;
        let binding = bind_recording(&self.recording(recording_id)?)?;
        let timeline_json = serde_json::to_string(&binding.timeline)?;
        if timeline_json.len() > 65536 {
            return Err(Error::InvalidInput("analysis timeline is too large"));
        }
        let latest = self.latest_analysis(id)?;
        if let Some(latest) = latest {
            if latest.recording_id != recording_id {
                return Err(Error::InvalidInput("analysis recording is immutable"));
            }
            let same = latest.media_sha256 == binding.media_sha256
                && latest.timeline_json == timeline_json;
            if same && !replace_worker {
                return Ok((AdmitOutcome::Unchanged, record_from(id, &latest)?));
            }
            if latest.state == "published" && same {
                return Err(Error::InvalidInput("analysis revision is published"));
            }
            if latest.revision >= 64 {
                return Err(Error::InvalidInput("analysis revision capacity reached"));
            }
            let revision = latest.revision + 1;
            self.insert_analysis(
                id,
                revision,
                &binding,
                &timeline_json,
                now,
                (latest.state == "admitted").then_some(latest.revision),
            )?;
            return Ok((
                AdmitOutcome::Replaced,
                self.analysis_revision(id, revision)?,
            ));
        }
        let count: i64 = self.connection.query_row(
            "SELECT count(DISTINCT id) FROM analysis_inputs",
            [],
            |row| row.get(0),
        )?;
        if count >= MAX_ANALYSIS_INPUTS {
            return Err(Error::InvalidInput("analysis input capacity reached"));
        }
        self.insert_analysis(id, 1, &binding, &timeline_json, now, None)?;
        Ok((AdmitOutcome::Created, self.analysis_revision(id, 1)?))
    }

    pub(crate) fn publish_analysis(&mut self, id: &str, revision: i64) -> Result<AnalysisRecord> {
        validate_key(id, "analysis input ID")?;
        let row = self.analysis_row(id, revision)?.ok_or(Error::NotFound)?;
        if row.state == "published" {
            return record_from(id, &row);
        }
        if row.state != "admitted" {
            return Err(Error::InvalidInput("stale analysis revision"));
        }
        let latest = self.latest_analysis(id)?.ok_or(Error::NotFound)?;
        if latest.revision != revision {
            return Err(Error::InvalidInput("stale analysis revision"));
        }
        let binding = bind_recording(&self.recording(&row.recording_id)?)?;
        let timeline_json = serde_json::to_string(&binding.timeline)?;
        if binding.media_sha256 != row.media_sha256 || timeline_json != row.timeline_json {
            return Err(Error::InvalidInput("retained checksum does not match"));
        }
        let changed = self.connection.execute(
            "UPDATE analysis_inputs SET state = 'published' WHERE id = ?1 AND revision = ?2 AND state = 'admitted'",
            params![id, revision],
        )?;
        if changed != 1 {
            return Err(Error::InvalidInput("stale analysis revision"));
        }
        self.analysis_revision(id, revision)
    }

    pub(crate) fn published_analysis(&self, id: &str, revision: i64) -> Result<AnalysisRecord> {
        validate_key(id, "analysis input ID")?;
        let row = self.analysis_row(id, revision)?.ok_or(Error::NotFound)?;
        if row.state != "published" {
            return Err(Error::InvalidInput("analysis revision is not published"));
        }
        let latest = self.latest_analysis(id)?.ok_or(Error::NotFound)?;
        if latest.revision != revision {
            return Err(Error::InvalidInput("stale analysis revision"));
        }
        let binding = bind_recording(&self.recording(&row.recording_id)?)?;
        let timeline_json = serde_json::to_string(&binding.timeline)?;
        if binding.media_sha256 != row.media_sha256 || timeline_json != row.timeline_json {
            return Err(Error::InvalidInput("retained checksum does not match"));
        }
        record_from(id, &row)
    }

    pub(crate) fn analysis_input(&self, id: &str) -> Result<AnalysisRecord> {
        validate_key(id, "analysis input ID")?;
        let latest = self.latest_analysis(id)?.ok_or(Error::NotFound)?;
        record_from(id, &latest)
    }

    fn insert_analysis(
        &mut self,
        id: &str,
        revision: i64,
        binding: &Binding,
        timeline_json: &str,
        now: i64,
        supersede: Option<i64>,
    ) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // A first pin cannot have a worker. Replacements must not move the pin
        // while native cleanup is still outstanding, even during cancellation.
        if revision > 1 {
            let native_active: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM analysis_jobs WHERE analysis_id = ?1 AND kind = 'local_asr' AND state IN ('running', 'cancelling'))",
                [id], |row| row.get(0),
            )?;
            if native_active {
                return Err(Error::Analysis("native-worker-active"));
            }
        }
        if let Some(previous) = supersede {
            let changed = transaction.execute(
                "UPDATE analysis_inputs SET state = 'superseded' WHERE id = ?1 AND revision = ?2 AND state = 'admitted'",
                params![id, previous],
            )?;
            if changed != 1 {
                return Err(Error::InvalidInput("stale analysis revision"));
            }
        }
        transaction.execute(
            "INSERT INTO analysis_inputs(id, revision, recording_id, media_sha256, timeline_json, state, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, 'admitted', ?6)",
            params![
                id,
                revision,
                binding.recording_id,
                binding.media_sha256,
                timeline_json,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn latest_analysis(&self, id: &str) -> Result<Option<StoredRevision>> {
        self.connection
            .query_row(
                "SELECT revision, recording_id, media_sha256, timeline_json, state FROM analysis_inputs WHERE id = ?1 ORDER BY revision DESC LIMIT 1",
                [id],
                read_revision,
            )
            .optional()
            .map_err(Error::from)
    }

    fn analysis_row(&self, id: &str, revision: i64) -> Result<Option<StoredRevision>> {
        self.connection
            .query_row(
                "SELECT revision, recording_id, media_sha256, timeline_json, state FROM analysis_inputs WHERE id = ?1 AND revision = ?2",
                params![id, revision],
                read_revision,
            )
            .optional()
            .map_err(Error::from)
    }

    pub(crate) fn analysis_revision(&self, id: &str, revision: i64) -> Result<AnalysisRecord> {
        record_from(
            id,
            &self.analysis_row(id, revision)?.ok_or(Error::NotFound)?,
        )
    }
}

struct Binding {
    recording_id: String,
    media_sha256: String,
    timeline: Timeline,
}

fn bind_recording(recording: &Recording) -> Result<Binding> {
    if recording.state != "completed"
        || recording.storage_state != "retained"
        || recording.open_ceiling > 0
        || recording.open_object_key.is_some()
    {
        return Err(Error::InvalidInput("unpublished recording"));
    }
    let Some(stored) = recording.sha256.as_deref() else {
        return Err(Error::InvalidInput("unpublished recording"));
    };
    let retained: Vec<_> = recording
        .intervals
        .iter()
        .filter(|interval| !interval.released)
        .collect();
    if retained.is_empty() {
        return Err(Error::InvalidInput("no retained media"));
    }
    if retained.len() == 1 && retained[0].sha256 != stored {
        return Err(Error::InvalidInput("retained checksum does not match"));
    }
    let intervals = retained
        .iter()
        .map(|interval| AnalysisSpan {
            ordinal: interval.ordinal,
            start_us: interval.decoded_start_us,
            end_us: interval.decoded_end_us,
            sha256: interval.sha256.clone(),
        })
        .collect::<Vec<_>>();
    let mut gaps = recording
        .gaps
        .iter()
        .map(|gap| AnalysisHole {
            ordinal: gap.ordinal,
            cause: gap.cause.as_str().to_owned(),
            start_us: gap.start_us,
            end_us: gap.end_us,
        })
        .collect::<Vec<_>>();
    refuse_overlap(&intervals, &gaps)?;
    let planned_us = recording
        .duration_seconds
        .checked_mul(1_000_000)
        .ok_or(Error::StorageIntegrity)?;
    fill_uncovered(planned_us, &intervals, &mut gaps)?;
    Ok(Binding {
        recording_id: recording.id.clone(),
        media_sha256: stored.to_owned(),
        timeline: Timeline {
            planned_us,
            intervals,
            gaps,
        },
    })
}

fn refuse_overlap(intervals: &[AnalysisSpan], gaps: &[AnalysisHole]) -> Result<()> {
    for interval in intervals {
        if interval.start_us >= interval.end_us {
            return Err(Error::StorageIntegrity);
        }
        for gap in gaps {
            if gap.start_us >= gap.end_us {
                return Err(Error::StorageIntegrity);
            }
            if interval.start_us < gap.end_us && gap.start_us < interval.end_us {
                return Err(Error::InvalidInput("retained audio overlaps a gap"));
            }
        }
    }
    Ok(())
}

fn fill_uncovered(
    planned_us: u64,
    intervals: &[AnalysisSpan],
    gaps: &mut Vec<AnalysisHole>,
) -> Result<()> {
    let mut covered = intervals
        .iter()
        .map(|interval| (interval.start_us, interval.end_us))
        .chain(gaps.iter().map(|gap| (gap.start_us, gap.end_us)))
        .collect::<Vec<_>>();
    covered.sort_unstable();
    let mut cursor = 0_u64;
    let mut extra = Vec::new();
    for (start_us, end_us) in covered {
        if start_us > cursor && cursor < planned_us {
            let end = start_us.min(planned_us);
            if end > cursor {
                extra.push((cursor, end));
            }
        }
        if end_us > cursor {
            cursor = end_us;
        }
    }
    if planned_us > cursor {
        extra.push((cursor, planned_us));
    }
    for (index, (start_us, end_us)) in extra.into_iter().enumerate() {
        let ordinal = UNCOVERED_ORDINAL
            .checked_add(u32::try_from(index).map_err(|_| Error::StorageIntegrity)?)
            .ok_or(Error::StorageIntegrity)?;
        gaps.push(AnalysisHole {
            ordinal,
            cause: "uncovered".to_owned(),
            start_us,
            end_us,
        });
    }
    gaps.sort_by_key(|gap| (gap.start_us, gap.ordinal));
    Ok(())
}

fn read_revision(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRevision> {
    Ok(StoredRevision {
        revision: row.get(0)?,
        recording_id: row.get(1)?,
        media_sha256: row.get(2)?,
        timeline_json: row.get(3)?,
        state: row.get(4)?,
    })
}

fn record_from(id: &str, row: &StoredRevision) -> Result<AnalysisRecord> {
    let timeline: Timeline = serde_json::from_str(&row.timeline_json)?;
    Ok(AnalysisRecord {
        id: id.to_owned(),
        revision: row.revision,
        recording_id: row.recording_id.clone(),
        media_sha256: row.media_sha256.clone(),
        state: row.state.clone(),
        planned_us: timeline.planned_us,
        intervals: timeline.intervals,
        gaps: timeline.gaps,
    })
}

#[cfg(test)]
mod tests {
    use super::super::dvr::{Publication, Retention};
    use super::*;
    use crate::{
        control::{Operation, RecordingOperation, apply},
        sources::{HttpHop, HttpSource, NetworkScope},
    };

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn setup(path: &std::path::Path) -> Result<Store> {
        let mut store = Store::open(path)?;
        let executable = std::env::current_exe()?;
        store.configure_dvr(
            10_000,
            64 * 1024 * 1024,
            14,
            executable
                .to_str()
                .ok_or(Error::InvalidInput("test executable"))?,
        )?;
        store.register_source(
            "radio:v1",
            &HttpSource::new(
                "Test radio",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
        Ok(store)
    }

    fn publication() -> Publication {
        Publication {
            bytes: 100,
            sha256: "a".repeat(64),
            format: "wav",
            decoded_microseconds: 1_000_000,
            end_reason: "end_of_body",
            http_route: vec![HttpHop {
                origin: "https://example.com".into(),
                peer: std::net::SocketAddr::from(([8, 8, 8, 8], 443)),
                status: 200,
            }],
            observations: Vec::new(),
            segments_sealed: false,
            gap: None,
        }
    }

    fn publish(store: &mut Store, id: &str) -> Result<()> {
        let job = store
            .admit_recording(id, "radio:v1", 60, 600, Retention::Temporary, false)?
            .ok_or(Error::StorageIntegrity)?;
        store.publish_recording(&job.version, &publication())
    }

    fn reserved(store: &Store) -> Result<i64> {
        Ok(store.connection.query_row(
            "SELECT coalesce(sum(reserved_micros), 0) FROM budgets",
            [],
            |row| row.get(0),
        )?)
    }

    #[test]
    fn unpublished_and_mismatched_recordings_are_refused() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let job = store
            .admit_recording("open", "radio:v1", 60, 600, Retention::Temporary, false)?
            .ok_or("not admitted")?;
        assert!(matches!(
            store.admit_analysis("pin", "open", false, 1),
            Err(Error::InvalidInput("unpublished recording"))
        ));
        store.publish_recording(&job.version, &publication())?;
        store.connection.execute(
            "UPDATE recordings SET sha256 = ?1 WHERE id = 'open'",
            ["b".repeat(64)],
        )?;
        assert!(matches!(
            store.admit_analysis("pin", "open", false, 2),
            Err(Error::InvalidInput("retained checksum does not match"))
        ));
        Ok(())
    }

    #[test]
    fn admission_pins_the_checksum_and_leaves_the_source_url_behind() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        publish(&mut store, "one")?;
        let before = reserved(&store)?;
        store.acknowledge_processing("one", "cleanup-receipt")?;
        let columns: String = store.connection.query_row(
            "SELECT group_concat(name, ',') FROM pragma_table_info('analysis_inputs')",
            [],
            |row| row.get(0),
        )?;
        assert!(!columns.contains("url"));
        assert!(!columns.contains("endpoint"));
        assert!(!columns.contains("receipt"));
        let (outcome, pin) = store.admit_analysis("pin", "one", false, 10)?;
        assert_eq!(outcome, AdmitOutcome::Created);
        assert_eq!(pin.revision, 1);
        assert_eq!(pin.media_sha256, "a".repeat(64));
        assert_eq!(pin.planned_us, 60_000_000);
        assert_eq!(pin.intervals.len(), 1);
        assert_eq!(pin.intervals[0].sha256, pin.media_sha256);
        assert!(
            pin.gaps
                .iter()
                .any(|gap| gap.cause == "uncovered" && gap.start_us == 1_000_000)
        );
        let worker = serde_json::to_string(&pin.intervals)?;
        let rendered = serde_json::to_string(&pin.gaps)?;
        assert!(!worker.contains("example.com"));
        assert!(!rendered.contains("example.com"));
        assert!(!rendered.contains("cleanup-receipt"));
        assert_eq!(reserved(&store)?, before);
        let (again, same) = store.admit_analysis("pin", "one", false, 11)?;
        assert_eq!(again, AdmitOutcome::Unchanged);
        assert_eq!(same.revision, 1);
        let count: i64 =
            store
                .connection
                .query_row("SELECT count(*) FROM analysis_inputs", [], |row| row.get(0))?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn a_prefix_gap_stays_a_gap_on_the_analysis_clock() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        let job = store
            .admit_recording("late", "radio:v1", 60, 600, Retention::Temporary, false)?
            .ok_or("not admitted")?;
        let transaction = store.connection.transaction()?;
        super::super::dvr::journal_prefix_gap(&transaction, "late", 30_000_000)?;
        transaction.commit()?;
        store.publish_recording(&job.version, &publication())?;
        let (_, pin) = store.admit_analysis("pin", "late", false, 5)?;
        assert!(pin.gaps.iter().any(|gap| {
            gap.cause == "late_start" && gap.start_us == 0 && gap.end_us == 30_000_000
        }));
        assert_eq!(pin.intervals[0].start_us, 30_000_000);
        assert!(pin.gaps.iter().any(|gap| gap.cause == "uncovered"));
        let rendered = serde_json::to_string(&pin)?;
        assert!(!rendered.contains("https://"));
        assert!(!rendered.contains("endpoint"));
        Ok(())
    }

    #[test]
    fn a_stale_revision_cannot_publish_over_the_replacement() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        publish(&mut store, "one")?;
        store.admit_analysis("pin", "one", false, 1)?;
        let (_, replaced) = store.admit_analysis("pin", "one", true, 2)?;
        assert_eq!(replaced.revision, 2);
        assert_eq!(replaced.state, "admitted");
        assert!(matches!(
            store.publish_analysis("pin", 1),
            Err(Error::InvalidInput("stale analysis revision"))
        ));
        let published = store.publish_analysis("pin", 2)?;
        assert_eq!(published.state, "published");
        let replay = store.publish_analysis("pin", 2)?;
        assert_eq!(replay.state, "published");
        let rows: i64 = store.connection.query_row(
            "SELECT count(*) FROM analysis_inputs WHERE state = 'published'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(rows, 1);
        Ok(())
    }

    #[test]
    fn a_failed_replacement_keeps_the_previous_pin_publishable() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        publish(&mut store, "one")?;
        store.admit_analysis("pin", "one", false, 1)?;
        store.connection.execute_batch(
            "CREATE TRIGGER reject_analysis_replacement BEFORE INSERT ON analysis_inputs
             WHEN NEW.revision = 2 BEGIN SELECT RAISE(ABORT, 'injected insert failure'); END;",
        )?;
        assert!(store.admit_analysis("pin", "one", true, 2).is_err());
        let current = store.analysis_input("pin")?;
        assert_eq!(current.revision, 1);
        assert_eq!(current.state, "admitted");
        assert_eq!(store.publish_analysis("pin", 1)?.state, "published");
        Ok(())
    }

    #[test]
    fn the_cleanup_receipt_does_not_admit_analysis() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut store = setup(&directory.path().join("catalog.sqlite3"))?;
        publish(&mut store, "one")?;
        apply(
            &mut store,
            Operation::Record {
                command: RecordingOperation::Processed {
                    id: "one".into(),
                    receipt: "cleanup-receipt".into(),
                },
            },
        )?;
        let rows: i64 =
            store
                .connection
                .query_row("SELECT count(*) FROM analysis_inputs", [], |row| row.get(0))?;
        assert_eq!(rows, 0);
        assert_eq!(
            store.recording("one")?.processing_receipt.as_deref(),
            Some("cleanup-receipt")
        );
        Ok(())
    }
}
