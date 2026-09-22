# Localization and language coverage

Reviewed: 2026-09-20. Status: research and explicit user scope; no localized Sigy interface or speech capability has been validated. Canadian French, Navajo, and Klingon are requested targets. [Language contract](../docs/design/languages.md) defines the implementation boundary; [multilingual processing](10-multilingual-processing.md) covers content evidence.

2026-09-22 follow-up: the user requested content-processing validation without depending on newly arranged human reviewers. The [current plan](../docs/development/language-pipeline.md) uses licensed published references, deterministic metrics, and calibrated independent model checks, with explicit evidence limits. This updates the review workflow below without dropping the named languages or claiming a human-reviewed interface pack.

## Language identity and distinct capabilities

The IANA registry inspected on this date includes French `fr`, Navajo `nv` (also described as Navaho), Klingon `tlh`, and the Klingon script `Piqd`. Use BCP 47 identifiers such as `fr-CA`, `nv`, and `tlh-Latn` where that precision is known. Some model vocabularies instead use ISO 639-3 or private provider codes; adapters must map them explicitly. A language tag does not establish model support. [IANA registry](https://www.iana.org/assignments/language-subtag-registry/language-subtag-registry), [Unicode identifier guidance](https://cldr.unicode.org/index/cldr-spec/picking-the-right-language-code).

| Target | Evidence and limitation | Sigy implication |
| --- | --- | --- |
| Canadian French | Translator documents `fr-ca` separately from `fr`; automatic text-language detection is not marked for the regional variant | UI locale can be `fr-CA`; station country or a generic French detection cannot establish regional speech variety |
| Navajo | IANA supplies `nv`; CLDR contains Navajo locale data and orthographic exemplars | Preserve diacritics and combining sequences; locale data is neither a completed UI translation nor an ASR model |
| Klingon | IANA supplies `tlh`; the Klingon Language Institute documents case distinctions including `q` versus `Q` | Preserve orthography; do not apply universal lowercasing, title-casing, or case-insensitive exact-match assumptions |

[Translator language matrix](https://learn.microsoft.com/en-us/azure/ai-services/translator/language-support), [CLDR Navajo summary, version 47](https://www.unicode.org/cldr/charts/47/summary/nv.html), [Klingon orthography](https://www.kli.org/about-klingon/sounds-of-klingon/). The CLDR summary is versioned older evidence for text fixtures; select current locale data when integrating a formatting library rather than treating that chart as the latest release.

## Actual processing coverage checked

The published Whisper large-v3 generation configuration inspected on this date contains `<|fr|>`, but no `<|fr-CA|>`, `<|nv|>`, or `<|tlh|>` language token. This establishes a declared French route, not measured Canadian French accuracy, and supplies no supported Navajo/Klingon route. The result does not claim every other local model lacks these languages. Separate model-card and licensed-corpus research remains necessary. [Published generation configuration](https://huggingface.co/openai/whisper-large-v3/blob/main/generation_config.json).

Microsoft's current Translator text matrix lists Canadian French and Klingon Latin/pIqaD cloud translation. It does not establish local zero-metered-cost inference, speech recognition, terminal font rendering, or translation quality for Sigy. No requests or purchases were made. This is an optional-provider research lead, not a selected adapter or permission to spend. [Language support](https://learn.microsoft.com/en-us/azure/ai-services/translator/language-support).

Do not route an unsupported language to English ASR and present the output as a confident transcript. Preserve recordings and unknown-language evidence, offer a truthful unsupported state, and allow later processing with a qualified model or supplied transcript. A user's explicit language hint remains separate from a model's detection.

Navajo evaluation needs lawful, appropriate material and credible reference evidence. Under the current automated workflow, preserve published reference provenance and unresolved quality gaps without requiring new speaker recruitment. Do not fabricate fluency or treat an Indigenous language as a novelty mode. Klingon can support a playful experience without making errors, costs, or permissions ambiguous; its required content-processing case remains open until measured. Avoid franchise imagery, copied dialogue, and licensed font assets unless their distribution rights are established.

## Localization technology candidates

| Component | Stable metadata reviewed | Intended role |
| --- | --- | --- |
| fluent-bundle | 0.16.0, 2025-05-22, Apache-2.0 OR MIT | Translator-owned messages, variables, and grammatical variants |
| unic-langid | 0.9.6, 2025-05-09, MIT OR Apache-2.0 | Locale identifiers; verify its accepted subset before using it for all content tags |
| ICU4X `icu` | 2.3.1, 2026-08-20, Unicode-3.0 | Locale formatting and selected Unicode services; evaluate individual modules/data size |

[Fluent guide](https://projectfluent.org/fluent/guide/), [Fluent metadata](https://crates.io/api/v1/crates/fluent-bundle), [identifier metadata](https://crates.io/api/v1/crates/unic-langid), [ICU4X](https://icu4x.unicode.org/), [ICU metadata](https://crates.io/api/v1/crates/icu). These are candidates, not added dependencies. Prefer one message system and only the formatting/data modules actually needed. Do not implement grammar by concatenating English fragments or write a partial locale standard parser just to save a dependency.

## Qualification work

Test translated text expansion, plural/number/time formatting, locale fallback, missing keys, malformed resources, variable escaping, and live locale changes without job mutation. Keep machine output invariant. Use pseudolocalization to expose layout assumptions, then competent review for actual language packs.

Build a per-capability matrix of UI completeness, text identification, speech identification, ASR, source/target translation, and optional synthesis. Each processing entry records exact model revision, language/script/variety, licensed test material, error examples, timing, hardware, and local/LAN/paid routing. UI completeness cannot upgrade a speech claim. A model upgrade creates new evidence rather than silently rewriting the support matrix.
