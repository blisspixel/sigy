# Multilingual signals, speech, and music

Reviewed: 2026-09-20. Status: research and proposed evaluation design; no models evaluated.

Implementation follow-up: the [2026-09-22 evaluation review](30-language-pipeline-evaluation.md) updates native candidates, reference corpora, and automated quality checks. The user confirmed this note's initial eight-language survey and requested evaluation without newly arranged human reviewers. The [active plan](../docs/development/language-pipeline.md) controls execution and evidence claims.

## Product baseline

Most expected listening is non-English. Language identification within blocks and translation primarily into English are confirmed requirements. A station, channel, or URL can carry several languages during one session. Its directory language is a discovery hint, not a permanent assignment to its content.

The same baseline applies to music: the intended catalog and listening sample are predominantly non-English. Region, station location, artist origin, title script, and sung language describe different things.

## Evidence and implications

BCP 47 supplies language identifiers with optional script and region subtags. It identifies a language; it does not measure recognition certainty or turn a multilingual block into one language. Sigy should store identified spans and uncertainty separately from the tag. [RFC 5646](https://www.rfc-editor.org/rfc/rfc5646).

FLEURS provides aligned speech material in 102 languages and supports speech recognition and language-identification evaluation. It is useful as a broad baseline. Read speech alone cannot qualify a system for noisy radio, code switching, overlapping presenters, regional names, or songs. Supplement it with appropriately usable, labeled broadcast and synthetic fault fixtures. Dataset acquisition remains future work. [Official FLEURS dataset card](https://huggingface.co/datasets/google/fleurs).

Unicode defines grapheme segmentation and character-width properties relevant to terminal handling. Actual shaping, bidirectional display, selection, and column layout still require terminal-specific testing. A Unicode-capable library alone does not establish usable Arabic or mixed-script captions. [Text segmentation](https://unicode.org/reports/tr29/), [East Asian width](https://unicode.org/reports/tr11/).

Native speech-runtime candidates and their unresolved streaming tradeoffs are in [Speech and translation](03-speech-and-translation.md). Model support claims must be evaluated by task: recognizing a language, identifying it, translating it, and handling singing are separate capabilities.

[Local processing and capacity](17-local-processing-and-capacity.md) adds broad-coverage Omnilingual ASR and independent language-detector research. Native export versions, detector coverage, transcription coverage, and license permissions require separate checks. A large supported-language list does not establish excellent radio transcription or code-switch detection for every listed language.

## Proposed block and span contract

An analysis block has a stable identity, source/channel configuration, acquisition identity, original sample or event range, content classification, language observations, processing state, and revision. Its boundaries serve bounded processing and replay; they need not equal storage-segment or sentence boundaries.

Within a block, retain zero or more language spans. Each contains offsets, candidate language tags, detection method, model/configuration, raw score if available, assessed certainty, and whether the decision was automatic or user-supplied. Overlapping speakers may need overlapping spans. If a model provides only a block label, record that coarse granularity rather than inventing precise switch timestamps.

Represent these cases explicitly:

| Content | Required representation |
| --- | --- |
| French speech followed by Arabic speech | Multiple timed spans and corresponding transcript/translation references |
| Simultaneous languages or an unresolved switch | Mixed/overlapping state with uncertainty and available candidates |
| Too little or too noisy speech | Undetermined; retain candidates without forcing English |
| Unsupported language | Detected if possible, processing unavailable, retained for later processing |
| Instrumental music, silence, or noise | No linguistic content detected; no fabricated transcript |
| Sung material | Music/vocal classification; language uncertain unless a qualified method supports it |
| Morse callsigns, abbreviations, or binary telemetry | Symbolic or structured content; natural-language classification only when applicable |

An unavailable detection result, a confident non-speech result, and a failed detector are different states. A provider returning a language label does not make that label correct.

## Processing alternatives

1. Joint multilingual ASR and language detection may avoid a separate first pass, but its label granularity and code-switch behavior must be measured.
2. Lightweight language identification followed by a selected recognizer can save resources, but an early wrong decision can damage both transcription and translation.
3. A bounded second pass can investigate ambiguous blocks. Its extra latency and cost must fit the selected profile and monitor policy.

The proposed design permits all three through explicit capabilities. Compare them on the same corpus before choosing defaults. Preserve uncertainty downstream. Temporal smoothing may reduce label flicker, but must not erase an actual switch, advertisement, or guest language. User overrides have a declared scope and do not overwrite the observed detection history.

Translate finalized original text to English by default. Keep originals, transliterations if requested, and translations as distinct revisions. Preserve names, numbers, dates, quotations, negation, and speaker attribution in evaluation. Translation failure must not hide a valid original transcript. Direct speech translation is another candidate, provided original evidence and alignment remain inspectable.

## Search and topic monitoring

Search should combine original-script text, translated text, and optional transliteration without treating them as independent evidence. An English topic can retrieve relevant non-English observations. Indexes must point back to the exact source-language revision used.

Evaluate multilingual embeddings against lexical and translation-assisted retrieval. An English translation index can improve access but also propagate translation errors. A monitor records language coverage at discovery, capture, recognition, translation, and analysis stages so a model's weak language support cannot silently narrow the station sample.

Keep original display text intact. Normalize separate search fields, test diacritics and language-specific token boundaries, and make approximate matching visible. Never ASCII-fold the only stored station name or track title.

## Music coverage

Audio fingerprinting does not require lyrics to be English, but useful identification still depends on catalog coverage and robust matching of the actual broadcast version. Station metadata, fingerprint matches, artist metadata, and lyric-language guesses carry separate provenance. See [Monitoring and music](09-monitoring-and-music.md).

Measure identified airtime and false matches by region, language where known, genre, catalog, and recording condition. Include local releases, independent artists, live performances, remixes, multilingual songs, DJ talk-over, and unknown tracks. Do not remove unidentified airtime from a ranking denominator or infer a song's language from its title.

## Qualification plan

- A majority of evaluation speech duration and a majority of music examples must be non-English. Set per-language and condition minimums before comparing candidates.
- Proposed initial survey: French, Spanish, Portuguese, Arabic, Swahili, Hindi, Mandarin, and English, expanded with representative African and other regional languages. This is a survey proposal, not a claim of qualified launch support.
- Report language-identification confusion and abstention, switch-boundary accuracy where available, ASR word/character errors, name/number errors, translation adequacy, and end-to-end topic recall separately.
- Assess translation against licensed published references, deterministic metrics, and calibrated independent model checks under the current authorized plan. Preserve any existing human-reference provenance without claiming new human review. Missing references and unreliable judges limit claims; fluent agreement alone is insufficient.
- Include short clips, long multilingual sessions, dialects, accents, low bitrates, overlap, noise, music beds, silence, and malformed metadata.
- Compare real-time factor, memory, power, cold-start time, and caption latency by language and host profile. Quantization needs its own quality result.
- Publish qualified, experimental, and unavailable capabilities by language/task/profile. English performance cannot compensate for a failed declared language.

## Near future

Track improved streaming multilingual models, code-switch handling, regional catalogs, and terminal text support. Preserve the corpus and scorecard across upgrades. Broader model availability does not automatically expand Sigy's support claim.
