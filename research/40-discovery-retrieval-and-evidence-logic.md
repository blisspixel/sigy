# Discovery, retrieval and evidence logic

Reviewed: 2026-10-03. Status: primary-source research and current-source inspection. The rules below are proposals for bounded increments, not implemented global coverage, language qualification, a representative sampling frame or a semantic truth engine. No runtime changes, builds, recordings or data-asset downloads were performed for this review.

The [implementation plan](../docs/development/reliability-and-scale.md) keeps country discovery before evaluated city discovery and preserves the existing [evidence and memory boundaries](../docs/design/storage-and-memory.md). The useful outcome is a country or passage that a user can inspect and act on explicitly, with the search scope and remaining uncertainty visible.

## Current owners and behavior

This table records source inspection before decision0080. Its country entry describes the earlier code editor. The now implemented reference/resolver follows [decision0080](../docs/decisions/0080-offline-country-reference.md); station-ID paging and literal archive matching remain as described. The proposals below preserve reviewed alternatives rather than overriding that selected contract.

| Owner inspected | Current behavior and limit |
| --- | --- |
| [Directory contracts](../crates/sigy-service/src/discovery/mod.rs) | Text filters have a 128-byte bound; country is blank or two uppercase ASCII letters. This checks syntax, not membership in a country reference. The cache ceiling is 10,000 stations; one refresh accepts at most 500 rows |
| [Directory storage](../crates/sigy-service/src/storage/discovery.rs) | Name matching uses lowercase substring comparison; country is exact; language and tag match lowercase list entries. Cached station pages contain 1 to 16 rows in station-ID order. Favorites are independent preferences |
| [Radio Browser adapter](../crates/sigy-service/src/discovery/radio_browser.rs) | Sends the country filter as `countrycode` and uppercases received country codes. Metadata is a provider observation, not verified location or acquisition authority |
| [Explorer filters](../crates/sigy/src/explorer/search.rs) | Draft, pending and applied scopes are separate. Country entry uppercases ASCII; backspace removes the last rendered grapheme. Text input is bounded and sanitized |
| [Explorer state](../crates/sigy/src/explorer/state.rs) and [transport](../crates/sigy/src/explorer/client.rs) | Responses are generation checked; accepted success changes applied rows and scope. A failed pending request preserves existing rows and scope. Selection admits no new collection |
| [Archive contract](../crates/sigy-service/src/archive.rs) and [scan](../crates/sigy-service/src/storage/archive.rs) | Read-only original-script and English search shares the monitor's literal comparison. Defaults are 16 hits, 20,000 examined rows and 1,000 ms; maxima are 64 hits, 200,000 rows and 2,000 ms. Page output is bounded to 57,344 bytes. Order is transcript ID, revision, pass and cue ordinal, not relevance or event time |
| [Monitor comparator](../crates/sigy-service/src/monitor.rs) | `term_matches` lowercases both strings and checks containment. It performs no normalization, stemming, translation or semantic interpretation |
| [Geometry core](../crates/sigy-core/src/geo.rs) | Spherical globe and flat-map geometry, validated finite coordinates, latitude bounds and normalized longitude. It models no ellipsoid, datum, elevation or location uncertainty; projection geometry is not a radius-search implementation |

Archive deadlines are checked between scanning steps; they are not proof of interrupting a blocked filesystem operation or a long SQLite step. Pages are continuation reads rather than an established immutable search snapshot. The time filter currently selects by recording capture start, not arbitrary event time or cue-overlap time. Language labels can be truncated independently from hits. Preserve these distinctions in future envelopes and acceptance briefs.

## Country reference and resolution rules

[CLDR display-name documentation](https://www.unicode.org/reports/tr35/tr35-general.html) describes localized names and alternate display forms. [Supplemental data](https://www.unicode.org/reports/tr35/tr35-info.html) includes territory groupings, code mappings and aliases; its regions include entities beyond sovereign countries. CLDR is locale data, not a station inventory or a political adjudication. The reviewed LDML pages identify version 48.2; an implementation must pin actual distributed data and its inclusion rule. Preserve the [Unicode license](https://www.unicode.org/license.txt) when distributing derived assets.

Proposed first country package:

These pre-implementation alternatives preceded decision0080. The implemented resolver preserves ambiguity across the complete normalized candidate set; the ranked alias shortcut below is not selected behavior.

1. Declare the selectable country/territory code set and upstream version. Preserve an explicit policy for provider-specific, deprecated, unknown and non-country codes. Do not accidentally include world or continent grouping codes as two-letter country choices.
2. Resolve a recognized two-letter code first. Names and aliases map to a set of codes, not directly to one presumed answer. Keep each alias's locale, provenance, validity and preferred or historical status.
3. Show current-locale names with a documented fallback chain and the stable code beside them. Search supported original-script aliases as well as display names. State the supplied locales and aliases; a complete selectable code set does not establish complete multilingual alias coverage.
4. Rank exact code, exact alias and prefix or substring candidates in documented tiers. An alias matching several identities returns choices. Historical codes with multiple successors return explicit alternatives; no first-row or locale guess silently resolves a split.
5. Selecting a resolved code reuses the existing country filter. Code entry remains a fallback. Resolution works with an empty station cache, offline, without refreshing or probing a stream.
6. Display zero as `0 cached matches` only when that count was actually computed over a declared cache scope. Unknown or uncomputed counts stay unknown. Reference completeness says which places can be selected, not which broadcasts exist.

Keep station-ID paging for this package. Name ordering requires a separate decision about collation, tie breakers, locale and index cost. Its future cursor must bind canonical query, ordering policy, reference/index generation and continuation key. Editing a name or changing locale invalidates an incompatible cursor. Names alone cannot be identities because they change and collide.

Acceptance uses the entire declared code set plus explicit ambiguous aliases, an uncached country, deprecated codes, unavailable locale fallback and missing names. Measure finite query bytes, candidate count, memory and response time. A table over every declared country is an exhaustive reference check, while a few station examples are a convenience sample.

## Ordered station search followup

Reviewed again: 2026-10-03, before decision0081. At that review the country resolver was implemented and station pages still used UUID order. [SQLite comparison rules](https://www.sqlite.org/datatype3.html#collation) specify ASCII-only folding for built-in `NOCASE`. Evaluate explicit UTF-8 comparison keys against the pinned normalization/folding profile instead of claiming multilingual collation from that setting. Keep original display names and station-ID tie breakers.

[SQLite transactions](https://www.sqlite.org/lang_transaction.html) support consistent reads while other connections commit changes. A new page operation should read its catalog revision, entries and lookahead together. A versioned revision-bound cursor can refuse later cache drift; it does not preserve that historical snapshot across requests.

[SQLite query planning](https://www.sqlite.org/queryplanner.html) describes indexed search and ordering. Inspect actual plans for the selected query and measure rejected rows and metadata predicates as well as returned rows. A small page alone cannot establish bounded work. The [EX-01C brief](../docs/development/near-term-implementation.md#ex-01c-stable-station-name-ordering-and-catalog-drift) proposes finite migration, query, cursor and race acceptance requirements. No ordering migration or new search operation is implemented by this review.

## Unicode matching and terminal editing

[Unicode normalization](https://www.unicode.org/reports/tr15/) separates canonical equivalence from compatibility equivalence. Compatibility transformations can erase meaningful mathematical distinctions. [Case guidance](https://unicode.org/faq/casemap_charprop.html) distinguishes casing from caseless comparison. [Text segmentation](https://www.unicode.org/reports/tr29/) defines grapheme boundaries, and [CLDR collation](https://www.unicode.org/reports/tr35/tr35-collation.html) describes language-sensitive ordering and search. No one transformation supplies all four behaviors.

Proposed rules:

- Preserve exact original text and immutable revisions. Derive versioned comparison keys separately. Never normalize source evidence in place.
- For country aliases, evaluate NFC plus full Unicode case folding as a declared comparison profile. Re-normalize the derived key where required by the chosen algorithm. Lowercasing alone fails to express all caseless equivalences. Locale-specific tailoring, including Turkish dotted and dotless I, needs named behavior and collision fixtures.
- Keep diacritics in the primary key. An optional accent-insensitive or transliterated fallback creates candidates with a visible match reason. It cannot silently resolve identity. Removing marks indiscriminately across scripts can change meaning.
- Keep scripts distinct. Visually similar Latin and Cyrillic letters are not equal country aliases merely because they look alike. Transliteration and compatibility matching are separately named expansions, especially for symbolic material.
- Treat bytes, Unicode scalars, graphemes and terminal columns as different limits. Editing and visible truncation use grapheme boundaries; protocol and storage limits remain bytes. Wide, combining and bidirectional text need actual terminal checks in addition to buffer tests.
- Bound transformed-key expansion and pasted input. A rejected or clipped query cannot be presented as fully accepted without a visible indication. Do not inject a terminal control sequence or rewrite an original script to make it fit.

Changing archive or monitor comparison is a behavioral migration. Keep today's literal mode explicit and introduce any normalized mode with a versioned contract; old stored matches and findings must retain the comparator that produced them. NFC equivalence is a string relation, not a conclusion that two speakers meant the same thing.

## Full-text semantics and archive queries

[SQLite FTS5](https://www.sqlite.org/fts5.html) provides token, phrase, prefix and Boolean queries. Its default Unicode61 tokenizer uses Unicode 6.1 case rules and removes Latin diacritics by default. Porter stemming is designed for English; trigram `MATCH` does not serve queries shorter than three Unicode characters. These modes do not preserve the existing literal substring behavior automatically.

Recommendation: keep literal, normalized literal and full-text modes distinct. Bind user strings safely and compile a typed query rather than exposing uncontrolled full-text grammar as a default. State whether a phrase means adjacent tokens, exact characters or proximity. Short terms, punctuation, apostrophes, hyphens, non-space-separated scripts and code-switching belong in the acceptance set. Original, target and both-field searches need explicit target identity rather than assuming English remains the only target.

A lexical or semantic index must identify each canonical cue, revision, target and profile. Index rebuilds are bounded jobs with watermarks, generation fencing and lag reporting. Filters should preserve their defined semantics across scan and index implementations. Scores order candidates within a named retrieval profile; they are not confidence in the source's claim.

[SQLite's progress callback](https://www.sqlite.org/c3ref/progress_handler.html) can interrupt database computation after configured virtual-machine work. Evaluate this separately from application deadlines; it does not establish a hard bound for blocked external I/O. Introduce no claim of service responsiveness without mixed request and storage-pressure measurements.

## Precision, recall and truncation logic

The original [information retrieval textbook's set evaluation](https://nlp.stanford.edu/IR-book/html/htmledition/evaluation-of-unranked-retrieval-sets-1.html) defines precision and recall against relevant items. Its [ranked evaluation](https://nlp.stanford.edu/IR-book/html/htmledition/evaluation-of-ranked-retrieval-results-1.html) discusses position-sensitive measures and incomplete judgments. These metrics depend on a specified corpus and relevance definition; they cannot prove source truth, total broadcast coverage or understanding outside the evaluation set.

For a frozen eligible corpus `C`, expected relevant set `G` and returned identities `R`, measure `precision = |R intersect G| / |R|` and `recall = |R intersect G| / |G|`. When either denominator is zero, report it explicitly rather than manufacturing a perfect score. `Recall@k` applies to a declared ranking and result cutoff. For an unfinished bounded scan, distinguish page precision from end-to-end recall after permitted continuation; measure how often stop bounds hide required evidence.

Freeze judgments before tuning, include relevant items near and beyond each boundary, and group independent original sources when estimating uncertainty. Report by script, language, target direction and query type as well as aggregate. A finite reference corpus establishes performance on that corpus. If annotations are incomplete, recall is not exhaustive; unlabeled results are not automatically false positives.

The downstream chain has separate gates: discovery, retained capture, decoding, transcription, translation, retrieval and interpretation. Retrieval over erroneous ASR can faithfully find the wrong stored word. Report exact stored-text retrieval separately from audio-grounded passage recovery and claim attribution. Preserve missing or failed processing as unavailable evidence, not irrelevant evidence. A negative search means no match under a declared representation and scope; it cannot establish that a broadcast, event or opinion was absent.

## City and radius search remain separate

[GeoNames' official extract documentation](https://download.geonames.org/export/dump/readme.txt) declares CC BY 4.0 licensing, stable IDs, WGS84 coordinates and alternate names. `cities15000` includes populations above 15,000 or capitals; it is not every city. The data carries no accuracy, completeness or timeliness warranty. Selection requires a pinned extract, license notices, inclusion manifest, alias joins and bounded import/search measurements. No extract was downloaded for this review.

Resolve a city to its ID, country/territory, administrative context and original-script name before station search. Duplicate names require explicit choices. Missing city data, missing station coordinates and zero nearby cached candidates are different states. Directory coordinates may represent studios, headquarters or supplied guesses; proximity is a metadata relation, not reception range, transmitter identity or broadcast origin.

[Karney's original geodesic work](https://arxiv.org/abs/1109.4448v2), revised 2012-03-28 and published in 2013, supplies robust ellipsoidal distance methods. Evaluate a maintained Rust implementation if ellipsoidal radius search becomes necessary. Current spherical rendering geometry does not qualify such a dependency or distance accuracy.

Proposed bounded geometry cases include finite coordinates, latitude endpoints, wrapped longitude, antimeridian crossing, identical points, near-antipodal points, zero and maximum radius, exact boundary inclusion and absent uncertainty. A bounding box is a candidate prefilter, followed by the declared distance function. Split antimeridian ranges and avoid longitude division near the poles. Bound candidate scans and show partial results if the bound stops filtering. An approximate distance needs a documented approximation limit; precision in a floating-point result is not coordinate accuracy.

Where justified location uncertainty bounds exist, classify a radius relationship as certainly inside, certainly outside or boundary-uncertain using the distance interval. Unknown uncertainty is not zero uncertainty. If the center or station provenance supplies no reliable bound, report nominal distance and unknown location accuracy instead of inventing a probability. Radius equality uses a stated inclusive rule and numerical tolerance appropriate to the implementation, never an unexplained epsilon.

## Information states and frozen context

Use explicit information states rather than collapsing them into success or empty. A resolved country, an observed directory row, a retained passage, an unqualified transcript and a supported claim are different artifacts. Pending, failed, stale, truncated, unsupported, expired, missing and unknown each explain what exists and the next permitted action. An operation's receipt records actual effects; a suggestion or selected view grants none.

A task context should freeze exact query and comparator, corpus or projection generation, original and target revisions, time cutoff, selected identities, omissions, coverage and resource bounds. Current continuation pages do not establish that frozen context by themselves. Admitting downstream publication requires a service-owned exact-membership snapshot, not the model's remembered search results. Imported instructions and retrieved text remain data throughout.

Relation identity answers which records were linked, under which algorithm and generation. Semantic truth answers whether the interpreted claim is supported. Hash equality, deterministic matching, a graph edge, a similarity score or a verification string cannot collapse those questions. Contradictions and independence remain attributed hypotheses unless supported by separate evidence. Repeated summaries and translations share their source lineage and cannot inflate corroboration.

## Recommended near-term increments at review

This sequence records alternatives before decision0081. [Selected station-name paging](../docs/decisions/0081-ordered-station-search.md) now implements the ordering/cursor slice; [active work](../docs/development/progress.md) holds its evidence. The remaining retrieval and city proposals keep separate gates.

1. Country reference and shared resolver: complete declared selectable identities, bounded names/aliases, explicit ambiguity and useful empty-cache behavior. Reuse current filters and station-ID paging.
2. Ordering and cursor migration: choose collation and tie breakers, bind query/order/generation, measure query cost and preserve selection across stale responses.
3. Archive envelope and comparator contract: preserve literal behavior, expose bounds and time semantics, freeze independently checked context identities before publication.
4. Lexical projection experiment: measure exact and full-text retrieval separately, including scripts, short queries, corrections, lag, truncation and recovery. Adopt only with demonstrated value.
5. City and radius package: evaluate licensed data coverage and geometry independently, then qualify nominal proximity and uncertainty handling without a global station-coverage claim.

Each package needs one user outcome, frozen inputs, permitted operations, finite limits, expected CLI/TUI artifacts and explicit difficult-state evidence. Capacity experiments and semantic interpretation remain separate gates, so a useful country picker does not wait for an entire graph, gazetteer or translation system.
