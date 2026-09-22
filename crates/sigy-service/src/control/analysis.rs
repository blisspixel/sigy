use super::{Operation, Snapshot};
use crate::{
    Result,
    storage::{
        Store,
        analysis::{AdmitOutcome, AnalysisHole, AnalysisRecord, AnalysisSpan},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnalysisOperation {
    /// Pin one completed recording. This does not read a source URL or start a model.
    Admit {
        id: String,
        recording_id: String,
        #[serde(default)]
        replace_worker: bool,
    },
    Show {
        id: String,
    },
    /// Seal the current revision. An older revision cannot publish over it.
    Publish {
        id: String,
        revision: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisDisposition {
    Created,
    Unchanged,
    Replaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisIntervalView {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisGapView {
    pub ordinal: u32,
    pub cause: String,
    pub start_us: u64,
    pub end_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisView {
    pub id: String,
    pub revision: i64,
    pub recording_id: String,
    pub media_sha256: String,
    pub state: String,
    pub planned_us: u64,
    pub intervals: Vec<AnalysisIntervalView>,
    pub gaps: Vec<AnalysisGapView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisPage {
    pub input: AnalysisView,
    pub disposition: Option<AnalysisDisposition>,
}

pub(super) fn apply(store: &mut Store, command: AnalysisOperation) -> Result<Snapshot> {
    let now = crate::storage::now_ms()?;
    let (disposition, record) = match command {
        AnalysisOperation::Admit {
            id,
            recording_id,
            replace_worker,
        } => {
            let (outcome, record) =
                store.admit_analysis(&id, &recording_id, replace_worker, now)?;
            (Some(map_outcome(outcome)), record)
        }
        AnalysisOperation::Show { id } => (None, store.analysis_input(&id)?),
        AnalysisOperation::Publish { id, revision } => {
            (None, store.publish_analysis(&id, revision)?)
        }
    };
    let mut view = super::snapshot(store)?;
    view.analysis = Some(AnalysisPage {
        input: view_from(record),
        disposition,
    });
    Ok(view)
}

fn map_outcome(outcome: AdmitOutcome) -> AnalysisDisposition {
    match outcome {
        AdmitOutcome::Created => AnalysisDisposition::Created,
        AdmitOutcome::Unchanged => AnalysisDisposition::Unchanged,
        AdmitOutcome::Replaced => AnalysisDisposition::Replaced,
    }
}

fn view_from(record: AnalysisRecord) -> AnalysisView {
    AnalysisView {
        id: record.id,
        revision: record.revision,
        recording_id: record.recording_id,
        media_sha256: record.media_sha256,
        state: record.state,
        planned_us: record.planned_us,
        intervals: record.intervals.into_iter().map(interval_view).collect(),
        gaps: record.gaps.into_iter().map(gap_view).collect(),
    }
}

fn interval_view(span: AnalysisSpan) -> AnalysisIntervalView {
    AnalysisIntervalView {
        ordinal: span.ordinal,
        start_us: span.start_us,
        end_us: span.end_us,
        sha256: span.sha256,
    }
}

fn gap_view(gap: AnalysisHole) -> AnalysisGapView {
    AnalysisGapView {
        ordinal: gap.ordinal,
        cause: gap.cause,
        start_us: gap.start_us,
        end_us: gap.end_us,
    }
}

impl From<AnalysisOperation> for Operation {
    fn from(command: AnalysisOperation) -> Self {
        Self::Analysis { command }
    }
}
