//! Manual claims over local IPC. Nothing here captures, fetches, or spends.

use serde::{Deserialize, Serialize};

use super::{Snapshot, snapshot};
use crate::{
    Result,
    archive::ArchiveQuery,
    claim::{ClaimCandidatePage, ClaimEvidenceKind, ClaimRelation, ClaimStatus, ClaimView},
    storage::{Store, claims::EvidenceRequest, claims::PassageRequest, claims::StatusRequest},
};

/// One explicit claim command. A model proposal is not a variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClaimOperation {
    /// Store one claim. The same id and first text change nothing.
    Create { id: String, text: String },
    /// Append a text revision after the revision the user last saw.
    Revise {
        id: String,
        expected_revision: i64,
        text: String,
    },
    /// List claim ids.
    List {},
    /// Read one claim, including stale citation flags.
    Show { id: String },
    /// Link one stored finding. A cue that is not a finding uses [`Self::Passage`].
    Sighting {
        claim: String,
        sighting: String,
        monitor: String,
        finding: String,
    },
    /// Link one retained cue. This does not insert a finding.
    Passage {
        claim: String,
        sighting: String,
        transcript_id: String,
        transcript_revision: i64,
        translation_revision: i64,
        cue_ordinal: u32,
    },
    /// Store one user evidence link.
    Evidence {
        claim: String,
        evidence: String,
        sighting: String,
        kind: ClaimEvidenceKind,
        note: String,
        #[serde(default)]
        relation: Option<ClaimRelation>,
    },
    /// Append a status the user set. It cites one stored evidence link.
    Status {
        claim: String,
        expected_revision: i64,
        status: ClaimStatus,
        evidence: String,
    },
    /// Read search hits that a stored finding already cites. This writes nothing.
    Candidates { query: ArchiveQuery },
}

/// A claim list or one claim page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClaimPage {
    /// Claim ids in identifier order.
    List { ids: Vec<String> },
    /// One claim. The timeline precedes the sightings.
    Claim { claim: Box<ClaimView> },
    /// Search hits that already are stored findings. No claim row is written.
    Candidates { page: Box<ClaimCandidatePage> },
}

pub(super) fn apply(store: &mut Store, command: ClaimOperation) -> Result<Snapshot> {
    let now = crate::storage::now_ms()?;
    let page = match command {
        ClaimOperation::Create { id, text } => ClaimPage::Claim {
            claim: Box::new(store.create_claim(&id, &text, now)?),
        },
        ClaimOperation::Revise {
            id,
            expected_revision,
            text,
        } => ClaimPage::Claim {
            claim: Box::new(store.revise_claim(&id, expected_revision, &text, now)?),
        },
        ClaimOperation::List {} => ClaimPage::List {
            ids: store.claim_ids()?,
        },
        ClaimOperation::Show { id } => ClaimPage::Claim {
            claim: Box::new(store.claim(&id)?),
        },
        ClaimOperation::Sighting {
            claim,
            sighting,
            monitor,
            finding,
        } => ClaimPage::Claim {
            claim: Box::new(store.add_sighting(&claim, &sighting, &monitor, &finding, now)?),
        },
        ClaimOperation::Passage {
            claim,
            sighting,
            transcript_id,
            transcript_revision,
            translation_revision,
            cue_ordinal,
        } => ClaimPage::Claim {
            claim: Box::new(store.add_passage(
                PassageRequest {
                    claim: &claim,
                    sighting: &sighting,
                    transcript_id: &transcript_id,
                    transcript_revision,
                    translation_revision,
                    cue_ordinal,
                },
                now,
            )?),
        },
        ClaimOperation::Evidence {
            claim,
            evidence,
            sighting,
            kind,
            note,
            relation,
        } => ClaimPage::Claim {
            claim: Box::new(store.add_evidence(
                EvidenceRequest {
                    claim: &claim,
                    evidence: &evidence,
                    sighting: &sighting,
                    kind,
                    note: &note,
                    relation,
                },
                now,
            )?),
        },
        ClaimOperation::Status {
            claim,
            expected_revision,
            status,
            evidence,
        } => ClaimPage::Claim {
            claim: Box::new(store.set_claim_status(
                StatusRequest {
                    claim: &claim,
                    expected_revision,
                    status,
                    evidence: &evidence,
                },
                now,
            )?),
        },
        ClaimOperation::Candidates { query } => ClaimPage::Candidates {
            page: Box::new(store.claim_candidates(query)?),
        },
    };
    let mut view = snapshot(store)?;
    view.claim = Some(Box::new(page));
    Ok(view)
}
