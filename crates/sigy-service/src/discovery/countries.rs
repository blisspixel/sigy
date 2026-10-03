//! Offline country and territory discovery. Reference labels never authorize a fetch.

mod reference;
#[cfg(test)]
mod tests;

use serde::Serialize;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::{Error, Result, sources::unsafe_display};

/// Pinned label, inclusion and Unicode matching contract. A change invalidates cursors.
pub const VERSION: &str = "cldr-48.2.0-unicode-17-policy-1";
/// SHA-256 of the retained source manifest, including every input asset hash.
pub const SOURCE_HASH: &str = "82655912aae4696b5c5432afc585a47f1a99807c3e629bc90566f4d29f8edd68";
/// Display locales. Search considers aliases from every bundled locale.
pub const LOCALES: [&str; 8] = ["en", "fr", "es", "ar", "hi", "zh", "pt", "sw"];
/// Country pages do not alter the existing station page limit or order.
pub const PAGE_SIZE: usize = 16;
/// Unchanged legal notices travel in the executable with the embedded reference data.
pub const LICENSES: &str = concat!(
    include_str!("../../../../assets/countries/cldr-48.2.0/LICENSE"),
    "\n",
    include_str!("../../../../assets/countries/cldr-48.2.0/UCD-LICENSE")
);
const QUERY_BYTES: usize = 128;
const KEY_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchReason {
    ReferenceList,
    ExplicitCode,
    NameSubstring,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Country {
    pub code: String,
    pub name: String,
    pub english_name: String,
    /// Explicit raw provider codes remain usable even when absent from this reference.
    pub listed: bool,
    pub match_reason: MatchReason,
    pub matched_alias: Option<String>,
    pub matched_locale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Page {
    pub reference_version: &'static str,
    pub source_hash: &'static str,
    pub requested_locale: String,
    pub display_locale: String,
    pub locale_fallback: bool,
    pub query_key: String,
    pub total_candidates: usize,
    pub ambiguous: bool,
    pub entries: Vec<Country>,
    pub next_after: Option<String>,
}

/// Inspect a bounded, stable-code page independently of the station cache or library.
///
/// # Errors
/// Rejects oversized or unsafe queries, unknown cursors and corrupted bundled data.
pub fn page(query: &str, locale: &str, after: Option<&str>) -> Result<Page> {
    validate_text(query, QUERY_BYTES)?;
    validate_text(locale, 32)?;
    let reference = reference::get()?;
    let key = reference.key(query.trim());
    if key.len() > KEY_BYTES {
        return Err(Error::InvalidInput("country query key exceeds 512 bytes"));
    }
    let locale_key = locale.to_ascii_lowercase();
    let display_locale = LOCALES
        .iter()
        .copied()
        .find(|candidate| *candidate == locale_key)
        .unwrap_or("en");
    let candidates = reference.candidates(query.trim(), &key, display_locale);
    let scope = scope_hash(&key, &locale_key, display_locale);
    let start = cursor_start(after, &scope, &candidates)?;
    let entries = candidates
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .cloned()
        .collect::<Vec<_>>();
    let next_after = if start + entries.len() < candidates.len() {
        entries
            .last()
            .map(|entry| format!("{scope}:{}", entry.code))
    } else {
        None
    };
    Ok(Page {
        reference_version: VERSION,
        source_hash: SOURCE_HASH,
        requested_locale: locale.to_owned(),
        display_locale: display_locale.to_owned(),
        locale_fallback: locale_key != display_locale,
        query_key: key,
        total_candidates: candidates.len(),
        ambiguous: candidates.len() > 1,
        entries,
        next_after,
    })
}

/// Resolve only a single candidate across the complete reference, never just one page.
/// A literal ASCII two-letter code takes precedence and preserves provider semantics.
///
/// # Errors
/// Refuses unsafe, unknown or ambiguous names. Assistance never silently changes a code.
pub fn resolve(query: &str) -> Result<String> {
    if query.is_empty() {
        return Ok(String::new());
    }
    let result = page(query, "en", None)?;
    if result.total_candidates != 1 {
        return Err(Error::InvalidInput(
            "country name is unknown or ambiguous; use radio countries QUERY and select a code",
        ));
    }
    result
        .entries
        .first()
        .map(|entry| entry.code.clone())
        .ok_or(Error::InvalidInput("country reference"))
}

fn validate_text(value: &str, maximum: usize) -> Result<()> {
    if value.len() > maximum || value.chars().any(unsafe_display) {
        return Err(Error::InvalidInput(
            "country query or locale has unsafe text or exceeds its byte bound",
        ));
    }
    Ok(())
}

fn scope_hash(key: &str, locale: &str, display_locale: &str) -> String {
    let mut hash = Sha256::new();
    for field in [VERSION, SOURCE_HASH, key, locale, display_locale] {
        hash.update(field.as_bytes());
        hash.update([0]);
    }
    hex(&hash.finalize())
}

fn hex(bytes: &[u8]) -> String {
    crate::storage::dvr::hex(bytes)
}

fn cursor_start(after: Option<&str>, scope: &str, candidates: &[Country]) -> Result<usize> {
    let Some(cursor) = after else {
        return Ok(0);
    };
    if cursor.len() != 67 {
        return Err(Error::InvalidInput("country cursor"));
    }
    let Some((prefix, code)) = cursor.split_once(':') else {
        return Err(Error::InvalidInput("country cursor"));
    };
    if prefix != scope {
        return Err(Error::InvalidInput(
            "country cursor belongs to another query, locale or reference",
        ));
    }
    candidates
        .iter()
        .position(|entry| entry.code == code)
        .map(|index| index + 1)
        .ok_or(Error::InvalidInput("country cursor code"))
}

fn canonical_key(value: &str, folding: &std::collections::BTreeMap<char, String>) -> String {
    let mut key = String::new();
    for character in value.nfc() {
        if let Some(mapping) = folding.get(&character) {
            key.push_str(mapping);
        } else {
            key.push(character);
        }
    }
    key.nfc().collect()
}
