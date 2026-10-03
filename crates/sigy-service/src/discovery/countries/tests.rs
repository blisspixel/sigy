use super::*;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[test]
fn every_included_country_is_selectable_without_a_library() -> TestResult {
    let mut cursor = None;
    let mut codes = std::collections::BTreeSet::new();
    loop {
        let result = page("", "en", cursor.as_deref())?;
        assert!(result.entries.len() <= PAGE_SIZE);
        assert!(result.ambiguous);
        for country in &result.entries {
            assert!(codes.insert(country.code.clone()));
            assert_eq!(resolve(&country.code)?, country.code);
            for locale in LOCALES {
                let label = page(&country.code, locale, None)?;
                assert!(!label.locale_fallback);
                assert!(!label.entries[0].name.is_empty());
            }
        }
        if result.next_after.is_none() {
            assert_eq!(codes.len(), result.total_candidates);
            break;
        }
        cursor = result.next_after;
    }
    assert_eq!(codes.len(), 257);
    for code in ["AC", "CP", "CQ", "DG", "EA", "IC", "TA", "XK"] {
        assert!(codes.contains(code), "exceptional territory {code}");
    }
    for code in ["EU", "EZ", "UN", "XA", "XB", "ZZ", "SU"] {
        assert!(!codes.contains(code));
        assert_eq!(resolve(code)?, code);
        assert!(!page(code, "en", None)?.entries[0].listed);
    }
    Ok(())
}

#[test]
fn aliases_preserve_scripts_accents_and_canonical_equivalence() -> TestResult {
    for (name, code) in [
        ("CANADA", "CA"),
        ("日本", "JP"),
        ("भारत", "IN"),
        ("المغرب", "MA"),
        ("Réunion", "RE"),
        ("Re\u{301}union", "RE"),
    ] {
        assert_eq!(resolve(name)?, code);
    }
    assert_eq!(resolve("ca")?, "CA");
    let china = page("中国", "zh", None)?;
    assert!(china.ambiguous);
    assert!(china.entries.iter().any(|entry| entry.code == "CN"));
    assert!(resolve("中国").is_err());
    assert!(china.entries.iter().all(|entry| {
        entry.match_reason == MatchReason::NameSubstring
            && entry
                .matched_alias
                .as_ref()
                .is_some_and(|alias| alias.contains("中国"))
            && entry.matched_locale.as_deref() == Some("zh")
    }));
    assert_eq!(
        page("CN", "en", None)?.entries[0].match_reason,
        MatchReason::ExplicitCode
    );
    assert_eq!(resolve("zz")?, "ZZ");
    // Swahili supplies an explicit unaccented alias; that does not remove accents
    // from the canonical keys of other labels.
    assert_eq!(resolve("Reunion")?, "RE");
    assert_ne!(
        reference::get()?.key("Réunion"),
        reference::get()?.key("Reunion")
    );
    assert_eq!(reference::get()?.key("Straße"), "strasse");
    assert_ne!(reference::get()?.key("A"), reference::get()?.key("А"));
    assert_ne!(reference::get()?.key("é"), reference::get()?.key("e"));
    assert_eq!(page("Canada", "fr-CA", None)?.display_locale, "en");
    assert!(page("Canada", "fr-CA", None)?.locale_fallback);
    Ok(())
}

#[test]
fn ambiguity_is_over_all_candidates_and_pages_do_not_resolve_it() -> TestResult {
    let congo = page("Congo", "en", None)?;
    assert_eq!(congo.total_candidates, 2);
    assert_eq!(
        congo
            .entries
            .iter()
            .map(|entry| entry.code.as_str())
            .collect::<Vec<_>>(),
        ["CD", "CG"]
    );
    assert!(resolve("Congo").is_err());
    let all = page("", "en", None)?;
    let next = page("", "en", all.next_after.as_deref())?;
    assert_eq!(all.total_candidates, next.total_candidates);
    assert!(all.ambiguous && next.ambiguous);
    assert!(all.entries.last().ok_or("last")?.code < next.entries.first().ok_or("first")?.code);
    assert!(resolve("Guinea").is_err());
    Ok(())
}

#[test]
fn cursors_bind_query_locale_reference_and_exact_candidate_membership() -> TestResult {
    let first = page("", "en", None)?;
    let cursor = first.next_after.ok_or("cursor")?;
    assert!(page("", "fr", Some(&cursor)).is_err());
    assert!(page("Guinea", "en", Some(&cursor)).is_err());
    assert!(page("", "en", Some(&cursor.replace(':', "!"))).is_err());
    assert!(page("", "en", Some(&format!("{}:ZZ", &cursor[..64]))).is_err());
    let canonical = page("A\u{30a}", "en", None)?;
    if let Some(cursor) = canonical.next_after {
        assert!(page("Å", "en", Some(&cursor)).is_ok());
    }
    Ok(())
}

#[test]
fn unsafe_and_oversized_inputs_fail_before_lookup() {
    for query in [
        "\u{1b}]52;c;payload\u{7}",
        "Canada\nCA",
        "\u{202e}Canada",
        "\u{0}",
    ] {
        assert!(page(query, "en", None).is_err());
        assert!(resolve(query).is_err());
    }
    assert!(page(&"a".repeat(129), "en", None).is_err());
    assert!(page("", &"e".repeat(33), None).is_err());
    assert!(page("", "en", Some(&"f".repeat(100_000))).is_err());
    assert!(resolve(r"\\server\share").is_err());
}

#[test]
fn retained_assets_and_manifest_match_the_reference_identity() -> TestResult {
    let manifest = include_str!("../../../../../assets/countries/cldr-48.2.0/provenance.json");
    assert_eq!(hex(&Sha256::digest(manifest.as_bytes())), SOURCE_HASH);
    let value: serde_json::Value = serde_json::from_str(manifest)?;
    let files = value["files"].as_array().ok_or("files")?;
    let source = |file: &str| -> Option<&str> {
        match file {
            "UCD-LICENSE" => Some(include_str!(
                "../../../../../assets/countries/cldr-48.2.0/UCD-LICENSE"
            )),
            "LICENSE" => Some(include_str!(
                "../../../../../assets/countries/cldr-48.2.0/LICENSE"
            )),
            "CaseFolding.txt" => Some(reference::FOLDING),
            _ => reference::SOURCES
                .iter()
                .find(|(locale, _)| format!("{locale}.json") == file)
                .map(|(_, source)| *source),
        }
    };
    for file in files {
        let name = file["file"].as_str().ok_or("name")?;
        let bytes = source(name).ok_or("source")?;
        assert_eq!(
            hex(&Sha256::digest(bytes.as_bytes())),
            file["sha256"].as_str().ok_or("hash")?
        );
    }
    assert_eq!(unicode_normalization::UNICODE_VERSION, (17, 0, 0));
    assert!(LICENSES.contains(include_str!(
        "../../../../../assets/countries/cldr-48.2.0/LICENSE"
    )));
    assert!(LICENSES.contains(include_str!(
        "../../../../../assets/countries/cldr-48.2.0/UCD-LICENSE"
    )));
    Ok(())
}
