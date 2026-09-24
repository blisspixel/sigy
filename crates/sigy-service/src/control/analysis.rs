use super::{Operation, Snapshot};
use crate::recognition::{LocalAsrJob, RecognitionProfile, TranscriptCuePage};
use crate::translation::{TranslationJob, TranslationPage, TranslationProfile};
use crate::{
    Error, Result,
    domain::money::Usd,
    storage::{
        Store,
        analysis::{AdmitOutcome, AnalysisHole, AnalysisRecord, AnalysisSpan},
        languages::LanguagePage,
        transcripts::LocalTranscript,
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
    /// Run one supervised local recognizer on a published pin. The job ID is the
    /// idempotency key. No source URL is read and no paid request is reserved.
    Transcribe {
        id: String,
        input: String,
        revision: i64,
        profile: String,
        /// The transcript revision this result follows. Omitted means the current one.
        #[serde(default)]
        parent_revision: Option<i64>,
    },
    /// Read one stored transcript revision's cues. Omitted revision means the newest.
    Transcript {
        id: String,
        #[serde(default)]
        revision: Option<i64>,
        #[serde(default)]
        after: Option<u32>,
    },
    Profile {
        command: ProfileOperation,
    },
    /// Translate one recognized transcript revision into English with a local profile.
    /// Runs in the service. The job ID is the idempotency key. No paid request is made.
    Translate {
        id: String,
        input: String,
        /// Transcript revision to translate. Omitted means the newest.
        #[serde(default)]
        transcript_revision: Option<i64>,
        profile: String,
    },
    /// Read the original and English cue pairs of a stored translation.
    Translation {
        id: String,
        #[serde(default)]
        transcript_revision: Option<i64>,
        #[serde(default)]
        revision: Option<i64>,
        #[serde(default)]
        after: Option<u32>,
    },
    TranslationProfile {
        command: TranslationProfileOperation,
    },
    Languages {
        command: LanguageOperation,
    },
    Verify {
        id: String,
        input: String,
        revision: i64,
    },
    Job {
        id: String,
    },
    Cancel {
        id: String,
        generation: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum LanguageOperation {
    List {
        id: String,
        revision: i64,
        after: Option<String>,
    },
    Show {
        id: String,
        revision: u32,
        after: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileOperation {
    /// Store one immutable profile. Files are re-hashed before every run.
    Add {
        profile: Box<RecognitionProfile>,
    },
    List {},
    Show {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum TranslationProfileOperation {
    /// Store one immutable profile. Files are re-hashed before every run.
    Add {
        profile: Box<TranslationProfile>,
    },
    List {},
    Show {
        id: String,
    },
}

/// Recognition results carried by a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecognitionView {
    Profiles {
        profiles: Vec<RecognitionProfile>,
    },
    Profile {
        profile: RecognitionProfile,
        created: bool,
    },
    Job {
        job: LocalAsrJob,
    },
    Transcript {
        page: TranscriptCuePage,
    },
    Empty {
        id: String,
    },
    TranslationProfiles {
        profiles: Vec<TranslationProfile>,
    },
    TranslationProfile {
        profile: TranslationProfile,
        created: bool,
    },
    TranslationJob {
        job: TranslationJob,
    },
    Translation {
        page: TranslationPage,
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
    #[serde(default)]
    pub languages: Option<LanguagePage>,
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
        AnalysisOperation::Transcript {
            id,
            revision,
            after,
        } => return transcript(store, &id, revision, after),
        AnalysisOperation::Profile { command } => return profile(store, command, now),
        AnalysisOperation::Translation {
            id,
            transcript_revision,
            revision,
            after,
        } => return translation(store, &id, transcript_revision, revision, after),
        AnalysisOperation::TranslationProfile { command } => {
            return translation_profile(store, command, now);
        }
        AnalysisOperation::Languages { command } => return languages(store, command),
        AnalysisOperation::Job { id } => {
            let mut snapshot = super::snapshot(store)?;
            if store.analysis_job_kind(&id)?.is_none()
                && let Ok(job) = store.translation_job(&id)
            {
                snapshot.recognition = Some(Box::new(RecognitionView::TranslationJob { job }));
            } else if store.analysis_job_kind(&id)?.as_deref() == Some("local_asr") {
                snapshot.recognition = Some(Box::new(RecognitionView::Job {
                    job: store.local_asr_job(&id)?,
                }));
            } else {
                snapshot.analysis_job = Some(store.analysis_job(&id)?);
            }
            return Ok(snapshot);
        }
        AnalysisOperation::Verify { .. }
        | AnalysisOperation::Cancel { .. }
        | AnalysisOperation::Transcribe { .. }
        | AnalysisOperation::Translate { .. } => {
            return Err(Error::ServiceRequired);
        }
    };
    finish(store, record, disposition)
}

fn transcript(
    store: &Store,
    id: &str,
    revision: Option<i64>,
    after: Option<u32>,
) -> Result<Snapshot> {
    let revision = match revision {
        Some(revision) => revision,
        None => store.latest_transcript_revision(id)?,
    };
    let view = if revision == 0 {
        RecognitionView::Empty { id: id.to_owned() }
    } else {
        RecognitionView::Transcript {
            page: store.transcript_cues_page(id, revision, after)?,
        }
    };
    let mut snapshot = super::snapshot(store)?;
    snapshot.recognition = Some(Box::new(view));
    Ok(snapshot)
}

fn translation(
    store: &Store,
    id: &str,
    transcript_revision: Option<i64>,
    revision: Option<i64>,
    after: Option<u32>,
) -> Result<Snapshot> {
    let transcript_revision = match transcript_revision {
        Some(value) => value,
        None => store.latest_transcript_revision(id)?,
    };
    let revision = match revision {
        Some(value) => value,
        None => store.latest_translation_revision(id, transcript_revision)?,
    };
    let view = if revision == 0 {
        RecognitionView::Empty { id: id.to_owned() }
    } else {
        RecognitionView::Translation {
            page: store.translation_page(id, transcript_revision, revision, after)?,
        }
    };
    let mut snapshot = super::snapshot(store)?;
    snapshot.recognition = Some(Box::new(view));
    Ok(snapshot)
}

fn translation_profile(
    store: &mut Store,
    command: TranslationProfileOperation,
    now: i64,
) -> Result<Snapshot> {
    let view = match command {
        TranslationProfileOperation::Add { profile } => {
            let created = store.add_translation_profile(&profile, now)?;
            RecognitionView::TranslationProfile {
                profile: store.translation_profile(&profile.id)?,
                created,
            }
        }
        TranslationProfileOperation::List {} => RecognitionView::TranslationProfiles {
            profiles: store.translation_profiles()?,
        },
        TranslationProfileOperation::Show { id } => RecognitionView::TranslationProfile {
            profile: store.translation_profile(&id)?,
            created: false,
        },
    };
    let mut snapshot = super::snapshot(store)?;
    snapshot.recognition = Some(Box::new(view));
    Ok(snapshot)
}

fn profile(store: &mut Store, command: ProfileOperation, now: i64) -> Result<Snapshot> {
    let view = match command {
        ProfileOperation::Add { profile } => {
            let created = store.add_recognition_profile(&profile, now)?;
            RecognitionView::Profile {
                profile: store.recognition_profile(&profile.id)?,
                created,
            }
        }
        ProfileOperation::List {} => RecognitionView::Profiles {
            profiles: store.recognition_profiles()?,
        },
        ProfileOperation::Show { id } => RecognitionView::Profile {
            profile: store.recognition_profile(&id)?,
            created: false,
        },
    };
    let mut snapshot = super::snapshot(store)?;
    snapshot.recognition = Some(Box::new(view));
    Ok(snapshot)
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
        languages: None,
    })
}

fn languages(store: &Store, command: LanguageOperation) -> Result<Snapshot> {
    let (record, languages) = match command {
        LanguageOperation::List {
            id,
            revision,
            after,
        } => (
            store.analysis_revision(&id, revision)?,
            store.language_evidence_list(&id, revision, after.as_deref())?,
        ),
        LanguageOperation::Show {
            id,
            revision,
            after,
        } => {
            let page = store.language_evidence_page(&id, revision, after)?;
            let LanguagePage::Evidence { summary, .. } = &page else {
                return Err(Error::StorageIntegrity);
            };
            (
                store.analysis_revision(&summary.analysis_id, summary.analysis_revision)?,
                page,
            )
        }
    };
    let mut snapshot = super::snapshot(store)?;
    snapshot.analysis = Some(AnalysisPage {
        input: view_from(record),
        disposition: None,
        transcript: None,
        decision: None,
        languages: Some(languages),
    });
    Ok(snapshot)
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
