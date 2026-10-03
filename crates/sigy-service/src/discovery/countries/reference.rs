//! Bounded parsing of immutable, licensed CLDR and Unicode assets.

use std::{collections::BTreeMap, sync::OnceLock};

use serde::Deserialize;

use super::{Country, LOCALES, MatchReason, canonical_key};
use crate::{Error, Result};

pub(super) const SOURCES: [(&str, &str); 8] = [
    (
        "en",
        include_str!("../../../../../assets/countries/cldr-48.2.0/en.json"),
    ),
    (
        "fr",
        include_str!("../../../../../assets/countries/cldr-48.2.0/fr.json"),
    ),
    (
        "es",
        include_str!("../../../../../assets/countries/cldr-48.2.0/es.json"),
    ),
    (
        "ar",
        include_str!("../../../../../assets/countries/cldr-48.2.0/ar.json"),
    ),
    (
        "hi",
        include_str!("../../../../../assets/countries/cldr-48.2.0/hi.json"),
    ),
    (
        "zh",
        include_str!("../../../../../assets/countries/cldr-48.2.0/zh.json"),
    ),
    (
        "pt",
        include_str!("../../../../../assets/countries/cldr-48.2.0/pt.json"),
    ),
    (
        "sw",
        include_str!("../../../../../assets/countries/cldr-48.2.0/sw.json"),
    ),
];
pub(super) const FOLDING: &str =
    include_str!("../../../../../assets/countries/cldr-48.2.0/CaseFolding.txt");
const EXCLUDED: [&str; 8] = ["EU", "EZ", "QO", "UN", "XA", "XB", "ZZ", "SU"];

#[derive(Debug, Deserialize)]
struct Document {
    main: BTreeMap<String, Locale>,
}
#[derive(Debug, Deserialize)]
struct Locale {
    #[serde(rename = "localeDisplayNames")]
    names: Names,
}
#[derive(Debug, Deserialize)]
struct Names {
    territories: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct Entry {
    labels: BTreeMap<String, String>,
    aliases: BTreeMap<String, Alias>,
}

#[derive(Debug)]
struct Alias {
    label: String,
    locale: String,
}

#[derive(Debug)]
pub(super) struct Reference {
    entries: BTreeMap<String, Entry>,
    folding: BTreeMap<char, String>,
}

pub(super) fn get() -> Result<&'static Reference> {
    static REFERENCE: OnceLock<Result<Reference>> = OnceLock::new();
    REFERENCE
        .get_or_init(load)
        .as_ref()
        .map_err(|_| Error::InvalidInput("bundled country reference"))
}

fn included(code: &str) -> bool {
    code.len() == 2
        && code.bytes().all(|byte| byte.is_ascii_uppercase())
        && !EXCLUDED.contains(&code)
}

fn load() -> Result<Reference> {
    let folding = load_folding()?;
    let mut reference = Reference {
        entries: BTreeMap::new(),
        folding,
    };
    for (locale, source) in SOURCES {
        if source.len() > 32 * 1024 {
            return Err(Error::InvalidInput("country source exceeds 32 KiB"));
        }
        let document: Document = serde_json::from_str(source)?;
        let names = document
            .main
            .get(locale)
            .ok_or(Error::InvalidInput("country locale"))?;
        for (identifier, label) in &names.names.territories {
            let code = identifier
                .split('-')
                .next()
                .ok_or(Error::InvalidInput("country code"))?;
            if !included(code) {
                continue;
            }
            super::validate_text(label, 256)?;
            let key = reference.key(label);
            if key.len() > super::KEY_BYTES {
                return Err(Error::InvalidInput("country alias key exceeds 512 bytes"));
            }
            let entry = reference.entries.entry(code.to_owned()).or_default();
            if identifier == code {
                entry.labels.insert(locale.to_owned(), label.clone());
            }
            entry.aliases.entry(key).or_insert_with(|| Alias {
                label: label.clone(),
                locale: locale.to_owned(),
            });
        }
    }
    if reference.entries.len() > 300
        || reference.entries.is_empty()
        || reference.entries.values().any(|entry| {
            entry.aliases.len() > 64
                || LOCALES
                    .iter()
                    .any(|locale| !entry.labels.contains_key(*locale))
        })
    {
        return Err(Error::InvalidInput(
            "country reference bounds or missing labels",
        ));
    }
    Ok(reference)
}

fn load_folding() -> Result<BTreeMap<char, String>> {
    if FOLDING.len() > 128 * 1024 {
        return Err(Error::InvalidInput(
            "Unicode folding source exceeds 128 KiB",
        ));
    }
    let mut folding = BTreeMap::new();
    for line in FOLDING
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
    {
        let mut fields = line.split(';').map(str::trim);
        let code = fields
            .next()
            .ok_or(Error::InvalidInput("Unicode folding code"))?;
        let status = fields
            .next()
            .ok_or(Error::InvalidInput("Unicode folding status"))?;
        let mapping = fields
            .next()
            .ok_or(Error::InvalidInput("Unicode folding mapping"))?;
        if !matches!(status, "C" | "F") {
            continue;
        }
        let character = scalar(code)?;
        let output = mapping
            .split_whitespace()
            .map(scalar)
            .collect::<Result<String>>()?;
        if output.len() > 12 || folding.insert(character, output).is_some() {
            return Err(Error::InvalidInput("Unicode folding duplicate or bounds"));
        }
    }
    if folding.len() > 2048 || folding.is_empty() {
        return Err(Error::InvalidInput("Unicode folding bounds"));
    }
    Ok(folding)
}

fn scalar(value: &str) -> Result<char> {
    u32::from_str_radix(value, 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or(Error::InvalidInput("Unicode folding scalar"))
}

impl Reference {
    pub(super) fn key(&self, value: &str) -> String {
        canonical_key(value, &self.folding)
    }

    pub(super) fn candidates(&self, query: &str, key: &str, locale: &str) -> Vec<Country> {
        if query.len() == 2 && query.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            let code = query.to_ascii_uppercase();
            return vec![self.country(&code, locale, MatchReason::ExplicitCode, None)];
        }
        self.entries
            .iter()
            .filter_map(|(code, entry)| {
                if key.is_empty() {
                    return Some(self.country(code, locale, MatchReason::ReferenceList, None));
                }
                entry
                    .aliases
                    .iter()
                    .find(|(alias, _)| alias.contains(key))
                    .map(|(_, alias)| {
                        self.country(code, locale, MatchReason::NameSubstring, Some(alias))
                    })
            })
            .collect()
    }

    fn country(
        &self,
        code: &str,
        locale: &str,
        reason: MatchReason,
        matched: Option<&Alias>,
    ) -> Country {
        let entry = self.entries.get(code);
        let english = entry
            .and_then(|entry| entry.labels.get("en"))
            .map_or("Unlisted provider code", String::as_str);
        let label = entry
            .and_then(|entry| entry.labels.get(locale))
            .map_or(english, String::as_str);
        Country {
            code: code.to_owned(),
            name: label.to_owned(),
            english_name: english.to_owned(),
            listed: entry.is_some(),
            match_reason: reason,
            matched_alias: matched.map(|alias| alias.label.clone()),
            matched_locale: matched.map(|alias| alias.locale.clone()),
        }
    }
}
