//! Legacy transcript inspection. Recognition storage lives in `storage::recognition`.
//! The decision is zero USD and is not a ledger request.

#[cfg(test)]
use std::{fs::File, io::Read, path::Path};

#[cfg(test)]
use rusqlite::TransactionBehavior;
use rusqlite::{Connection, OptionalExtension, params};
#[cfg(test)]
use sha2::{Digest, Sha256};

use super::Store;
#[cfg(test)]
use super::{
    analysis::{AnalysisRecord, AnalysisSpan},
    dvr::{Recording, hex},
};
use crate::{Error, Result};

mod integrity;
pub(super) use integrity::audit;

#[cfg(test)]
const MAX_CUES: usize = 1024;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptOutcome {
    Created,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptCue {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub script: String,
    pub wording: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalTranscript {
    pub id: String,
    pub revision: i64,
    pub analysis_revision: i64,
    pub recording_id: String,
    pub media_sha256: String,
    pub role: String,
    pub profile: String,
    pub cues: Vec<TranscriptCue>,
    pub amount_micros: i64,
}

impl Store {
    /// Store one local transcript after the retained files match the pin.
    /// # Errors
    /// Returns validation or catalog errors. A paid request is never reserved.
    #[cfg(test)]
    pub(crate) fn commit_local_transcript(
        &mut self,
        directory: &Path,
        id: &str,
        revision: i64,
        now: i64,
    ) -> Result<(TranscriptOutcome, LocalTranscript)> {
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        let pin = self.published_analysis(id, revision)?;
        if let Some(existing) = self.local_transcript(id, revision)? {
            return unchanged(existing, &pin);
        }
        self.prepare_pin(directory, &pin)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if read_transcript(&transaction, id, revision)?.is_some() {
            return Err(Error::InvalidInput("transcript revision is published"));
        }
        insert_transcript(&transaction, &pin, now)?;
        transaction.commit()?;
        let stored = self
            .local_transcript(id, revision)?
            .ok_or(Error::StorageIntegrity)?;
        Ok((TranscriptOutcome::Created, stored))
    }

    pub(crate) fn local_transcript(
        &self,
        id: &str,
        revision: i64,
    ) -> Result<Option<LocalTranscript>> {
        read_transcript(&self.connection, id, revision)
    }

    pub(crate) fn audit_transcripts(&self) -> Result<()> {
        audit(&self.connection)
    }

    #[cfg(test)]
    fn prepare_pin(&self, directory: &Path, pin: &AnalysisRecord) -> Result<()> {
        let recording = self.recording(&pin.recording_id)?;
        verify_retained_files(directory, pin, &recording)?;
        cues_for(pin)?;
        Ok(())
    }

    #[cfg(test)]
    fn rollback_local_transcript(
        &mut self,
        directory: &Path,
        id: &str,
        revision: i64,
        now: i64,
    ) -> Result<()> {
        if now < 0 {
            return Err(Error::InvalidInput("clock range"));
        }
        let pin = self.published_analysis(id, revision)?;
        if self.local_transcript(id, revision)?.is_some() {
            return Err(Error::InvalidInput("transcript revision is published"));
        }
        self.prepare_pin(directory, &pin)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_transcript(&transaction, &pin, now)?;
        transaction.rollback()?;
        Ok(())
    }
}

#[cfg(test)]
fn unchanged(
    existing: LocalTranscript,
    pin: &AnalysisRecord,
) -> Result<(TranscriptOutcome, LocalTranscript)> {
    if existing.recording_id != pin.recording_id || existing.media_sha256 != pin.media_sha256 {
        return Err(Error::InvalidInput("retained checksum does not match"));
    }
    if !aligned(&existing, pin) {
        return Err(Error::StorageIntegrity);
    }
    Ok((TranscriptOutcome::Unchanged, existing))
}

#[cfg(test)]
fn aligned(existing: &LocalTranscript, pin: &AnalysisRecord) -> bool {
    if existing.cues.len() != pin.intervals.len() {
        return false;
    }
    pin.intervals.iter().all(|interval| {
        existing.cues.iter().any(|cue| {
            cue.ordinal == interval.ordinal
                && cue.start_us == interval.start_us
                && cue.end_us == interval.end_us
                && cue.script.is_empty()
                && cue.wording == "uncertain"
        })
    })
}

#[cfg(test)]
fn verify_retained_files(
    directory: &Path,
    pin: &AnalysisRecord,
    recording: &Recording,
) -> Result<()> {
    if pin.intervals.is_empty() {
        return Err(Error::InvalidInput("no retained media"));
    }
    for span in &pin.intervals {
        let Some(interval) = recording.intervals.iter().find(|interval| {
            interval.ordinal == span.ordinal
                && interval.sha256 == span.sha256
                && !interval.released
                && interval.decoded_start_us == span.start_us
                && interval.decoded_end_us == span.end_us
        }) else {
            return Err(Error::InvalidInput("retained checksum does not match"));
        };
        let path = crate::recordings::media_path(directory, &interval.object_key)?;
        let actual = hash_file(&path)?;
        if actual != span.sha256 {
            return Err(Error::InvalidInput("retained checksum does not match"));
        }
    }
    Ok(())
}

#[cfg(test)]
fn hash_file(path: &Path) -> Result<String> {
    crate::library::reject_link(path)?;
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::InvalidInput("retained media is missing"));
        }
        Err(error) => return Err(error.into()),
    };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
fn cues_for(pin: &AnalysisRecord) -> Result<Vec<AnalysisSpan>> {
    if pin.intervals.is_empty() {
        return Err(Error::InvalidInput("no retained media"));
    }
    if pin.intervals.len() > MAX_CUES {
        return Err(Error::InvalidInput("transcript cue capacity reached"));
    }
    let mut seen = Vec::with_capacity(pin.intervals.len());
    for interval in &pin.intervals {
        if interval.start_us >= interval.end_us || seen.contains(&interval.ordinal) {
            return Err(Error::StorageIntegrity);
        }
        for gap in &pin.gaps {
            if gap.start_us >= gap.end_us {
                return Err(Error::StorageIntegrity);
            }
            if interval.start_us < gap.end_us && gap.start_us < interval.end_us {
                return Err(Error::InvalidInput("retained audio overlaps a gap"));
            }
        }
        seen.push(interval.ordinal);
    }
    Ok(pin.intervals.clone())
}

#[cfg(test)]
fn insert_transcript(connection: &Connection, pin: &AnalysisRecord, now: i64) -> Result<()> {
    let cues = cues_for(pin)?;
    connection.execute(
        "INSERT INTO transcripts(id, revision, analysis_id, analysis_revision, recording_id, media_sha256, role, profile, state, created_ms) VALUES (?1, 1, ?1, ?2, ?3, ?4, 'original', 'local-unmeasured', 'published', ?5)",
        params![pin.id, pin.revision, pin.recording_id, pin.media_sha256, now],
    )?;
    for cue in &cues {
        connection.execute(
            "INSERT INTO transcript_cues(transcript_id, revision, ordinal, start_us, end_us, script, wording) VALUES (?1, 1, ?2, ?3, ?4, '', 'uncertain')",
            params![
                pin.id,
                i64::from(cue.ordinal),
                micros(cue.start_us)?,
                micros(cue.end_us)?
            ],
        )?;
    }
    connection.execute(
        "INSERT INTO analysis_decisions(transcript_id, transcript_revision, amount_micros, request_id, created_ms) VALUES (?1, 1, 0, NULL, ?2)",
        params![pin.id, now],
    )?;
    Ok(())
}

#[cfg(test)]
fn micros(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::StorageIntegrity)
}

fn read_transcript(
    connection: &Connection,
    analysis_id: &str,
    analysis_revision: i64,
) -> Result<Option<LocalTranscript>> {
    let row = connection
        .query_row(
            "SELECT t.id, t.revision, t.recording_id, t.media_sha256, t.role, t.profile, d.amount_micros, d.request_id
             FROM transcripts t
             JOIN analysis_decisions d ON d.transcript_id = t.id AND d.transcript_revision = t.revision
             WHERE t.analysis_id = ?1 AND t.analysis_revision = ?2 AND t.kind = 'legacy_placeholder'",
            params![analysis_id, analysis_revision],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((id, revision, recording_id, media_sha256, role, profile, amount, request_id)) = row
    else {
        return Ok(None);
    };
    if role != "original"
        || profile != "local-unmeasured"
        || amount != 0
        || request_id.is_some()
        || revision != 1
    {
        return Err(Error::StorageIntegrity);
    }
    let cues = read_cues(connection, &id, revision)?;
    Ok(Some(LocalTranscript {
        id,
        revision,
        analysis_revision,
        recording_id,
        media_sha256,
        role,
        profile,
        cues,
        amount_micros: amount,
    }))
}

#[cfg(test)]
pub(super) mod migration_tests;

fn read_cues(connection: &Connection, id: &str, revision: i64) -> Result<Vec<TranscriptCue>> {
    let mut statement = connection.prepare(
        "SELECT ordinal, start_us, end_us, script, wording FROM transcript_cues WHERE transcript_id = ?1 AND revision = ?2 ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![id, revision], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut cues = Vec::new();
    for row in rows {
        let (ordinal, start_us, end_us, script, wording) = row?;
        if !script.is_empty() || wording != "uncertain" {
            return Err(Error::StorageIntegrity);
        }
        cues.push(TranscriptCue {
            ordinal: u32::try_from(ordinal).map_err(|_| Error::StorageIntegrity)?,
            start_us: u64::try_from(start_us).map_err(|_| Error::StorageIntegrity)?,
            end_us: u64::try_from(end_us).map_err(|_| Error::StorageIntegrity)?,
            script,
            wording,
        });
    }
    if cues.is_empty() {
        return Err(Error::StorageIntegrity);
    }
    Ok(cues)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::dvr::{Publication, Retention};
    use super::*;
    use crate::{
        control::{AnalysisDisposition, AnalysisOperation, Operation, apply, apply_library},
        domain::money::Usd,
        library::Library,
        sources::{HttpHop, HttpSource, NetworkScope},
    };

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    // Preserve v23 persistence/rollback coverage without exposing the legacy empty writer in production.
    fn apply_legacy_fixture(
        library: &mut Library,
        operation: Operation,
    ) -> Result<crate::control::Snapshot> {
        let Operation::Analysis {
            command:
                AnalysisOperation::Transcribe {
                    input: id,
                    revision,
                    ..
                },
        } = operation
        else {
            return Err(Error::InvalidInput("legacy fixture operation"));
        };
        let directory = library.directory().to_path_buf();
        let (outcome, _) = library
            .store_mut()
            .commit_local_transcript(&directory, &id, revision, 12)?;
        let mut snapshot = apply(library.store_mut(), AnalysisOperation::Show { id }.into())?;
        let page = snapshot.analysis.as_mut().ok_or(Error::StorageIntegrity)?;
        page.disposition = Some(match outcome {
            TranscriptOutcome::Created => AnalysisDisposition::Transcribed,
            TranscriptOutcome::Unchanged => AnalysisDisposition::TranscriptUnchanged,
        });
        Ok(snapshot)
    }

    fn open_library(root: &Path) -> Result<Library> {
        let mut library = Library::open(root, true)?;
        let executable = std::env::current_exe()?;
        library.store_mut().configure_dvr(
            10_000,
            64 * 1024 * 1024,
            14,
            executable
                .to_str()
                .ok_or(Error::InvalidInput("test executable"))?,
        )?;
        library.store_mut().register_source(
            "radio:v1",
            &HttpSource::new(
                "Test radio",
                "https://example.com/audio",
                NetworkScope::PublicInternet {},
            )?,
        )?;
        Ok(library)
    }

    fn publication(sha: &str, bytes: u64) -> Publication {
        Publication {
            bytes,
            sha256: sha.to_owned(),
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
        }
    }

    fn media_digest(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    fn write_media(directory: &Path, key: &str, bytes: &[u8]) -> Result<PathBuf> {
        crate::recordings::checked_directory(directory, true)?;
        let path = crate::recordings::media_path(directory, key)?;
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    fn publish_audio(library: &mut Library, id: &str, bytes: &[u8]) -> Result<PathBuf> {
        let sha = media_digest(bytes);
        let job = library
            .store_mut()
            .admit_recording(id, "radio:v1", 60, 600, Retention::Temporary, false)?
            .ok_or(Error::StorageIntegrity)?;
        let len = u64::try_from(bytes.len()).map_err(|_| Error::StorageIntegrity)?;
        library
            .store_mut()
            .publish_recording(&job.version, &publication(&sha, len))?;
        let key = library
            .store()
            .recording(id)?
            .intervals
            .first()
            .ok_or(Error::StorageIntegrity)?
            .object_key
            .clone();
        write_media(library.directory(), &key, bytes)
    }

    fn seal_pin(library: &mut Library, pin: &str, recording: &str) -> Result<()> {
        library
            .store_mut()
            .admit_analysis(pin, recording, false, 10)?;
        library.store_mut().publish_analysis(pin, 1)?;
        Ok(())
    }

    fn transcribe(id: &str, revision: i64) -> Operation {
        Operation::Analysis {
            command: AnalysisOperation::Transcribe {
                id: format!("job-{id}"),
                input: id.to_owned(),
                revision,
                profile: "absent-profile".to_owned(),
                parent_revision: None,
            },
        }
    }

    fn transcript_count(store: &Store) -> Result<i64> {
        Ok(store
            .connection
            .query_row("SELECT count(*) FROM transcripts", [], |row| row.get(0))?)
    }

    #[test]
    fn production_transcribe_refuses_without_creating_placeholder_or_work() -> TestResult {
        let root = tempfile::tempdir()?;
        let mut library = open_library(root.path())?;
        publish_audio(&mut library, "one", b"retained fixture")?;
        seal_pin(&mut library, "pin", "one")?;
        assert!(matches!(
            apply_library(&mut library, transcribe("pin", 1)),
            Err(Error::ServiceRequired)
        ));
        assert_eq!(transcript_count(library.store())?, 0);
        let jobs: i64 = library.store().connection.query_row(
            "SELECT count(*) FROM analysis_jobs",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(jobs, 0);
        assert_ledger(library.store(), 0)?;
        Ok(())
    }

    fn assert_ledger(store: &Store, limit: i64) -> Result<()> {
        store.audit_ledger()?;
        store.audit_transcripts()?;
        let (actual_limit, reserved): (i64, i64) = store.connection.query_row(
            "SELECT limit_micros, reserved_micros FROM budgets WHERE id = 'global'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!((actual_limit, reserved), (limit, 0));
        let requests: i64 =
            store
                .connection
                .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))?;
        assert_eq!(requests, 0);
        let paid: i64 = store.connection.query_row(
            "SELECT count(*) FROM ledger_events WHERE kind IN ('reserved', 'submitted', 'uncertain', 'settled', 'released')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(paid, 0);
        Ok(())
    }

    #[test]
    fn refused_inputs_publish_no_transcript() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut library = open_library(directory.path())?;
        let bytes = b"local-audio";
        publish_audio(&mut library, "open", bytes)?;
        library
            .store_mut()
            .admit_analysis("pin-open", "open", false, 1)?;
        assert!(matches!(
            apply_legacy_fixture(&mut library, transcribe("pin-open", 1)),
            Err(Error::InvalidInput("analysis revision is not published"))
        ));

        let gone = publish_audio(&mut library, "gone", bytes)?;
        seal_pin(&mut library, "pin-gone", "gone")?;
        library
            .store_mut()
            .acknowledge_processing("gone", "cleanup-receipt")?;
        std::fs::remove_file(&gone)?;
        assert!(matches!(
            apply_legacy_fixture(&mut library, transcribe("pin-gone", 1)),
            Err(Error::InvalidInput("retained media is missing"))
        ));

        let bad = publish_audio(&mut library, "bad", bytes)?;
        seal_pin(&mut library, "pin-bad", "bad")?;
        std::fs::write(&bad, b"not-the-audio")?;
        assert!(matches!(
            apply_legacy_fixture(&mut library, transcribe("pin-bad", 1)),
            Err(Error::InvalidInput("retained checksum does not match"))
        ));

        publish_audio(&mut library, "shift", bytes)?;
        seal_pin(&mut library, "pin-shift", "shift")?;
        library.store_mut().connection.execute(
            "UPDATE recordings SET sha256 = ?1 WHERE id = 'shift'",
            ["b".repeat(64)],
        )?;
        assert!(matches!(
            apply_legacy_fixture(&mut library, transcribe("pin-shift", 1)),
            Err(Error::InvalidInput("retained checksum does not match"))
        ));
        assert_eq!(transcript_count(library.store())?, 0);
        assert_ledger(library.store(), 0)?;
        Ok(())
    }

    #[test]
    fn one_original_script_revision_is_zero_usd_and_labels_uncertainty() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut library = open_library(directory.path())?;
        let bytes = b"sigy-local-transcript";
        let path = publish_audio(&mut library, "one", bytes)?;
        seal_pin(&mut library, "pin", "one")?;
        assert!(matches!(
            apply(library.store_mut(), transcribe("pin", 1)),
            Err(Error::ServiceRequired)
        ));
        assert_eq!(transcript_count(library.store())?, 0);

        let view = apply_legacy_fixture(&mut library, transcribe("pin", 1))?;
        assert_eq!(view.schema_version, super::super::SCHEMA_VERSION);
        let page = view.analysis.as_ref().ok_or(Error::StorageIntegrity)?;
        assert_eq!(page.disposition, Some(AnalysisDisposition::Transcribed));
        let transcript = page.transcript.as_ref().ok_or(Error::StorageIntegrity)?;
        assert_eq!(transcript.role, "original");
        assert_eq!(transcript.profile, "local-unmeasured");
        assert_eq!(transcript.revision, 1);
        let cue = transcript.cues.first().ok_or(Error::StorageIntegrity)?;
        assert_eq!(cue.script, "");
        assert_eq!(cue.wording, "uncertain");
        assert_eq!(cue.start_us, 0);
        assert_eq!(cue.end_us, 1_000_000);
        let decision = page.decision.as_ref().ok_or(Error::StorageIntegrity)?;
        assert_eq!(decision.amount_usd, "0.000000");
        assert!(!decision.paid_request);
        let rendered = serde_json::to_string(page)?;
        assert!(!rendered.contains("example.com"));
        assert!(!rendered.contains("https://"));
        assert!(!rendered.contains("http://"));
        assert_eq!(transcript_count(library.store())?, 1);
        let decisions: i64 = library.store().connection.query_row(
            "SELECT count(*) FROM analysis_decisions WHERE amount_micros = 0 AND request_id IS NULL",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(decisions, 1);
        assert_ledger(library.store(), 0)?;

        let shown = apply(
            library.store_mut(),
            Operation::Analysis {
                command: AnalysisOperation::Show { id: "pin".into() },
            },
        )?;
        let shown_page = shown.analysis.as_ref().ok_or(Error::StorageIntegrity)?;
        assert_eq!(shown_page.disposition, None);
        assert_eq!(
            shown_page
                .transcript
                .as_ref()
                .ok_or(Error::StorageIntegrity)?
                .cues
                .first()
                .ok_or(Error::StorageIntegrity)?
                .wording,
            "uncertain"
        );

        std::fs::remove_file(path)?;
        let again = apply_legacy_fixture(&mut library, transcribe("pin", 1))?;
        assert_eq!(
            again.analysis.as_ref().and_then(|item| item.disposition),
            Some(AnalysisDisposition::TranscriptUnchanged)
        );
        assert_eq!(transcript_count(library.store())?, 1);
        assert_ledger(library.store(), 0)?;
        Ok(())
    }

    #[test]
    fn rollback_before_commit_and_replay_publish_one_transcript() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut library = open_library(directory.path())?;
        publish_audio(&mut library, "one", b"rollback-audio")?;
        seal_pin(&mut library, "pin", "one")?;
        let root = library.directory().to_path_buf();
        library
            .store_mut()
            .rollback_local_transcript(&root, "pin", 1, 10)?;
        assert_eq!(transcript_count(library.store())?, 0);
        assert_ledger(library.store(), 0)?;
        let created = apply_legacy_fixture(&mut library, transcribe("pin", 1))?;
        assert_eq!(
            created.analysis.as_ref().and_then(|item| item.disposition),
            Some(AnalysisDisposition::Transcribed)
        );
        let replay = apply_legacy_fixture(&mut library, transcribe("pin", 1))?;
        assert_eq!(
            replay.analysis.as_ref().and_then(|item| item.disposition),
            Some(AnalysisDisposition::TranscriptUnchanged)
        );
        assert_eq!(transcript_count(library.store())?, 1);
        let cues: i64 = library.store().connection.query_row(
            "SELECT count(*) FROM transcript_cues",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cues, 1);
        assert_ledger(library.store(), 0)?;
        Ok(())
    }

    #[test]
    fn a_gap_is_not_transcribed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut library = open_library(directory.path())?;
        let bytes = b"late-audio";
        let sha = media_digest(bytes);
        let job = library
            .store_mut()
            .admit_recording("late", "radio:v1", 60, 600, Retention::Temporary, false)?
            .ok_or(Error::StorageIntegrity)?;
        {
            let store = library.store_mut();
            let transaction = store.connection.transaction()?;
            super::super::dvr::journal_prefix_gap(&transaction, "late", 30_000_000)?;
            transaction.commit()?;
        }
        let len = u64::try_from(bytes.len()).map_err(|_| Error::StorageIntegrity)?;
        library
            .store_mut()
            .publish_recording(&job.version, &publication(&sha, len))?;
        let key = library
            .store()
            .recording("late")?
            .intervals
            .first()
            .ok_or(Error::StorageIntegrity)?
            .object_key
            .clone();
        write_media(library.directory(), &key, bytes)?;
        seal_pin(&mut library, "pin", "late")?;
        let view = apply_legacy_fixture(&mut library, transcribe("pin", 1))?;
        let page = view.analysis.as_ref().ok_or(Error::StorageIntegrity)?;
        assert!(
            page.input
                .gaps
                .iter()
                .any(|gap| gap.cause == "late_start" && gap.start_us == 0)
        );
        assert!(page.input.gaps.iter().any(|gap| gap.cause == "uncovered"));
        let cue = page
            .transcript
            .as_ref()
            .ok_or(Error::StorageIntegrity)?
            .cues
            .first()
            .ok_or(Error::StorageIntegrity)?;
        assert_eq!(
            page.transcript.as_ref().map(|item| item.cues.len()),
            Some(1)
        );
        assert_eq!(cue.start_us, 30_000_000);
        assert_eq!(cue.end_us, 31_000_000);
        assert_eq!(cue.wording, "uncertain");
        assert!(cue.script.is_empty());
        let rendered = serde_json::to_string(page)?;
        assert!(!rendered.contains("example.com"));
        assert_ledger(library.store(), 0)?;
        Ok(())
    }

    #[test]
    fn a_stale_analysis_revision_is_not_transcribed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut library = open_library(directory.path())?;
        publish_audio(&mut library, "one", b"replaced-audio")?;
        library.store_mut().admit_analysis("pin", "one", false, 1)?;
        library.store_mut().admit_analysis("pin", "one", true, 2)?;
        library.store_mut().publish_analysis("pin", 2)?;
        assert!(matches!(
            apply_legacy_fixture(&mut library, transcribe("pin", 1)),
            Err(Error::InvalidInput("analysis revision is not published"))
        ));
        assert_eq!(transcript_count(library.store())?, 0);
        let view = apply_legacy_fixture(&mut library, transcribe("pin", 2))?;
        assert_eq!(
            view.analysis.as_ref().and_then(|item| item.disposition),
            Some(AnalysisDisposition::Transcribed)
        );
        assert_eq!(transcript_count(library.store())?, 1);
        assert_ledger(library.store(), 0)?;
        Ok(())
    }

    #[test]
    fn a_positive_budget_still_reserves_no_paid_request() -> TestResult {
        let directory = tempfile::tempdir()?;
        let mut library = open_library(directory.path())?;
        let amount: Usd = "1".parse()?;
        library.store_mut().set_budget_limit("global", amount)?;
        publish_audio(&mut library, "one", b"budget-audio")?;
        seal_pin(&mut library, "pin", "one")?;
        apply_legacy_fixture(&mut library, transcribe("pin", 1))?;
        assert_eq!(transcript_count(library.store())?, 1);
        assert_ledger(library.store(), 1_000_000)?;
        assert_eq!(library.store().budget("global")?.limit(), amount);
        assert_eq!(library.store().budget("global")?.reserved(), Usd::ZERO);
        Ok(())
    }
}
