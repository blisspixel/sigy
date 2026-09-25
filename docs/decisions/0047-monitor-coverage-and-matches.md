# Monitor coverage and passage matches

Date: 2026-09-24. Status: implemented and tested on Windows x86_64. This covers the counting part of roadmap operation 31 and the matching part of operation 32 in the [topic monitoring design](../design/topic-monitoring.md). Monitors still do not schedule, store findings or write briefings.

## Decision

Two read-only views answer "what did this monitor actually cover" and "where did its terms appear". Neither writes a row, starts work, contacts a network or spends money.

- **Window.** Both take a half-open window on capture start times, at most 31 days. The CLI defaults to the last 24 hours and accepts exact Unix milliseconds. Scope is the sources the monitor follows now (its latest version plus applied source actions) and the saved schedules in its latest version.
- **Coverage** counts each stage separately per source: captures started, captures with a published file, recorded audio, recorded gaps, published pins, pins whose latest recognition revision has text or no text (with the audio each covers), and cues in the latest translation by state, with untranslated reasons, plus cues whose transcript has no translation. Schedules report admitted, missed (window elapsed), missed (spring forward) and waiting occurrences. No stage implies another, nothing is combined into a percentage, and the daily audio cap is shown next to the counts with a statement that it is not enforced yet. At most 1,024 captures are read per source; a truncated source says so.
- **Matches** compare each term with the latest recognition revision of each capture's latest published pin, and with that revision's latest English translation. Matching is literal containment after Unicode lowercasing: no normalization, stemming, transliteration or accent folding. Terms in any language are compared with the original script; `en` and `und` terms are also compared with the English text. Each match cites the source, recording, capture start, transcript ID and revision, cue ordinal and media clock, the original text, and the translation revision and English text when present. At most 512 transcripts are read and 64 matches returned; a partial result says so.

Recognized text is uncertain and English text is machine output; the CLI states both. A match is a location to check, not a finding.

## Evidence

A storage fixture covers one published recording with a gap, one recognized Spanish cue, a missed and an admitted schedule occurrence, and a monitor with Spanish, English, `und` and Arabic terms: stage counts before and after translation, an untranslated cue counted with its reason, the latest of two translations used, an English term matched only after English text exists, an uppercase term matched case-insensitively, a term in another script not matched, the half-open window, window and monitor refusals, no rows written, and a later source reported with its own zero counts.

## Limitations

Coverage uses the sources followed now, not the sources followed when each capture ran. No normalization means spelling and diacritic variants are missed, and recognizer errors hide terms. Matches are not stored, so there are no finding IDs or evidence links yet; those come with cited findings in operation 32. No scheduling, repetition grouping or briefing exists. Neither view is exposed through `sigy mcp` yet.
