//! RFC 8216 attribute lists. Every value is untrusted text.

use crate::{Error, Result};

const MAX_ATTRIBUTES: usize = 64;

#[derive(Clone, Copy)]
enum Value<'a> {
    Quoted(&'a str),
    Plain(&'a str),
}

/// A parsed attribute list. Names are unique; quoted strings cannot contain a quote.
pub(super) struct Attributes<'a> {
    pairs: Vec<(&'a str, Value<'a>)>,
}

impl<'a> Attributes<'a> {
    pub(super) fn parse(text: &'a str) -> Result<Self> {
        let mut pairs: Vec<(&'a str, Value<'a>)> = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            if pairs.len() == MAX_ATTRIBUTES {
                return Err(invalid());
            }
            let (name, after) = rest.split_once('=').ok_or_else(invalid)?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
                || pairs.iter().any(|(existing, _)| *existing == name)
            {
                return Err(invalid());
            }
            let (value, remaining) = if let Some(quoted) = after.strip_prefix('"') {
                let (inner, remaining) = quoted.split_once('"').ok_or_else(invalid)?;
                (Value::Quoted(inner), remaining)
            } else {
                let (plain, remaining) = after.split_at(after.find(',').unwrap_or(after.len()));
                if plain.is_empty() || plain.contains('"') {
                    return Err(invalid());
                }
                (Value::Plain(plain), remaining)
            };
            pairs.push((name, value));
            rest = if remaining.is_empty() {
                remaining
            } else {
                match remaining.strip_prefix(',') {
                    Some(next) if !next.is_empty() => next,
                    _ => return Err(invalid()),
                }
            };
        }
        Ok(Self { pairs })
    }

    fn get(&self, name: &str) -> Option<Value<'a>> {
        self.pairs
            .iter()
            .find(|(existing, _)| *existing == name)
            .map(|(_, value)| *value)
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// A quoted-string value. A plain value under this name is malformed.
    pub(super) fn quoted(&self, name: &str) -> Result<Option<&'a str>> {
        match self.get(name) {
            None => Ok(None),
            Some(Value::Quoted(text)) => Ok(Some(text)),
            Some(Value::Plain(_)) => Err(invalid()),
        }
    }

    /// An enumerated or other unquoted value.
    pub(super) fn plain(&self, name: &str) -> Result<Option<&'a str>> {
        match self.get(name) {
            None => Ok(None),
            Some(Value::Plain(text)) => Ok(Some(text)),
            Some(Value::Quoted(_)) => Err(invalid()),
        }
    }

    /// A decimal-integer value.
    pub(super) fn integer(&self, name: &str) -> Result<Option<u64>> {
        self.plain(name)?.map(decimal_integer).transpose()
    }
}

/// RFC 8216 decimal-integer: ASCII digits only, at most 20 of them.
pub(super) fn decimal_integer(text: &str) -> Result<u64> {
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    text.parse::<u64>().map_err(|_| invalid())
}

/// A non-negative decimal duration in microseconds. Fractions beyond microseconds are dropped.
pub(super) fn duration_us(text: &str) -> Result<u64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, "0"));
    if whole.is_empty()
        || fraction.is_empty()
        || whole.len() > 5
        || fraction.len() > 32
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid());
    }
    let seconds = whole.parse::<u64>().map_err(|_| invalid())?;
    let mut micros = 0_u64;
    for (position, digit) in fraction.bytes().take(6).enumerate() {
        let place = 10_u64.pow(5 - u32::try_from(position).map_err(|_| invalid())?);
        micros += u64::from(digit - b'0') * place;
    }
    seconds
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(micros))
        .ok_or_else(invalid)
}

pub(super) fn invalid() -> Error {
    Error::InvalidInput("unsupported HLS playlist")
}

#[cfg(test)]
mod tests {
    use super::{Attributes, decimal_integer, duration_us};

    #[test]
    fn attribute_lists_keep_quoted_commas_and_reject_malformed_lists() -> crate::Result<()> {
        let parsed = Attributes::parse(
            r#"BANDWIDTH=64000,CODECS="mp4a.40.2,mp4a.40.5",RESOLUTION=640x360,AUDIO="aac""#,
        )?;
        assert_eq!(parsed.integer("BANDWIDTH")?, Some(64_000));
        assert_eq!(parsed.quoted("CODECS")?, Some("mp4a.40.2,mp4a.40.5"));
        assert_eq!(parsed.plain("RESOLUTION")?, Some("640x360"));
        assert!(parsed.contains("AUDIO"));
        assert!(parsed.quoted("BANDWIDTH").is_err());
        assert!(parsed.plain("CODECS").is_err());
        assert_eq!(parsed.quoted("MISSING")?, None);
        for hostile in [
            "BANDWIDTH",
            "=1",
            "bandwidth=1",
            "A=1,A=2",
            "A=1,",
            "A=\"open",
            "A=\"x\"y",
            "A=",
            "A=1,,B=2",
            "A=x\"y",
        ] {
            assert!(Attributes::parse(hostile).is_err(), "{hostile}");
        }
        let many = (0..65)
            .map(|index| format!("A{index}=1"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(Attributes::parse(&many).is_err());
        Ok(())
    }

    #[test]
    fn numbers_are_bounded_decimal_text() -> crate::Result<()> {
        assert_eq!(decimal_integer("0")?, 0);
        assert_eq!(decimal_integer("18446744073709551615")?, u64::MAX);
        for hostile in ["", "-1", "+1", "1.0", "0x10", "18446744073709551616", " 1"] {
            assert!(decimal_integer(hostile).is_err(), "{hostile}");
        }
        assert_eq!(duration_us("10")?, 10_000_000);
        assert_eq!(duration_us("9.009")?, 9_009_000);
        assert_eq!(duration_us("1.0000009")?, 1_000_000);
        for hostile in ["", "-1", ".5", "1.", "1e3", "100000", "1.2.3", "NaN"] {
            assert!(duration_us(hostile).is_err(), "{hostile}");
        }
        Ok(())
    }
}
