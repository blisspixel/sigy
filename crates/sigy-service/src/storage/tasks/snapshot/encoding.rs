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

/// Count exact serialized bytes without retaining another copy of the output.
struct Counted {
    bytes: usize,
    overflowed: bool,
}

impl Write for Counted {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_SNAPSHOT_BYTES - self.bytes {
            self.overflowed = true;
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "snapshot byte limit",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Counted {
    fn citation(&mut self, citation: &crate::task::TaskCitation, separated: bool) -> Result<bool> {
        if separated && self.write(b",").is_err() {
            return Ok(false);
        }
        let result = serde_json::to_writer(&mut *self, citation);
        if self.overflowed {
            return Ok(false);
        }
        result?;
        Ok(true)
    }
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
    let mandatory_bytes = encode(&trial)?
        .ok_or(Error::InvalidInput("task snapshot mandatory size"))?
        .len();
    let mut count = Counted {
        bytes: mandatory_bytes,
        overflowed: false,
    };
    // The empty array's brackets are already counted. Each independent citation
    // adds its exact JSON bytes and, after the first member, one comma. Reserved
    // truncation flags stay fixed during counting; final encoding rechecks the cap.
    for citation in citations {
        if !count.citation(&citation, !snapshot.evidence.citations.is_empty())? {
            snapshot.evidence.more = trial.evidence.more;
            snapshot.evidence.reasons = trial.evidence.reasons;
            snapshot.evidence.outcome = trial.evidence.outcome;
            break;
        }
        snapshot.evidence.citations.push(citation);
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

    #[test]
    fn counted_json_preserves_declared_escape_and_multibyte_lengths() -> Result<()> {
        let value = "é\n\"\\\u{0}";
        // Quotes: 2, UTF-8 é: 2, three short escapes: 6, NUL escape: 6.
        let mut exact = Counted {
            bytes: MAX_SNAPSHOT_BYTES - 16,
            overflowed: false,
        };
        serde_json::to_writer(&mut exact, &value)?;
        assert_eq!(exact.bytes, MAX_SNAPSHOT_BYTES);
        assert!(!exact.overflowed);
        let mut short = Counted {
            bytes: MAX_SNAPSHOT_BYTES - 15,
            overflowed: false,
        };
        assert!(serde_json::to_writer(&mut short, &value).is_err());
        assert!(short.overflowed);
        assert!(short.bytes <= MAX_SNAPSHOT_BYTES);
        let mut array = Counted {
            bytes: 2,
            overflowed: false,
        };
        serde_json::to_writer(&mut array, &"é")?;
        array.write_all(b",")?;
        serde_json::to_writer(&mut array, &"水")?;
        assert_eq!(array.bytes, 12);
        assert_eq!(serde_json::to_vec(&["é", "水"])?.len(), 12);
        Ok(())
    }
}
