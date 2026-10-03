//! Exact task coverage is a distinct canonical briefing origin, never monitor coverage.

use super::{
    Request, group_ordinals, header, insert_header, insert_members, number,
    task::{checked_ids, selected_citations},
};
use crate::{
    Error, Result,
    storage::Store,
    task::{
        run::{TaskEvidenceBriefing, TaskEvidenceBriefingMember},
        snapshot::TaskEvidenceSnapshot,
    },
};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn is_exact(connection: &Connection, monitor: &str, id: &str) -> Result<bool> {
    Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM task_briefing_evidence WHERE monitor_id=?1 AND briefing_id=?2)",params![monitor,id],|r|r.get(0))?)
}

pub(in crate::storage) fn write_task_evidence_briefing(
    connection: &Connection,
    id: &str,
    snapshot: &TaskEvidenceSnapshot,
    finding_ids: &[String],
    now: i64,
) -> Result<()> {
    let monitor = &snapshot.scope.monitor_id;
    let request = Request {
        monitor,
        id,
        from_ms: snapshot.scope.from_ms,
        to_ms: snapshot.scope.to_ms,
        now,
    };
    super::validate_briefing(&request)?;
    if header(connection, monitor, id)?.is_some() {
        return Err(Error::Analysis("briefing-conflict"));
    }
    let ids = checked_ids(finding_ids)?;
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM monitor_briefings WHERE monitor_id=?1",
        [monitor],
        |r| r.get(0),
    )?;
    if count >= 1024 {
        return Err(Error::Analysis("briefing-limit"));
    }
    let selected = selected_citations(connection, monitor, &ids)?;
    let groups = group_ordinals(&selected);
    let distinct = groups.iter().max().map_or(0, |ordinal| ordinal + 1);
    insert_header(
        connection,
        &request,
        count + 1,
        i64::from(snapshot.scope.monitor_version),
        i64::try_from(distinct).map_err(|_| Error::StorageIntegrity)?,
    )?;
    insert_members(connection, &request, &selected, &groups)?;
    let digest: String = connection.query_row(
        "SELECT payload_sha256 FROM task_evidence_snapshots WHERE task_id=?1 AND ordinal=?2",
        params![snapshot.task_id, snapshot.ordinal],
        |r| r.get(0),
    )?;
    connection.execute("INSERT INTO task_briefing_evidence(monitor_id,briefing_id,task_id,snapshot_ordinal,observation_sha256) VALUES (?1,?2,?3,?4,?5)",params![monitor,id,snapshot.task_id,snapshot.ordinal,digest])?;
    Ok(())
}

impl Store {
    pub(in crate::storage) fn task_evidence_briefing_record(
        &self,
        task: &str,
        id: &str,
    ) -> Result<TaskEvidenceBriefing> {
        let (monitor,ordinal,digest):(String,u32,String)=self.connection.query_row("SELECT monitor_id,snapshot_ordinal,observation_sha256 FROM task_briefing_evidence WHERE task_id=?1 AND briefing_id=?2",params![task,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(Error::NotFound)?;
        let snapshot = self.checked_evidence_snapshot(task, ordinal)?;
        let actual: String = self.connection.query_row(
            "SELECT payload_sha256 FROM task_evidence_snapshots WHERE task_id=?1 AND ordinal=?2",
            params![task, ordinal],
            |r| r.get(0),
        )?;
        let saved = header(&self.connection, &monitor, id)?.ok_or(Error::StorageIntegrity)?;
        if actual != digest
            || monitor != snapshot.scope.monitor_id
            || saved.monitor_version != i64::from(snapshot.scope.monitor_version)
            || saved.from_ms != snapshot.scope.from_ms
            || saved.to_ms != snapshot.scope.to_ms
            || saved.classification != "off"
        {
            return Err(Error::StorageIntegrity);
        }
        let created: i64 = self.connection.query_row(
            "SELECT created_ms FROM monitor_briefings WHERE monitor_id=?1 AND id=?2",
            params![monitor, id],
            |r| r.get(0),
        )?;
        let mut statement=self.connection.prepare("SELECT finding_id,group_ordinal FROM monitor_briefing_members WHERE monitor_id=?1 AND briefing_id=?2 ORDER BY group_ordinal,finding_id LIMIT 65")?;
        let members = statement
            .query_map(params![monitor, id], |r| {
                Ok(TaskEvidenceBriefingMember {
                    finding_id: r.get(0)?,
                    group_ordinal: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if members.len() > 64 {
            return Err(Error::StorageIntegrity);
        }
        Ok(TaskEvidenceBriefing {
            task_id: task.into(),
            id: id.into(),
            monitor_id: monitor,
            generation: number(saved.generation)?,
            created_ms: created,
            snapshot: Box::new(snapshot),
            members,
        })
    }
}
