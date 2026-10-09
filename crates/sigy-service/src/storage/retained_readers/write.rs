use rusqlite::{Connection, Transaction, TransactionBehavior, params};

use super::{
    Limits, QueryWork, RetainedExcerpt, RetainedReadSpec, RetainedReadView, Store, read,
    validate_key,
};
use crate::{Error, Result};

mod finding;
mod refusal;

#[derive(Clone, Copy)]
enum Request<'a> {
    Legacy {
        recording: &'a str,
        seek: u64,
    },
    Range {
        recording: &'a str,
        seek: u64,
        end: u64,
    },
    Finding {
        monitor: &'a str,
        finding: &'a str,
    },
}

impl Request<'_> {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Legacy { recording, .. } | Self::Range { recording, .. } => {
                validate_key(recording, "recording ID")
            }
            Self::Finding { monitor, finding } => {
                validate_key(monitor, "monitor ID")?;
                validate_key(finding, "finding ID")
            }
        }
    }

    fn matches(&self, view: &RetainedReadView) -> bool {
        match self {
            Self::Legacy { recording, seek } => {
                view.spec.excerpt.is_none()
                    && view.spec.recording_id == *recording
                    && view.seek_us == *seek
            }
            Self::Range {
                recording,
                seek,
                end,
            } => {
                view.spec.recording_id == *recording
                    && view.seek_us == *seek
                    && view.spec.excerpt.as_ref().is_some_and(|excerpt| {
                        excerpt.version == 2
                            && excerpt.timeline_end_us == *end
                            && excerpt.citation.is_none()
                    })
            }
            Self::Finding { monitor, finding } => view
                .spec
                .excerpt
                .as_ref()
                .and_then(|excerpt| excerpt.citation.as_ref())
                .is_some_and(|citation| {
                    citation.monitor_id == *monitor && citation.finding_id == *finding
                }),
        }
    }
}

impl Store {
    pub(crate) fn admit_retained_reader(
        &mut self,
        id: &str,
        recording_id: &str,
        seek_us: u64,
        now_ms: i64,
    ) -> Result<(RetainedReadSpec, bool)> {
        self.admit_retained_request(
            id,
            Request::Legacy {
                recording: recording_id,
                seek: seek_us,
            },
            now_ms,
        )
    }

    pub(crate) fn admit_retained_range(
        &mut self,
        id: &str,
        recording_id: &str,
        seek_us: u64,
        end_us: u64,
        now_ms: i64,
    ) -> Result<(RetainedReadSpec, bool)> {
        self.admit_retained_request(
            id,
            Request::Range {
                recording: recording_id,
                seek: seek_us,
                end: end_us,
            },
            now_ms,
        )
    }

    pub(crate) fn admit_retained_finding(
        &mut self,
        id: &str,
        monitor_id: &str,
        finding_id: &str,
        now_ms: i64,
    ) -> Result<(RetainedReadSpec, bool)> {
        self.admit_retained_request(
            id,
            Request::Finding {
                monitor: monitor_id,
                finding: finding_id,
            },
            now_ms,
        )
    }

    fn admit_retained_request(
        &mut self,
        id: &str,
        request: Request<'_>,
        now_ms: i64,
    ) -> Result<(RetainedReadSpec, bool)> {
        validate_key(id, "retained reader ID")?;
        request.validate()?;
        let work = QueryWork::start(&self.connection, Limits::RETAINED_HISTORY)?;
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        if let Some(view) = read::find(&tx, id)? {
            if !request.matches(&view) {
                return Err(Error::IdempotencyConflict);
            }
            read::audit(&tx, &view)?;
            work.check()?;
            tx.commit()?;
            work.finish()?;
            return Ok((view.spec, false));
        }
        if now_ms < 0 {
            return Err(Error::InvalidInput("retained reader clock"));
        }
        let (spec, seek) = match request {
            Request::Legacy { recording, seek } => {
                let seek = signed_seek(seek)?;
                (resolve(&tx, id, recording, seek)?, seek)
            }
            Request::Range {
                recording,
                seek,
                end,
            } => {
                let seek = signed_seek(seek)?;
                (resolve_range(&tx, id, recording, seek, end, None)?, seek)
            }
            Request::Finding { monitor, finding } => {
                let selected = finding::resolve(&tx, monitor, finding)?;
                let seek = signed_seek(selected.start_us)?;
                (
                    resolve_range(
                        &tx,
                        id,
                        &selected.recording_id,
                        seek,
                        selected.end_us,
                        Some(selected.citation),
                    )?,
                    seek,
                )
            }
        };
        insert(&tx, &spec, seek, now_ms)?;
        work.check()?;
        tx.commit()?;
        work.finish()?;
        Ok((spec, true))
    }

    pub(crate) fn cancel_retained_reader(
        &mut self,
        id: &str,
        generation: u64,
        now_ms: i64,
    ) -> Result<RetainedReadView> {
        self.mutate_retained_reader(id, |tx, view| {
            if view.spec.generation != generation {
                return Err(Error::RequestState);
            }
            if view.state == "running" {
                tx.execute(
                    "UPDATE retained_readers SET state='cancelling',updated_ms=?2 WHERE id=?1",
                    params![id, now_ms.max(view.updated_ms)],
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn finish_retained_reader(
        &mut self,
        receipt: &crate::recordings::retained::RetainedReadReceipt,
        now_ms: i64,
    ) -> Result<RetainedReadView> {
        self.mutate_retained_reader(receipt.id(), |tx, view| {
            if view.spec.generation != receipt.generation() || !receipt.matches(&view.spec) {
                return Err(Error::RequestState);
            }
            let state = if receipt.successful() { "completed" } else { "failed" };
            if matches!(view.state.as_str(), "completed" | "failed") {
                if view.state != state || view.completion_reason.as_deref() != Some(receipt.reason()) {
                    return Err(Error::IdempotencyConflict);
                }
                return Ok(());
            }
            tx.execute("UPDATE retained_readers SET state=?2,completion_reason=?3,updated_ms=?4 WHERE id=?1", params![receipt.id(),state,receipt.reason(),now_ms.max(view.updated_ms)])?;
            Ok(())
        })
    }

    fn mutate_retained_reader(
        &mut self,
        id: &str,
        change: impl FnOnce(&Connection, &RetainedReadView) -> Result<()>,
    ) -> Result<RetainedReadView> {
        validate_key(id, "retained reader ID")?;
        let work = QueryWork::start(&self.connection, Limits::RETAINED_HISTORY)?;
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let view = read::find(&tx, id)?.ok_or(Error::NotFound)?;
        read::audit(&tx, &view)?;
        change(&tx, &view)?;
        let result = read::find(&tx, id)?.ok_or(Error::StorageIntegrity)?;
        work.check()?;
        tx.commit()?;
        work.finish()?;
        Ok(result)
    }

    pub(crate) fn recover_retained_readers(&mut self, now_ms: i64) -> Result<u32> {
        if now_ms < 0 {
            return Err(Error::InvalidInput("retained reader clock"));
        }
        let work = QueryWork::start(&self.connection, Limits::RETAINED_HISTORY)?;
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let count = tx.execute("UPDATE retained_readers SET state='recovery_held',recovery_reason='restart-completion-unproven',updated_ms=max(updated_ms,?1) WHERE state IN ('running','cancelling')", [now_ms])?;
        work.check()?;
        tx.commit()?;
        work.finish()?;
        u32::try_from(count).map_err(|_| Error::StorageIntegrity)
    }

    pub(crate) fn hold_retained_reader(
        &mut self,
        id: &str,
        generation: u64,
        reason: &str,
        now_ms: i64,
    ) -> Result<RetainedReadView> {
        validate_key(reason, "retained recovery reason")?;
        if now_ms < 0 {
            return Err(Error::InvalidInput("retained reader clock"));
        }
        self.mutate_retained_reader(id, |tx,view| {
            if view.spec.generation != generation { return Err(Error::RequestState); }
            if !matches!(view.state.as_str(), "completed" | "failed") {
                tx.execute("UPDATE retained_readers SET state='recovery_held',recovery_reason=?2,updated_ms=max(updated_ms,?3) WHERE id=?1",params![id,reason,now_ms])?;
            }
            Ok(())
        })
    }
}

fn signed_seek(seek: u64) -> Result<i64> {
    i64::try_from(seek).map_err(|_| Error::InvalidInput("retained seek"))
}

fn resolve_range(
    connection: &Connection,
    id: &str,
    recording_id: &str,
    seek: i64,
    end_us: u64,
    citation: Option<super::RetainedCitation>,
) -> Result<RetainedReadSpec> {
    let end = i64::try_from(end_us).map_err(|_| Error::InvalidInput("retained excerpt end"))?;
    if seek >= end {
        return Err(Error::InvalidInput(
            "retained excerpt must have start before end",
        ));
    }
    let mut spec = resolve(connection, id, recording_id, seek)?;
    if end_us > spec.timeline_end_us {
        return Err(Error::InvalidInput(
            "retained excerpt must fit one sealed interval",
        ));
    }
    let gapped: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM recording_gaps WHERE recording_id=?1 AND start_us<?3 AND end_us>?2)",
        params![recording_id, seek, end], |row| row.get(0),
    )?;
    if gapped {
        return Err(Error::InvalidInput(
            "retained excerpt overlaps a recording gap",
        ));
    }
    spec.excerpt = Some(RetainedExcerpt {
        version: 2,
        timeline_end_us: end_us,
        citation,
    });
    spec.playback_duration_us()?;
    spec.spec_sha256 = spec.digest()?;
    Ok(spec)
}

fn resolve(
    connection: &Connection,
    id: &str,
    recording_id: &str,
    seek: i64,
) -> Result<RetainedReadSpec> {
    let mut query = connection.prepare("SELECT c.source_revision,i.ordinal,i.object_key,i.sha256,i.format,i.byte_end-i.byte_start,i.decoded_start_us,i.decoded_end_us FROM recording_intervals i JOIN recordings r ON r.id=i.recording_id JOIN capture_jobs c ON c.id=r.id WHERE r.id=?1 AND r.storage_state IN ('reserved','retained') AND i.decoded_start_us<=?2 AND i.decoded_end_us>?2 AND (r.open_object_key IS NULL OR r.open_object_key!=i.object_key) AND NOT EXISTS(SELECT 1 FROM recording_releases x WHERE x.recording_id=i.recording_id AND x.segment_ordinal=i.ordinal) AND NOT EXISTS(SELECT 1 FROM recording_gaps g WHERE g.recording_id=r.id AND g.start_us<=?2 AND g.end_us>?2) ORDER BY i.ordinal LIMIT 2")?;
    let mut rows = query.query(params![recording_id, seek])?;
    let Some(row) = rows.next()? else {
        return Err(refusal::explain(connection, recording_id, seek)?);
    };
    let start = read::number(row, 6)?;
    let end = read::number(row, 7)?;
    let mut spec = RetainedReadSpec {
        request_id: id.to_owned(),
        generation: 1,
        recording_id: recording_id.to_owned(),
        source_revision: read::text(row, 0, 128)?,
        ordinal: u32::try_from(read::number(row, 1)?).map_err(|_| Error::StorageIntegrity)?,
        object_key: read::text(row, 2, 32)?,
        sha256: read::text(row, 3, 64)?,
        format: read::text(row, 4, 6)?,
        bytes: read::number(row, 5)?,
        timeline_start_us: start,
        timeline_end_us: end,
        file_seek_us: u64::try_from(seek)
            .map_err(|_| Error::StorageIntegrity)?
            .checked_sub(start)
            .ok_or(Error::StorageIntegrity)?,
        file_duration_us: end.checked_sub(start).ok_or(Error::StorageIntegrity)?,
        spec_sha256: String::new(),
        excerpt: None,
    };
    if spec.bytes == 0 || spec.bytes > super::MAX_RETAINED_READ_BYTES || rows.next()?.is_some() {
        return Err(Error::StorageIntegrity);
    }
    if spec.file_duration_us > super::RETAINED_READ_DEADLINE_SECONDS * 1_000_000 {
        return Err(Error::InvalidInput("retained object duration"));
    }
    spec.spec_sha256 = spec.digest()?;
    Ok(spec)
}

pub(super) fn insert(
    connection: &Connection,
    s: &RetainedReadSpec,
    seek: i64,
    now_ms: i64,
) -> Result<()> {
    let excerpt = s.excerpt.as_ref();
    let citation = excerpt.and_then(|excerpt| excerpt.citation.as_ref());
    connection.execute("INSERT INTO retained_readers(id,recording_id,seek_us,generation,source_revision,ordinal,object_key,sha256,format,bytes,timeline_start_us,timeline_end_us,file_seek_us,file_duration_us,spec_sha256,state,admitted_ms,updated_ms,excerpt_version,excerpt_end_us,citation_monitor,citation_finding,citation_transcript,citation_transcript_revision,citation_translation_revision,citation_cue_ordinal) VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'running',?15,?15,?16,?17,?18,?19,?20,?21,?22,?23)", params![s.request_id,s.recording_id,seek,s.source_revision,s.ordinal,s.object_key,s.sha256,s.format,read::signed(s.bytes)?,read::signed(s.timeline_start_us)?,read::signed(s.timeline_end_us)?,read::signed(s.file_seek_us)?,read::signed(s.file_duration_us)?,s.spec_sha256,now_ms,excerpt.map(|value|value.version),excerpt.map(|value|read::signed(value.timeline_end_us)).transpose()?,citation.map(|value|value.monitor_id.as_str()),citation.map(|value|value.finding_id.as_str()),citation.map(|value|value.transcript_id.as_str()),citation.map(|value|value.transcript_revision),citation.map(|value|value.translation_revision),citation.map(|value|value.cue_ordinal)])?;
    Ok(())
}
