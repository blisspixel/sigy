//! Pure media-clock chunking.
//!
//! Each chunk is at most 30 seconds of one retained interval. Abutting segments stay
//! separate, a short tail stays its own chunk, and a gap or uncovered hole is never a chunk.
//! This grid admits the job and fixes its manifest. The worker may end a window earlier
//! when a phrase reaches the window edge and audio remains in the segment.

use std::fmt;

/// The longest decoded slice one recognizer process receives.
pub const CHUNK_US: u64 = 30_000_000;
/// The most chunks one recognition job may plan.
pub const MAX_CHUNKS: usize = 1024;

/// One retained audio segment the planner may slice. It is not a catalog type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AsrSegment {
    /// Interval ordinal on the recording.
    pub ordinal: u32,
    /// Catalog object key. The task spec never carries this.
    pub object_key: String,
    pub byte_length: u64,
    pub start_us: u64,
    pub end_us: u64,
    pub source_sha256: String,
    /// Container token such as `wav`. The decoder receives it as a format name, not a URL.
    pub format: String,
}

/// One frozen slice of a segment. Ordinals increase in plan order, starting at zero.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlannedChunk {
    pub ordinal: u32,
    pub interval_ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub source_sha256: String,
}

/// A retained interval, in the order the catalog stored it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChunkSpan {
    pub ordinal: u32,
    pub start_us: u64,
    pub end_us: u64,
    pub source_sha256: String,
}

/// A gap or uncovered hole. Touching an endpoint does not intersect it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkHole {
    pub start_us: u64,
    pub end_us: u64,
}

/// Why a timeline cannot become a recognition plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkError {
    /// No retained interval was supplied.
    Empty,
    /// A span ends at or before it starts.
    Reversed,
    /// A span starts before the previous span ends. Spans are not reordered.
    Overlap,
    /// A span meets a gap or an uncovered hole.
    IntersectsHole,
    /// The plan would exceed [`MAX_CHUNKS`].
    TooMany,
}

impl fmt::Display for ChunkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "empty recognition plan",
            Self::Reversed => "reversed recognition span",
            Self::Overlap => "overlapping recognition span",
            Self::IntersectsHole => "recognition span meets a gap",
            Self::TooMany => "too many recognition chunks",
        })
    }
}

impl std::error::Error for ChunkError {}

/// Slice `spans` in the order given. Holes produce no chunks.
/// # Errors
/// Refuses an empty plan, a reversed or overlapping span, a span that meets a hole,
/// or more than [`MAX_CHUNKS`] slices.
pub(crate) fn plan_chunks(
    spans: &[ChunkSpan],
    holes: &[ChunkHole],
) -> Result<Vec<PlannedChunk>, ChunkError> {
    if spans.is_empty() {
        return Err(ChunkError::Empty);
    }
    let mut chunks = Vec::new();
    let mut previous_end = None;
    for span in spans {
        check_span(span, holes, previous_end)?;
        previous_end = Some(span.end_us);
        append_span(span, &mut chunks)?;
    }
    Ok(chunks)
}

fn check_span(
    span: &ChunkSpan,
    holes: &[ChunkHole],
    previous_end: Option<u64>,
) -> Result<(), ChunkError> {
    if span.end_us <= span.start_us {
        return Err(ChunkError::Reversed);
    }
    if previous_end.is_some_and(|end| span.start_us < end) {
        return Err(ChunkError::Overlap);
    }
    if holes
        .iter()
        .any(|hole| intersects(span.start_us, span.end_us, hole.start_us, hole.end_us))
    {
        return Err(ChunkError::IntersectsHole);
    }
    Ok(())
}

fn intersects(start: u64, end: u64, other_start: u64, other_end: u64) -> bool {
    start < other_end && other_start < end
}

fn append_span(span: &ChunkSpan, chunks: &mut Vec<PlannedChunk>) -> Result<(), ChunkError> {
    let mut cursor = span.start_us;
    while cursor < span.end_us {
        if chunks.len() >= MAX_CHUNKS {
            return Err(ChunkError::TooMany);
        }
        let end = cursor.saturating_add(CHUNK_US).min(span.end_us);
        if end <= cursor {
            return Err(ChunkError::Reversed);
        }
        let ordinal = u32::try_from(chunks.len()).map_err(|_| ChunkError::TooMany)?;
        chunks.push(PlannedChunk {
            ordinal,
            interval_ordinal: span.ordinal,
            start_us: cursor,
            end_us: end,
            source_sha256: span.source_sha256.clone(),
        });
        cursor = end;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CHUNK_US, ChunkError, ChunkHole, ChunkSpan, MAX_CHUNKS, plan_chunks};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn span(ordinal: u32, start_us: u64, end_us: u64) -> ChunkSpan {
        ChunkSpan {
            ordinal,
            start_us,
            end_us,
            source_sha256: "a".repeat(64),
        }
    }

    fn bounds(chunks: &[super::PlannedChunk]) -> Vec<(u32, u64, u64)> {
        chunks
            .iter()
            .map(|chunk| (chunk.ordinal, chunk.start_us, chunk.end_us))
            .collect()
    }

    #[test]
    fn windows_split_at_thirty_seconds_and_a_one_microsecond_tail_stays() -> TestResult {
        let one = plan_chunks(&[span(4, 0, 1)], &[])?;
        assert_eq!(bounds(&one), vec![(0, 0, 1)]);
        assert_eq!(one[0].interval_ordinal, 4);
        let thirty = plan_chunks(&[span(0, 5, 5 + CHUNK_US)], &[])?;
        assert_eq!(bounds(&thirty), vec![(0, 5, 5 + CHUNK_US)]);
        let minute = plan_chunks(&[span(0, 0, CHUNK_US * 2)], &[])?;
        assert_eq!(
            bounds(&minute),
            vec![(0, 0, CHUNK_US), (1, CHUNK_US, CHUNK_US * 2)]
        );
        let tail = plan_chunks(&[span(0, 0, CHUNK_US * 2 + 1)], &[])?;
        assert_eq!(
            bounds(&tail),
            vec![
                (0, 0, CHUNK_US),
                (1, CHUNK_US, CHUNK_US * 2),
                (2, CHUNK_US * 2, CHUNK_US * 2 + 1)
            ]
        );
        let ninety = plan_chunks(&[span(0, 0, CHUNK_US * 3)], &[])?;
        assert_eq!(ninety.len(), 3);
        assert_eq!(ninety[2].end_us - ninety[2].start_us, CHUNK_US);
        Ok(())
    }

    #[test]
    fn abutting_segments_stay_separate_and_holes_are_not_chunks() -> TestResult {
        let five = 5_000_000;
        let segments = plan_chunks(&[span(0, 0, five), span(1, five, five * 2)], &[])?;
        assert_eq!(
            segments
                .iter()
                .map(|chunk| (chunk.interval_ordinal, chunk.start_us, chunk.end_us))
                .collect::<Vec<_>>(),
            vec![(0, 0, five), (1, five, five * 2)]
        );
        let around = plan_chunks(
            &[span(0, 0, five), span(2, five * 2, five * 3)],
            &[ChunkHole {
                start_us: five,
                end_us: five * 2,
            }],
        )?;
        assert_eq!(around.len(), 2);
        assert!(
            around
                .iter()
                .all(|chunk| chunk.end_us <= five || chunk.start_us >= five * 2)
        );
        let touching = plan_chunks(
            &[span(0, 0, five)],
            &[ChunkHole {
                start_us: five,
                end_us: five * 2,
            }],
        )?;
        assert_eq!(touching.len(), 1);
        Ok(())
    }

    #[test]
    fn reversed_overlapping_backward_and_holed_spans_are_refused() {
        let hole = [ChunkHole {
            start_us: 5,
            end_us: 8,
        }];
        assert!(matches!(plan_chunks(&[], &[]), Err(ChunkError::Empty)));
        assert!(matches!(
            plan_chunks(&[span(0, 5, 5)], &[]),
            Err(ChunkError::Reversed)
        ));
        assert!(matches!(
            plan_chunks(&[span(0, 8, 4)], &[]),
            Err(ChunkError::Reversed)
        ));
        assert!(matches!(
            plan_chunks(&[span(0, 0, 10), span(1, 9, 12)], &[]),
            Err(ChunkError::Overlap)
        ));
        assert!(matches!(
            plan_chunks(&[span(1, 10, 20), span(0, 0, 5)], &[]),
            Err(ChunkError::Overlap)
        ));
        assert!(matches!(
            plan_chunks(&[span(0, 0, 10)], &hole),
            Err(ChunkError::IntersectsHole)
        ));
    }

    #[test]
    fn the_chunk_ceiling_is_exact() -> TestResult {
        let width = CHUNK_US * u64::try_from(MAX_CHUNKS)?;
        let full = plan_chunks(&[span(0, 0, width)], &[])?;
        assert_eq!(full.len(), MAX_CHUNKS);
        assert_eq!(full[0].ordinal, 0);
        assert_eq!(full[MAX_CHUNKS - 1].ordinal, u32::try_from(MAX_CHUNKS - 1)?);
        assert!(matches!(
            plan_chunks(&[span(0, 0, width + 1)], &[]),
            Err(ChunkError::TooMany)
        ));
        Ok(())
    }
}
