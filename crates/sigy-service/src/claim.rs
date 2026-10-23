//! A manual claim over findings that are already stored.
//!
//! The user writes the text, links a stored finding, and sets any status.
//! Nothing in a feed, transcript, or proposal inserts a row. The page has no
//! truth score and does not read an anomaly record.

use serde::{Deserialize, Serialize};

use crate::monitor::FindingOriginal;

/// Most claims stored in one library.
pub const CLAIM_LIMIT: i64 = 256;
/// Most text revisions of one claim. Revision 1 is the create.
pub const TEXT_REVISION_LIMIT: i64 = 64;
/// Most sightings linked from one claim.
pub const SIGHTING_LIMIT: i64 = 32;
/// Most evidence links on one claim.
pub const EVIDENCE_LIMIT: i64 = 64;
/// Most status revisions of one claim. Unverified stores no row.
pub const STATUS_LIMIT: i64 = 32;
/// Most characters in claim text.
pub const TEXT_CHARS: usize = 2_000;
/// Most characters in an evidence note.
pub const NOTE_CHARS: usize = 500;

/// What the user says an evidence note does. This is not a classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimEvidenceKind {
    /// The user says the sighting supports the claim.
    Supports,
    /// The user says the sighting disputes the claim.
    Disputes,
    /// The user says the sighting is context, not support or dispute.
    Context,
    /// The user leaves the relation unresolved.
    Unresolved,
}

impl ClaimEvidenceKind {
    /// Token stored in the catalog and printed in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::Disputes => "disputes",
            Self::Context => "context",
            Self::Unresolved => "unresolved",
        }
    }
}

/// A label the user attaches. Unsupported wording stays unresolved instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimRelation {
    /// The user says one passage quotes another.
    Quotation,
    /// The user says one passage negates another.
    Negation,
    /// The user says the wording is sarcasm. That label is not support.
    Sarcasm,
    /// The user says a date changed.
    ChangedDate,
    /// The user says the wording is a translation.
    Translation,
    /// The user says the passages share a press release.
    SharedPressRelease,
    /// The user says one passage corrects another.
    Correction,
}

impl ClaimRelation {
    /// Token stored in the catalog and printed in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quotation => "quotation",
            Self::Negation => "negation",
            Self::Sarcasm => "sarcasm",
            Self::ChangedDate => "changed-date",
            Self::Translation => "translation",
            Self::SharedPressRelease => "shared-press-release",
            Self::Correction => "correction",
        }
    }
}

/// A status the user sets. Unverified is the absence of a row, not a token stored at create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    /// The user marks the claim disputed.
    Disputed,
    /// The user marks the claim corroborated.
    Corroborated,
    /// The user marks the claim retracted or corrected by its source.
    Retracted,
}

impl ClaimStatus {
    /// Catalog token. The human label for retracted is longer.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disputed => "disputed",
            Self::Corroborated => "corroborated",
            Self::Retracted => "retracted",
        }
    }

    /// Sentence shown to a person.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disputed => "disputed",
            Self::Corroborated => "corroborated",
            Self::Retracted => "retracted or corrected by source",
        }
    }
}

/// What this page examined. It is stated before the sightings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimTimeline {
    /// The only material this page searched.
    pub searched: String,
    /// Phase 1 does not search the library beyond the linked findings.
    pub library_search: bool,
    /// Language coverage outside the cited cues.
    pub languages: String,
    /// The cited intervals, or `none` when no retained interval is linked.
    pub window: String,
    /// Material this page did not examine.
    pub gaps: String,
}

impl ClaimTimeline {
    /// Searched-sources sentence for this phase.
    pub const SEARCHED: &'static str = "linked findings only";
    /// Language sentence. Cited cues are not a library language survey.
    pub const LANGUAGES: &'static str = "not assessed outside the cited cues";
    /// Gap sentence. It is printed before the first sighting.
    pub const GAPS: &'static str = "Material outside the linked findings was not examined.";
}

/// One append-only wording of the claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimTextRevision {
    /// Revision number, starting at 1.
    pub revision: i64,
    /// The user's words at that revision.
    pub text: String,
    /// When this revision was stored, in Unix milliseconds.
    pub created_ms: i64,
}

/// One link to a stored finding. The cited range is that finding, not a new range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimSighting {
    /// Sighting id, unique within the claim.
    pub id: String,
    /// Monitor that owns the finding.
    pub monitor_id: String,
    /// Stored finding id.
    pub finding_id: String,
    /// Recording cited by the finding.
    pub recording_id: String,
    /// Source revision of that recording, or `unknown` when the job is gone.
    pub source_revision: String,
    /// Source name when the revision is still in the catalog.
    pub source_name: Option<String>,
    /// What the finding says about the original audio.
    pub original: FindingOriginal,
    /// Retained cue start. Expired and missing findings store none.
    pub start_us: Option<u64>,
    /// Retained cue end. Expired and missing findings store none.
    pub end_us: Option<u64>,
    /// Clock this lane uses. Expired and missing lanes have no retained interval.
    pub time_basis: String,
    /// A newer original-text revision exists. The sighting is not rewritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_transcript: Option<bool>,
    /// A newer translation of the cited revision exists. The sighting is not rewritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_translation: Option<bool>,
}

/// One user evidence link. Origin is always `user`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimEvidenceLink {
    /// Evidence id, unique within the claim.
    pub id: String,
    /// Sighting on this same claim.
    pub sighting_id: String,
    /// Support, dispute, context, or unresolved.
    pub kind: ClaimEvidenceKind,
    /// Always `user` in this phase.
    pub origin: String,
    /// The user's note.
    pub note: String,
    /// The user's relation label, or `unspecified`.
    pub relation: String,
}

/// Why the latest status may no longer match the evidence it cites.
///
/// These names are read from the cited finding. They are not a new status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimBasisReason {
    /// A newer transcript revision has kind `correction` and outcome `text`.
    Correction,
    /// A newer recognition of the same transcript, or a newer translation of the cited revision.
    Superseded,
    /// The cited finding states that the original audio is missing.
    MissingMedia,
    /// The cited finding states that the original audio has expired.
    RetentionExpired,
}

impl ClaimBasisReason {
    /// Token printed in JSON and on the claim page.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Correction => "correction",
            Self::Superseded => "superseded",
            Self::MissingMedia => "missing-media",
            Self::RetentionExpired => "retention-expired",
        }
    }
}

/// Whether the latest status still cites the evidence the user used.
///
/// `none` is unverified. `current` means no listed change applies.
/// `review` names those changes and leaves the stored status in place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimBasis {
    /// `none`, `current`, or `review`.
    pub state: String,
    /// Evidence id cited by the latest status. Absent while unverified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    /// Sighting that evidence cites. Absent while unverified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sighting_id: Option<String>,
    /// Finding that sighting cites. Absent while unverified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    /// Changes that apply to that finding, in a fixed order. Empty when none apply.
    pub reasons: Vec<ClaimBasisReason>,
}

impl ClaimBasis {
    /// No status row exists, so there is no decision to review.
    #[must_use]
    pub fn unverified() -> Self {
        Self {
            state: "none".to_owned(),
            evidence_id: None,
            sighting_id: None,
            finding_id: None,
            reasons: Vec::new(),
        }
    }
}

/// One source lane. Several sightings of the same source revision share a lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimCarrier {
    /// Source revision, or `unknown`.
    pub source_revision: String,
    /// Source name when it is still stored.
    pub source_name: Option<String>,
    /// Sightings in that lane, in link order.
    pub sighting_ids: Vec<String>,
    /// Repetition is not identified. Grouping is not corroboration.
    pub repetition_group: String,
    /// Independence is unknown until provenance is checked.
    pub independence: String,
}

/// One claim, including every text revision and the current status.
///
/// Field order puts the timeline before the sightings. There is no score field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimView {
    /// Claim id.
    pub id: String,
    /// When the claim was created, in Unix milliseconds.
    pub created_ms: i64,
    /// Latest text revision.
    pub text_revision: i64,
    /// Latest text.
    pub text: String,
    /// Every text revision, oldest first. A revise appends. It does not erase.
    pub revisions: Vec<ClaimTextRevision>,
    /// Searched sources, languages, window, and gaps. This precedes sightings.
    pub timeline: ClaimTimeline,
    /// Links to stored findings.
    pub sightings: Vec<ClaimSighting>,
    /// User evidence links.
    pub evidence: Vec<ClaimEvidenceLink>,
    /// `unverified` when no status row exists. Otherwise the latest token.
    pub status: String,
    /// Human status sentence.
    pub status_label: String,
    /// Count of status rows. Zero means unverified.
    pub status_revision: i64,
    /// Who set the latest status. Absent while unverified.
    pub status_setter: Option<String>,
    /// Evidence id cited by the latest status. Absent while unverified.
    pub status_evidence: Option<String>,
    /// Whether that evidence still matches the cited finding. The status row is unchanged.
    pub basis: ClaimBasis,
    /// One lane per source revision. Independence stays unknown.
    pub carriers: Vec<ClaimCarrier>,
}
