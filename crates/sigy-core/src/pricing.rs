//! Exact provider prices and worst-case request liability.
//!
//! Catalog rates are USD per unit with up to 18 fractional digits, so they are
//! held as integer attodollars (10^-18 USD). Liabilities are computed exactly and
//! rounded up to the ledger's USD micro-unit only at the end. No floating point
//! is used anywhere in this module.

use std::fmt;

use crate::money::Usd;

/// Fractional digits carried by an exact rate.
pub const FRACTION_DIGITS: usize = 18;
/// Attodollars in one USD.
pub const ATTO_PER_DOLLAR: u128 = 1_000_000_000_000_000_000;
/// Attodollars in one ledger micro-unit.
pub const ATTO_PER_MICRO: u128 = 1_000_000_000_000;
/// Longest accepted price or cost text. Real catalog and usage values are far shorter.
pub const MAX_TEXT_BYTES: usize = 64;
/// Units in the per-million routing price used by provider maximum-price fields.
const UNITS_PER_MILLION: u128 = 1_000_000;
/// Significant digits accepted in a reported cost; 10^36 fits in `u128`.
const MAX_SIGNIFICANT_DIGITS: usize = 36;
/// Exponent digits accepted in a reported cost.
const MAX_EXPONENT_DIGITS: usize = 6;
/// Largest power of ten representable in `u128`.
const MAX_POW10_U128: u32 = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingError {
    Empty,
    TooLong,
    Negative,
    InvalidDecimal,
    InvalidNumber,
    TooPrecise,
    Overflow,
    ZeroCompletionBound,
    UnboundedDimension(Dimension),
    UnrecognizedCharge,
}

impl fmt::Display for PricingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("price text is empty"),
            Self::TooLong => f.write_str("price text exceeds 64 bytes"),
            Self::Negative => f.write_str("price or cost cannot be negative"),
            Self::InvalidDecimal => {
                f.write_str("expected an unsigned decimal USD rate without exponent or whitespace")
            }
            Self::InvalidNumber => f.write_str("expected a JSON number"),
            Self::TooPrecise => f.write_str("value has more precision than can be held exactly"),
            Self::Overflow => f.write_str("value exceeds the accounting range"),
            Self::ZeroCompletionBound => {
                f.write_str("a paid request needs a positive completion token bound")
            }
            Self::UnboundedDimension(dimension) => write!(
                f,
                "route charges for {dimension}, which has no proven unit bound"
            ),
            Self::UnrecognizedCharge => {
                f.write_str("route lists a nonzero charge this policy does not recognize")
            }
        }
    }
}

impl std::error::Error for PricingError {}

/// An exact nonnegative USD price for one billing unit, in attodollars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Rate(u128);

impl Rate {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn from_attodollars(attodollars: u128) -> Self {
        Self(attodollars)
    }

    #[must_use]
    pub const fn attodollars(self) -> u128 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Parses a catalog decimal string such as `"0.00000015"`.
    ///
    /// The whole part has no leading zeros unless it is exactly `0`. A fraction,
    /// when present, has 1 to 18 digits. Signs, exponents, whitespace, grouping
    /// separators, and non-ASCII digits are rejected.
    ///
    /// # Errors
    /// Rejects malformed, negative, overly precise, or out-of-range text.
    pub fn parse(input: &str) -> Result<Self, PricingError> {
        check_text(input)?;
        let (whole, fraction) = match input.split_once('.') {
            Some((whole, fraction)) => (whole, Some(fraction)),
            None => (input, None),
        };
        if !is_canonical_integer(whole) {
            return Err(PricingError::InvalidDecimal);
        }
        let fraction = fraction.unwrap_or_default();
        if (input.contains('.') && fraction.is_empty()) || !all_digits(fraction) {
            return Err(PricingError::InvalidDecimal);
        }
        if fraction.len() > FRACTION_DIGITS {
            return Err(PricingError::TooPrecise);
        }
        let whole = accumulate(0, whole)?;
        let mut fraction_value = accumulate(0, fraction)?;
        for _ in fraction.len()..FRACTION_DIGITS {
            fraction_value = fraction_value
                .checked_mul(10)
                .ok_or(PricingError::Overflow)?;
        }
        whole
            .checked_mul(ATTO_PER_DOLLAR)
            .and_then(|value| value.checked_add(fraction_value))
            .map(Self)
            .ok_or(PricingError::Overflow)
    }

    /// Exact decimal USD for one million units, for provider maximum-price fields
    /// that are denominated per million tokens.
    ///
    /// # Errors
    /// Rejects a rate whose per-million value leaves the exact range.
    pub fn per_million_decimal(self) -> Result<String, PricingError> {
        self.0
            .checked_mul(UNITS_PER_MILLION)
            .map(|value| Self(value).to_string())
            .ok_or(PricingError::Overflow)
    }

    fn cost(self, units: u64) -> Result<u128, PricingError> {
        self.0
            .checked_mul(u128::from(units))
            .ok_or(PricingError::Overflow)
    }
}

impl fmt::Display for Rate {
    /// Canonical exact decimal: no trailing fractional zeros and no bare point.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / ATTO_PER_DOLLAR;
        let fraction = self.0 % ATTO_PER_DOLLAR;
        if fraction == 0 {
            return write!(f, "{whole}");
        }
        let digits = format!("{fraction:018}");
        write!(f, "{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// Billable dimensions listed by a catalog price snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    Prompt,
    Completion,
    Request,
    InternalReasoning,
    InputCacheRead,
    InputCacheWrite,
    Image,
    Audio,
    WebSearch,
}

impl Dimension {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Completion => "completion",
            Self::Request => "request",
            Self::InternalReasoning => "internal_reasoning",
            Self::InputCacheRead => "input_cache_read",
            Self::InputCacheWrite => "input_cache_write",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::WebSearch => "web_search",
        }
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-unit rates for one route. Prompt and completion rates are mandatory;
/// every other dimension starts at zero and must be set by the adapter that read
/// the snapshot. An adapter may leave a dimension at zero only when its source
/// documents absence as no charge, and must report any unknown nonzero key
/// through [`PriceSnapshot::with_unrecognized_charge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceSnapshot {
    prompt: Rate,
    completion: Rate,
    request: Rate,
    internal_reasoning: Rate,
    input_cache_read: Rate,
    input_cache_write: Rate,
    image: Rate,
    audio: Rate,
    web_search: Rate,
    unrecognized: Rate,
}

impl PriceSnapshot {
    #[must_use]
    pub const fn new(prompt: Rate, completion: Rate) -> Self {
        Self {
            prompt,
            completion,
            request: Rate::ZERO,
            internal_reasoning: Rate::ZERO,
            input_cache_read: Rate::ZERO,
            input_cache_write: Rate::ZERO,
            image: Rate::ZERO,
            audio: Rate::ZERO,
            web_search: Rate::ZERO,
            unrecognized: Rate::ZERO,
        }
    }

    #[must_use]
    pub const fn with(mut self, dimension: Dimension, rate: Rate) -> Self {
        match dimension {
            Dimension::Prompt => self.prompt = rate,
            Dimension::Completion => self.completion = rate,
            Dimension::Request => self.request = rate,
            Dimension::InternalReasoning => self.internal_reasoning = rate,
            Dimension::InputCacheRead => self.input_cache_read = rate,
            Dimension::InputCacheWrite => self.input_cache_write = rate,
            Dimension::Image => self.image = rate,
            Dimension::Audio => self.audio = rate,
            Dimension::WebSearch => self.web_search = rate,
        }
        self
    }

    /// Records a listed charge whose billing unit this policy does not know.
    /// Any nonzero value makes the route ineligible.
    #[must_use]
    pub fn with_unrecognized_charge(mut self, rate: Rate) -> Self {
        self.unrecognized = self.unrecognized.max(rate);
        self
    }

    #[must_use]
    pub const fn rate(&self, dimension: Dimension) -> Rate {
        match dimension {
            Dimension::Prompt => self.prompt,
            Dimension::Completion => self.completion,
            Dimension::Request => self.request,
            Dimension::InternalReasoning => self.internal_reasoning,
            Dimension::InputCacheRead => self.input_cache_read,
            Dimension::InputCacheWrite => self.input_cache_write,
            Dimension::Image => self.image,
            Dimension::Audio => self.audio,
            Dimension::WebSearch => self.web_search,
        }
    }

    /// Highest price any one prompt token can be billed at. Cache writes and
    /// reads are alternative billing classes for prompt tokens, so taking the
    /// maximum covers every mix without assuming a cache hit.
    fn worst_prompt(&self) -> Rate {
        self.prompt
            .max(self.input_cache_write)
            .max(self.input_cache_read)
    }

    /// Highest price any one completion-class token can be billed at. The
    /// completion bound covers reasoning and visible output together.
    fn worst_completion(&self) -> Rate {
        self.completion.max(self.internal_reasoning)
    }

    fn check_bounded(&self) -> Result<(), PricingError> {
        for dimension in [Dimension::Image, Dimension::Audio, Dimension::WebSearch] {
            if !self.rate(dimension).is_zero() {
                return Err(PricingError::UnboundedDimension(dimension));
            }
        }
        if self.unrecognized.is_zero() {
            Ok(())
        } else {
            Err(PricingError::UnrecognizedCharge)
        }
    }
}

/// Proven unit bounds for exactly one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitBounds {
    prompt_tokens: u64,
    completion_tokens: u64,
}

impl UnitBounds {
    /// `completion_tokens` must be the enforced total for reasoning plus
    /// visible output, such as a provider `max_completion_tokens` limit.
    ///
    /// # Errors
    /// Rejects a zero completion bound, which providers treat as unset.
    pub const fn new(prompt_tokens: u64, completion_tokens: u64) -> Result<Self, PricingError> {
        if completion_tokens == 0 {
            Err(PricingError::ZeroCompletionBound)
        } else {
            Ok(Self {
                prompt_tokens,
                completion_tokens,
            })
        }
    }

    #[must_use]
    pub const fn prompt_tokens(self) -> u64 {
        self.prompt_tokens
    }

    #[must_use]
    pub const fn completion_tokens(self) -> u64 {
        self.completion_tokens
    }
}

/// Exact worst-case charge of one request and its ledger reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Liability {
    attodollars: u128,
    reserve: Usd,
}

impl Liability {
    /// Computes one request fee, plus prompt tokens at the highest of the prompt,
    /// cache-write, and cache-read rates, plus completion tokens at the higher of
    /// the completion and internal-reasoning rates, rounded up to the ledger
    /// micro-unit.
    ///
    /// # Errors
    /// Rejects any nonzero unbounded or unrecognized dimension and any overflow.
    pub fn worst_case(snapshot: &PriceSnapshot, bounds: UnitBounds) -> Result<Self, PricingError> {
        snapshot.check_bounded()?;
        let prompt = snapshot.worst_prompt().cost(bounds.prompt_tokens)?;
        let completion = snapshot.worst_completion().cost(bounds.completion_tokens)?;
        let attodollars = snapshot
            .request
            .0
            .checked_add(prompt)
            .and_then(|value| value.checked_add(completion))
            .ok_or(PricingError::Overflow)?;
        Ok(Self {
            attodollars,
            reserve: ceil_usd(attodollars)?,
        })
    }

    #[must_use]
    pub const fn attodollars(self) -> u128 {
        self.attodollars
    }

    /// The amount to reserve. It is never below the exact liability.
    #[must_use]
    pub const fn reserve(self) -> Usd {
        self.reserve
    }
}

/// A provider-reported cost parsed from JSON number text without floating point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedCost {
    attodollars: u128,
    sub_attodollar: bool,
    usd: Usd,
}

impl ReportedCost {
    /// Parses RFC 8259 number text such as `"0.000123456789"` or `"1.5e-5"`.
    ///
    /// A nonzero remainder below one attodollar rounds up and is flagged.
    ///
    /// # Errors
    /// Rejects non-JSON text, negative values including `-0`, more than 36
    /// significant digits, exponents over six digits, and values beyond the
    /// ledger range.
    pub fn parse(input: &str) -> Result<Self, PricingError> {
        check_text(input)?;
        let (mantissa, exponent) = match input.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (mantissa, parse_exponent(exponent)?),
            None => (input, 0),
        };
        let (whole, fraction) = match mantissa.split_once('.') {
            Some((whole, fraction)) if !fraction.is_empty() && all_digits(fraction) => {
                (whole, fraction)
            }
            Some(_) => return Err(PricingError::InvalidNumber),
            None => (mantissa, ""),
        };
        if !is_canonical_integer(whole) {
            return Err(PricingError::InvalidNumber);
        }
        let fraction_len = i64::try_from(fraction.len()).map_err(|_| PricingError::TooLong)?;
        let mut scale = exponent - fraction_len + 18;
        let digits = format!("{whole}{fraction}");
        let significant = digits.trim_start_matches('0');
        let trimmed = significant.trim_end_matches('0');
        if trimmed.is_empty() {
            return Ok(Self {
                attodollars: 0,
                sub_attodollar: false,
                usd: Usd::ZERO,
            });
        }
        scale +=
            i64::try_from(significant.len() - trimmed.len()).map_err(|_| PricingError::TooLong)?;
        if trimmed.len() > MAX_SIGNIFICANT_DIGITS {
            return Err(PricingError::TooPrecise);
        }
        let mantissa = accumulate(0, trimmed)?;
        let (attodollars, sub_attodollar) = scale_attodollars(mantissa, scale)?;
        Ok(Self {
            attodollars,
            sub_attodollar,
            usd: ceil_usd(attodollars)?,
        })
    }

    /// Exact value in attodollars, rounded up only when `sub_attodollar` is set.
    #[must_use]
    pub const fn attodollars(self) -> u128 {
        self.attodollars
    }

    #[must_use]
    pub const fn sub_attodollar(self) -> bool {
        self.sub_attodollar
    }

    /// The cost rounded up to the ledger micro-unit.
    #[must_use]
    pub const fn usd(self) -> Usd {
        self.usd
    }
}

fn scale_attodollars(mantissa: u128, scale: i64) -> Result<(u128, bool), PricingError> {
    if scale >= 0 {
        let power = u32::try_from(scale).map_err(|_| PricingError::Overflow)?;
        if power > MAX_POW10_U128 {
            return Err(PricingError::Overflow);
        }
        let value = 10_u128
            .checked_pow(power)
            .and_then(|factor| mantissa.checked_mul(factor))
            .ok_or(PricingError::Overflow)?;
        return Ok((value, false));
    }
    let power = u32::try_from(scale.unsigned_abs()).unwrap_or(u32::MAX);
    if power > MAX_POW10_U128 {
        // The mantissa has at most 36 digits, so it is a nonzero fraction of one attodollar.
        return Ok((1, true));
    }
    let divisor = 10_u128.checked_pow(power).ok_or(PricingError::Overflow)?;
    let value = mantissa.div_ceil(divisor);
    Ok((value, !mantissa.is_multiple_of(divisor)))
}

fn parse_exponent(text: &str) -> Result<i64, PricingError> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..).unwrap_or_default()),
        Some(b'+') => (false, text.get(1..).unwrap_or_default()),
        _ => (false, text),
    };
    if digits.is_empty() || !all_digits(digits) {
        return Err(PricingError::InvalidNumber);
    }
    if digits.len() > MAX_EXPONENT_DIGITS {
        return Err(PricingError::Overflow);
    }
    let value = i64::try_from(accumulate(0, digits)?).map_err(|_| PricingError::Overflow)?;
    Ok(if negative { -value } else { value })
}

fn check_text(input: &str) -> Result<(), PricingError> {
    if input.is_empty() {
        Err(PricingError::Empty)
    } else if input.len() > MAX_TEXT_BYTES {
        Err(PricingError::TooLong)
    } else if input.starts_with('-') {
        Err(PricingError::Negative)
    } else {
        Ok(())
    }
}

fn all_digits(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_canonical_integer(text: &str) -> bool {
    !text.is_empty() && all_digits(text) && (text == "0" || !text.starts_with('0'))
}

fn accumulate(start: u128, digits: &str) -> Result<u128, PricingError> {
    digits.bytes().try_fold(start, |value, byte| {
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u128::from(byte - b'0')))
            .ok_or(PricingError::Overflow)
    })
}

fn ceil_usd(attodollars: u128) -> Result<Usd, PricingError> {
    let micros =
        i64::try_from(attodollars.div_ceil(ATTO_PER_MICRO)).map_err(|_| PricingError::Overflow)?;
    Usd::from_micros(micros).map_err(|_| PricingError::Overflow)
}

#[cfg(test)]
mod tests;
