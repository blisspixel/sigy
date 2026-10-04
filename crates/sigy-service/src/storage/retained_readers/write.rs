use rusqlite::{Connection, Transaction, TransactionBehavior, params};

use super::{Limits, QueryWork, RetainedReadSpec, RetainedReadView, Store, read, validate_key};
use crate::{Error, Result};

mod refusal;

impl Store {
    pub(crate) fn admit_retained_reader(
        &mut self,
        id: &str,
        recording_id: &str,
        seek_us: u64,
        now_ms: i64,
    ) -> Result<(RetainedReadSpec, bool)> {
        validate_key(id, "retained reader ID")?;
        validate_key(recording_id, "recording ID")?;
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        if let Some(view) = read::find(&tx, id)? {
            if view.spec.recording_id != recording_id || view.seek_us != seek_us {
                return Err(Error::IdempotencyConflict);
            }
            read::audit(&tx, &view)?;
            work.check()?;
            tx.commit()?;
            work.finish()?;
            return Ok((view.spec, false));
        }
        let seek = i64::try_from(seek_us).map_err(|_| Error::InvalidInput("retained seek"))?;
        if now_ms < 0 {
            return Err(Error::InvalidInput("retained reader clock"));
        }
        let spec = resolve(&tx, id, recording_id, seek)?;
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
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
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
        let work = QueryWork::start(&self.connection, Limits::TASK_EVIDENCE)?;
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
    connection.execute("INSERT INTO retained_readers(id,recording_id,seek_us,generation,source_revision,ordinal,object_key,sha256,format,bytes,timeline_start_us,timeline_end_us,file_seek_us,file_duration_us,spec_sha256,state,admitted_ms,updated_ms) VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'running',?15,?15)", params![s.request_id,s.recording_id,seek,s.source_revision,s.ordinal,s.object_key,s.sha256,s.format,read::signed(s.bytes)?,read::signed(s.timeline_start_us)?,read::signed(s.timeline_end_us)?,read::signed(s.file_seek_us)?,read::signed(s.file_duration_us)?,s.spec_sha256,now_ms])?;
    Ok(())
}
