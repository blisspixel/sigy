use super::{Operation, Snapshot};
use crate::{
    Error, Result,
    domain::money::Usd,
    library::Library,
    storage::{
        Store,
        analysis::{AdmitOutcome, AnalysisHole, AnalysisRecord, AnalysisSpan},
        transcripts::{LocalTranscript, TranscriptOutcome},
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
    /// Store one local original-script revision. No paid request is reserved.
    Transcribe {
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
    Transcribed,
    TranscriptUnchanged,
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
pub struct TranscriptCueView {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub script: String,
    pub wording: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptView {
    pub revision: i64,
    pub role: String,
    pub profile: String,
    pub cues: Vec<TranscriptCueView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisDecisionView {
    pub amount_usd: String,
    pub paid_request: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisPage {
    pub input: AnalysisView,
    pub disposition: Option<AnalysisDisposition>,
    #[serde(default)]
    pub transcript: Option<TranscriptView>,
    #[serde(default)]
    pub decision: Option<AnalysisDecisionView>,
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
        AnalysisOperation::Transcribe { .. } => {
            return Err(Error::InvalidInput(
                "transcription requires library ownership",
            ));
        }
    };
    finish(store, record, disposition)
}

/// Hash the retained files, then store one original-script revision and a zero-USD decision.
/// # Errors
/// Returns validation or catalog errors. Does not reserve a paid request.
pub(super) fn transcribe(library: &mut Library, id: &str, revision: i64) -> Result<Snapshot> {
    let directory = library.directory().to_path_buf();
    let now = crate::storage::now_ms()?;
    let (outcome, _) = library
        .store_mut()
        .commit_local_transcript(&directory, id, revision, now)?;
    let disposition = match outcome {
        TranscriptOutcome::Created => AnalysisDisposition::Transcribed,
        TranscriptOutcome::Unchanged => AnalysisDisposition::TranscriptUnchanged,
    };
    let record = library.store().published_analysis(id, revision)?;
    finish(library.store(), record, Some(disposition))
}

fn finish(
    store: &Store,
    record: AnalysisRecord,
    disposition: Option<AnalysisDisposition>,
) -> Result<Snapshot> {
    let analysis = page(store, record, disposition)?;
    let mut view = super::snapshot(store)?;
    view.analysis = Some(analysis);
    Ok(view)
}

fn page(
    store: &Store,
    record: AnalysisRecord,
    disposition: Option<AnalysisDisposition>,
) -> Result<AnalysisPage> {
    let stored = store.local_transcript(&record.id, record.revision)?;
    let (transcript, decision) = match stored {
        Some(value) => (Some(transcript_view(&value)), Some(decision_view(&value)?)),
        None => (None, None),
    };
    Ok(AnalysisPage {
        input: view_from(record),
        disposition,
        transcript,
        decision,
    })
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

fn transcript_view(value: &LocalTranscript) -> TranscriptView {
    TranscriptView {
        revision: value.revision,
        role: value.role.clone(),
        profile: value.profile.clone(),
        cues: value
            .cues
            .iter()
            .map(|cue| TranscriptCueView {
                ordinal: cue.ordinal,
                start_us: cue.start_us,
                end_us: cue.end_us,
                script: cue.script.clone(),
                wording: cue.wording.clone(),
            })
            .collect(),
    }
}

fn decision_view(value: &LocalTranscript) -> Result<AnalysisDecisionView> {
    if value.amount_micros != 0 {
        return Err(Error::StorageIntegrity);
    }
    Ok(AnalysisDecisionView {
        amount_usd: Usd::from_micros(value.amount_micros)?.to_string(),
        paid_request: false,
    })
}

impl From<AnalysisOperation> for Operation {
    fn from(command: AnalysisOperation) -> Self {
        Self::Analysis { command }
    }
}
