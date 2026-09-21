# Language and localization contract

Updated: 2026-09-20. Status: implementation target. English is the current maintenance CLI language. Canadian French, Navajo, and Klingon are minimum named content-understanding and translation acceptance cases, alongside interface localization. They are not stretch goals or an upper bound. The [universal interpretation contract](signal-interpretation.md) governs the broader ambition; [research](../../research/23-localization-and-language-coverage.md) records current evidence and gaps.

## Separate language axes

Store interface locale, source-declared language, user-supplied hint, observed content-language spans, translation target, and provider/model capability separately. The interface can be Canadian French while a source contains several languages and its translation target is English. A station's location or locale preference cannot become a detected language.

Use standards-based language identifiers, preserving script and region when actually known. Map provider aliases at the adapter boundary and retain the original provider value in provenance. Unknown content remains unknown. Mixed content has spans and evidence, not a single forced station language. A manual correction records a new revision with its basis and origin, preserving the original observation.

## Interface resources

Use one message catalog with stable message IDs, typed arguments, full translated messages, and explicit locale fallback. Human labels, help, warnings, and errors may be localized. Commands, flags, JSON keys, IDs, stored enums, exact monetary values, and protocol error codes remain stable for automation. Localized display formatting never changes parsing of machine-readable USD strings.

Resolve the requested locale from explicit configuration before OS preferences. Show a fallback when a requested pack is absent or incomplete; never silently claim the interface is fully translated. A missing string can fall back to reviewed English without resetting the user's language choice. Language packs are data, cannot execute code or modify service policy, and require resource/schema checks.

Use `fr-CA` for the Canadian French interface target, `nv` for Navajo, and a reviewed `tlh`/`tlh-Latn` pack for Klingon. Evaluate grammatical resources and required formatting independently per locale. Keep English equivalents available for critical technical terms. Playful translations must retain clear meanings for recording, deletion, unsupported processing, and spending controls.

Preserve original Unicode text and search/display transformations separately. Navajo tests include `ł`, nasal vowels, and stacked combining accents. Klingon tests preserve case and apostrophes, including distinct `q` and `Q`; exact search must not collapse them. Canadian French tests include accented text, longer labels, region-appropriate terminology, and explicit time-zone/currency context. Translation review must involve competent speakers before declaring a pack complete.

## Capability status and fallback

Every capability is separately reported as unavailable, declared by its provider, experimental, or validated for a named profile. Only measured evidence earns validated status. The matrix covers UI localization, text/speech language identification, transcription, translation direction, and optional speech synthesis. Installed-model presence alone is not qualification.

An unsupported local route preserves the source and offers a clear explanation plus suitable installed/configurable alternatives. It must not trigger a paid request, mislabel output as another language, or claim arbitrary language support because an LLM accepts free-form prompts. Optional remote routing retains the normal user policy, finite reservation, and cost accounting.

Quality tests include regional varieties, code switching, music, silence/noise, and low-resource languages. Detection can abstain. User hints and corrections remain available. Evaluate text quality, latency, and capacity separately; good batch accuracy does not establish real-time performance.

## Acceptance and continuity

Before the first substantial TUI screen, establish the message boundary, locale fallback, grapheme/cell handling, and pseudo-localized layout tests. Implement Canadian French as an early complete-pack target. Navajo and Klingon need reviewed packs and substantive content processing, with gaps tracked as required work. Do not fill packs with unreviewed generated translations merely to report completion.

Keep a versioned coverage table with evidence links when actual packs/models are integrated. Tests cover message keys and arguments, fallback, plural cases, terminal-safe interpolation, stable JSON/CLI behavior under each locale, and restart persistence of preferences. Screen-reader and right-to-left support remain terminal-profile claims requiring separate tests.
