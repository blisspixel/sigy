//! Shared pinned NFC and full case folding, preserving scripts and diacritics.

use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

pub(super) fn canonical_key(value: &str, folding: &BTreeMap<char, String>) -> String {
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
