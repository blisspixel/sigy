//! Language evidence describes observations, processing outcomes, and route capability separately.

use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLanguageEvidence(pub &'static str);

impl fmt::Display for InvalidLanguageEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for InvalidLanguageEvidence {}

macro_rules! evidence_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl FromStr for $name {
            type Err = InvalidLanguageEvidence;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok(Self::$variant)),+,
                    _ => Err(InvalidLanguageEvidence(concat!("invalid ", stringify!($name)))),
                }
            }
        }
    };
}

evidence_enum!(Observation {
    Identified => "identified",
    Mixed => "mixed",
    Unknown => "unknown",
    NonSpeech => "non_speech",
});

evidence_enum!(DetectionOutcome {
    NotAttempted => "not_attempted",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
    Interrupted => "interrupted",
    UnavailableInput => "unavailable_input",
});

evidence_enum!(RouteCapability {
    Unevaluated => "unevaluated",
    Supported => "supported",
    Unsupported => "unsupported",
});

evidence_enum!(LanguageTask {
    Detection => "language_detection",
    Transcription => "transcription",
    EnglishTranslation => "translation_en",
});

evidence_enum!(EvidenceOrigin {
    Acoustic => "acoustic",
    Recognizer => "recognizer",
    Text => "text",
});

evidence_enum!(Resolution {
    Block => "block",
    SpeechSpan => "speech_span",
    Word => "word",
});

pub const MAX_LANGUAGE_LABELS: usize = 8;
pub const MAX_LANGUAGE_SPANS: usize = 1024;

impl Observation {
    /// Validate label cardinality without converting missing evidence into an observation.
    /// # Errors
    /// Rejects identified speech without one label and labels on unknown/non-speech.
    /// Mixed labels name known members; other members may remain unidentified.
    pub const fn validate_labels(self, count: usize) -> Result<(), InvalidLanguageEvidence> {
        let valid = match self {
            Self::Identified => count == 1,
            Self::Mixed => count <= MAX_LANGUAGE_LABELS,
            Self::Unknown | Self::NonSpeech => count == 0,
        };
        if valid {
            Ok(())
        } else {
            Err(InvalidLanguageEvidence("language observation label count"))
        }
    }
}

impl DetectionOutcome {
    /// # Errors
    /// A failed or unattempted detector cannot publish observations. Success needs measured coverage.
    pub const fn validate_spans(
        self,
        count: usize,
        has_reason: bool,
    ) -> Result<(), InvalidLanguageEvidence> {
        let valid = match self {
            Self::Succeeded => count > 0 && count <= MAX_LANGUAGE_SPANS && !has_reason,
            _ => count == 0 && has_reason,
        };
        if valid {
            Ok(())
        } else {
            Err(InvalidLanguageEvidence(
                "language outcome and observations disagree",
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaRange {
    start_us: u64,
    end_us: u64,
}

impl MediaRange {
    /// # Errors
    /// Refuses empty, inverted, or catalog-unrepresentable ranges.
    pub const fn new(start_us: u64, end_us: u64) -> Result<Self, InvalidLanguageEvidence> {
        if end_us <= start_us || end_us > i64::MAX as u64 {
            return Err(InvalidLanguageEvidence("language media range"));
        }
        Ok(Self { start_us, end_us })
    }

    #[must_use]
    pub const fn start_us(self) -> u64 {
        self.start_us
    }

    #[must_use]
    pub const fn end_us(self) -> u64 {
        self.end_us
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.start_us <= other.start_us && other.end_us <= self.end_us
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.start_us < other.end_us && other.start_us < self.end_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absence_failure_and_unknown_are_not_interchangeable() {
        assert!(
            DetectionOutcome::NotAttempted
                .validate_spans(1, true)
                .is_err()
        );
        assert!(DetectionOutcome::Failed.validate_spans(1, true).is_err());
        assert!(DetectionOutcome::Failed.validate_spans(0, true).is_ok());
        assert!(
            DetectionOutcome::Succeeded
                .validate_spans(0, false)
                .is_err()
        );
        assert!(Observation::Unknown.validate_labels(0).is_ok());
        assert!(Observation::Unknown.validate_labels(1).is_err());
        assert!("directory_hint".parse::<EvidenceOrigin>().is_err());
    }

    #[test]
    fn mixed_can_preserve_unresolved_members() {
        assert!(Observation::Mixed.validate_labels(0).is_ok());
        assert!(Observation::Mixed.validate_labels(2).is_ok());
        assert!(Observation::Mixed.validate_labels(1).is_ok());
        assert!(Observation::Mixed.validate_labels(9).is_err());
        assert!(Observation::Identified.validate_labels(1).is_ok());
        assert!(Observation::NonSpeech.validate_labels(1).is_err());
    }

    #[test]
    fn ranges_are_half_open_and_do_not_wrap() -> Result<(), InvalidLanguageEvidence> {
        let first = MediaRange::new(0, 10)?;
        let second = MediaRange::new(10, 20)?;
        assert!(!first.intersects(second));
        assert!(first.contains(MediaRange::new(1, 10)?));
        assert!(!first.contains(MediaRange::new(1, 11)?));
        assert!(MediaRange::new(10, 10).is_err());
        assert!(MediaRange::new(0, u64::MAX).is_err());
        Ok(())
    }
}
