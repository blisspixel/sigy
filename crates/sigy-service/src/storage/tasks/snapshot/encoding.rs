//! Capped serialization reserves mandatory lineage before optional citations.

use std::io::{self, Write};

use crate::{
    Error, Result,
    task::{
        evidence::TaskOutcome,
        snapshot::{MAX_SNAPSHOT_BYTES, TaskEvidenceSnapshot},
    },
};

struct Capped {
    bytes: Vec<u8>,
    overflowed: bool,
}

impl Default for Capped {
    fn default() -> Self {
        Self {
            bytes: Vec::with_capacity(MAX_SNAPSHOT_BYTES),
            overflowed: false,
        }
    }
}

impl Write for Capped {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_SNAPSHOT_BYTES - self.bytes.len() {
            self.overflowed = true;
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "snapshot byte limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode(snapshot: &TaskEvidenceSnapshot) -> Result<Option<String>> {
    let mut output = Capped::default();
    let result = serde_json::to_writer(&mut output, snapshot);
    if output.overflowed {
        return Ok(None);
    }
    result?;
    String::from_utf8(output.bytes)
        .map(Some)
        .map_err(|_| Error::StorageIntegrity)
}

/// Mandatory metadata must fit by itself. Citations are appended only while it fits.
pub(in crate::storage::tasks) fn fit(snapshot: &mut TaskEvidenceSnapshot) -> Result<String> {
    let citations = std::mem::take(&mut snapshot.evidence.citations);
    let mut trial = snapshot.clone();
    if !citations.is_empty() {
        trial.evidence.more = true;
        trial
            .evidence
            .reasons
            .push("snapshot-citations-truncated".into());
        trial.evidence.outcome = TaskOutcome::Partial;
    }
    if encode(&trial)?.is_none() {
        return Err(Error::InvalidInput("task snapshot mandatory size"));
    }
    for citation in citations {
        snapshot.evidence.citations.push(citation);
        // Reserve the stop reason even when the complete output would just fit.
        trial
            .evidence
            .citations
            .clone_from(&snapshot.evidence.citations);
        if encode(&trial)?.is_none() {
            snapshot.evidence.citations.pop();
            snapshot.evidence.more = trial.evidence.more;
            snapshot.evidence.reasons = trial.evidence.reasons;
            snapshot.evidence.outcome = trial.evidence.outcome;
            break;
        }
    }
    encode(snapshot)?.ok_or(Error::InvalidInput("task snapshot size"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_byte_ceiling_rejects_growth_before_copying() -> io::Result<()> {
        let mut output = Capped::default();
        let capacity = output.bytes.capacity();
        let block = vec![b'a'; MAX_SNAPSHOT_BYTES];
        assert_eq!(output.write(&block)?, MAX_SNAPSHOT_BYTES);
        assert_eq!(output.bytes.len(), MAX_SNAPSHOT_BYTES);
        assert_eq!(output.bytes.capacity(), capacity);
        assert!(output.write(b"b").is_err());
        assert!(output.overflowed);
        assert_eq!(output.bytes, block);
        assert_eq!(output.bytes.capacity(), capacity);
        let mut fresh = Capped::default();
        let oversized = vec![b'a'; MAX_SNAPSHOT_BYTES + 1];
        assert!(fresh.write(&oversized).is_err());
        assert!(fresh.bytes.is_empty());
        assert_eq!(fresh.bytes.capacity(), capacity);
        Ok(())
    }

    #[test]
    fn serialized_bytes_include_escape_expansion_and_exact_boundary() -> Result<()> {
        let mut exact = Capped::default();
        // JSON quotes add two bytes, so a valid string lands exactly on the ceiling.
        serde_json::to_writer(&mut exact, &"x".repeat(MAX_SNAPSHOT_BYTES - 2))?;
        assert_eq!(exact.bytes.len(), MAX_SNAPSHOT_BYTES);
        let mut escaped = Capped::default();
        let capacity = escaped.bytes.capacity();
        assert!(serde_json::to_writer(&mut escaped, &"\"".repeat(MAX_SNAPSHOT_BYTES / 2)).is_err());
        assert!(escaped.overflowed);
        assert!(escaped.bytes.len() <= MAX_SNAPSHOT_BYTES);
        assert_eq!(escaped.bytes.capacity(), capacity);
        Ok(())
    }
}
