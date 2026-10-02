# Archive passage search

Date: 2026-10-02. Status: implemented and tested on Windows x86_64. This is the first bounded increment of roadmap operation 46: literal passage retrieval over stored original-script cues and their English translations across the whole library. Exact retained cue playback, revision comparison, dependency inspection, deliberately requested recomputation and report bundles remain separate increments. Catalog schema is unchanged. Local IPC gains one read request; the protocol version is assigned when this increment is integrated. No language is qualified and no stage exit is established.

## Decision

`sigy analysis search --term TERM` reads stored text and returns one bounded page of hits. The same request is `{"action":"search","query":{...}}` under the existing `analysis` operation on local IPC, and `analysis_search` through `sigy mcp`. It creates no job, translation, finding, briefing, ledger event, provider attempt, retention change or network request.

### Matching

Every cue is compared with the function [monitor matches](0047-monitor-coverage-and-matches.md) use: both strings are lowercased with Unicode rules and the term must be contained literally. There is no Unicode normalization, stemming, transliteration or accent folding, so a precomposed and a decomposed accent are different text. A term has 1 to 200 characters, no control characters and no leading or trailing whitespace, the same rule as a monitor term.

`--in` selects `original`, `english` or `both` (the default). `original` corresponds to a monitor term in a language other than `en` or `und`; `both` corresponds to an `en` or `und` term. With `both`, a cue whose original script contains the term is one `original` hit; otherwise its English can be one `english` hit, as in monitor matches.

### Revisions

By default the search reads the newest text revision of each transcript, a recognition or a [correction](0056-transcript-corrections.md) whose outcome is `text`, and that revision's newest translation. A revision is stale when a newer text revision of the same transcript exists, the rule [stored findings](0057-stored-findings.md) use. `--history` also reads every older text revision and every older translation revision. An older translation is read in its own pass and contributes only English hits whose original did not already hit. Each hit reports `stale_transcript` and `stale_translation` explicitly. Legacy placeholders and `no_text` revisions are read and skipped.

The scope is the library, not a monitor. Monitor matches read only the sources a monitor follows, captures that started in its window and the latest published pin of each recording. Where those scopes coincide, the fixture shows the same citations for the same term.

### Each hit

A hit cites the source revision, recording, capture start, transcript ID, revision and kind, cue ordinal, half-open media interval, where the term was found, the original script, the translation revision, and the English text or the untranslated reason. Wording stays uncertain and English is machine output.

The media state comes from the same catalog reading stored findings use, now one shared function: `retained` (the recording is retained with the transcript's checksum, one published interval covers the cue, no gap overlaps it and its segment is not released), `released` (the segment that held the cue was released; see [segment retention](0028-segment-retention.md)), `expired` (the recording is deleting or deleted), `missing` (the cue lies outside published audio or overlaps a gap) or `unavailable` (covered, but the recording is not verified retained, such as a checksum mismatch). It is metadata: no file is opened or hashed, nothing is protected, and nothing is played. Text whose audio expired stays searchable and says so.

Language labels come from stored [language evidence](0033-language-evidence.md) on the hit's analysis input: the newest revision of each evidence track, every span that overlaps the cue. Each label keeps its tag, evidence ID and revision, the transcript revision it is bound to, and the stored route capability, such as `unevaluated`. A correction copies cue times, so evidence bound to the older revision still overlaps, and the binding shows that. At most 8 labels are reported per hit and at most 1,024 label rows are read per transcript revision; `more_languages` marks either limit. `--language TAG` normalizes the tag and keeps cues with an exactly matching label, or, for a filter without subtags, a label with that primary language, so `fr` accepts `fr-CA`. A cue with no stored label never passes a language filter. Recognizer labels are unevaluated block labels, not measured identification.

### Bounds

Rows are visited in catalog key order: transcript ID, transcript revision, pass, cue ordinal. That order follows the primary-key index, so a request does not sort the catalog. It is not time order; every hit carries its capture start.

- Results: `--limit` 1 to 64, default 16.
- Rows: `--scan-rows` 2 to 200,000, default 20,000. Transcript revision rows and cue rows both count, and the budget is never exceeded.
- Deadline: `--deadline-ms` 10 to 2,000, default 1,000, checked before each row once the cursor has moved. At least one row is always read, so a continued request always advances.
- Page bytes: 57,344 serialized bytes including JSON escaping. The largest possible hit, two 4,096-byte texts of control characters at six escaped bytes each, fits an empty page. The bound leaves room for the snapshot envelope inside the 64 KiB agent tool output and the 256 KiB local response.
- Filters: `--source` and `--from-ms`/`--to-ms` on capture start (half-open) are applied to each transcript row after it is read, so a narrow filter can spend the budget without hits.

A page that ends early says why in `stopped`: `results` (another hit exists beyond the limit), `page_bytes` (another hit would not fit), `rows` or `deadline`. `next` is the first unread position, `ID/REVISION/PASS/ORDINAL`, and is passed back unchanged as `after` with the same term and options. Absent `stopped` means every row after the starting position was read. Pages are not a snapshot: a write between requests is visible to the next request, and a revision that became stale between requests is skipped by a default search.

The search runs in the catalog actor like other reads. The 2,000 ms ceiling bounds how long one search holds it, below the 5-second local request timeout.

### Output

Plain output prints each hit with the original script on its own line and the English below it, the revision and media states, the language labels with their binding, the stop reason and the continuation cursor. Every stored string passes through the existing terminal sanitizer, which drops control and bidirectional formatting characters for display only. `--json` prints the exact page; JSON escapes control characters and changes no text.

### Agent tool

`analysis_search` is a read-only, idempotent, closed-world tool. It fits the [agent plugin](0021-agent-plugin.md) authority model: it runs the same command with a fixed argument list against the startup library, accepts no path, budget or shell text, starts no work and contacts no network. The term is always passed as the value of `--term`, which accepts leading hyphens, so text such as `--data-dir=elsewhere` stays a term and cannot select another library. The existing transcript and translation tools already return this text to an agent; the search returns the same text with citations and bounds. It does not publish a finding. A hit is a place to check, and there is still no tool that stores a finding or a briefing.

## Evidence

Service fixtures use real recording, pin, recognition, correction, translation and language evidence rows; recognizer and translator results are synthetic storage fixtures.

- Multilingual scripts: Arabic, Devanagari with candrabindu, Chinese, and Spanish in precomposed and decomposed forms. Uppercase terms match lowercase text. Anusvara does not match candrabindu, and a precomposed accent does not match a decomposed one.
- Agreement: eight monitor terms in `ar`, `hi`, `zh`, `es`, `und` and `en` return the same transcript, revision, cue and field citations from monitor matches and from archive search over the same source and window.
- Corrections: a corrected-away word is found only with `--history` and is labeled stale; a copied cue is current on the new revision and stale on the old one; a second translation marks the first stale; translating the correction makes its English current. Stored translations are unchanged.
- Media: retained, released, expired while deleting and after deletion, missing for an overlapping gap, and unavailable for a checksum mismatch.
- Language: `es`, `fr-CA`, Navajo `nv` and Klingon `tlh` labels filter by exact tag and primary subtag; a correction keeps showing the older binding.
- Bounds: result limits page through every hit without loss or duplication; row budgets of 2, 3 and 5 rows never overrun and always advance; an already expired deadline still finishes the scan one row at a time; oversized hostile text stops at the page byte bound with each page under 57,344 bytes; the largest possible hit fits an empty page.
- Hostile text: escape sequences, bell characters and a right-to-left override round-trip exactly through the control operation and JSON, and the plain output drops them.
- Empty library and no writes: every relevant table count and the connection change counter are unchanged; the command-line test checks that catalog file sizes are unchanged and that invalid limits, budgets, deadlines, terms, cursors, language tags, half windows and fields write nothing to standard output.
- Agent tool: the schema is read-only, a term of `--data-dir=elsewhere` is searched as text and creates no other library, and an extra `data_dir` argument is refused.
- Populated catalog: 256 transcript revisions of 16 cues each are read in exactly 4,352 rows; a 1,000-row budget stops at exactly 1,000; a matching term stops at 64 hits after 70 rows.
- Rendered output: the command-line binary was run against a temporary fixture library with Arabic, Devanagari, Chinese and hostile cues, a correction, a deleted recording and language labels. The plain output was inspected for current and stale revisions, expired and retained audio, the language binding, sanitized control text and singular and plural counts. The receipt is a local diagnostic and is not committed.

Measured on 2026-10-02 on one AMD Ryzen 7 7840U host (8 cores, 16 threads) while parallel builds held the processor at 100% load, over a cloned catalog of 8,192 transcript revisions, 131,072 cues and 131,072 English cues (139,264 rows), five runs per case through the ignored `archive_measurement` test:

| Case | Optimized release test build | Unoptimized test build |
| --- | --- | --- |
| No hit, default 20,000-row budget | 150 to 325 ms, stopped at `rows` | 279 to 409 ms, stopped at `rows` |
| No hit, 200,000-row budget, newest revisions | whole catalog in 730 to 1,293 ms | `deadline` at 2,000 ms after 66,569 to 100,337 rows |
| No hit, 200,000-row budget, `--history` | whole catalog in 4 runs, 862 to 1,309 ms; 1 run `deadline` after 120,549 rows | `deadline` at 2,000 ms after 102,054 to 134,909 rows |
| 64 hits | 11 to 24 ms after 70 rows, stopped at `results` | 27 to 55 ms after 70 rows |

These timings are contention-affected observations on one host, not a capacity claim. The deadline stops show the bound working under load.

Integrated workspace verification outcomes belong in [active work](../development/progress.md).

## Limitations

- Literal containment only. Spelling, script and normalization variants, transliterations and recognizer errors hide passages. There is no index, ranking or full-text search.
- Key order, not time order. Source and window filters are applied after the transcript row is read.
- The library scope ignores which pin of a recording is the latest; every stored analysis input is searched.
- When the newest revision has no translation, its hits show no English. The older translation appears only with `--history`.
- Media states are catalog metadata. Exact retained cue playback, hash verification and surrounding context are later increments.
- Language labels are stored evidence and are not evaluated. Labels beyond the per-revision row limit are not read, and a filter can miss them.
- One search can hold the catalog actor for up to 2 seconds. Throughput beyond the measured fixture and on other hosts is unmeasured. Linux was not run.
- Pages are not snapshots of the catalog.
