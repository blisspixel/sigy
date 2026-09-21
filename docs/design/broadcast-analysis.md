# Changing broadcast content

Updated: 2026-09-21. Status: planned analysis contract. No content, advertisement, language-change or song-boundary detector is implemented yet.

## One source, several aligned timelines

A station can change languages, programmes, advertisements and songs repeatedly. Directory languages are discovery hints, never the language assignment for a recording. Capture retains the original stream while independent analysis tracks describe what happened within it.

| Track | Required observations |
| --- | --- |
| Capture | Actual media/sample intervals, clock mapping, reconnects and missing data |
| Acoustic content | Speech, music, silence, noise and overlaps with model identity and uncertainty |
| Language | Per-speech-span languages, mixed/code-switched speech, unknown/unsupported states and revisions |
| Programme context | Candidate advertisement, station ident, jingle, host link or programme segment, with supporting evidence |
| Music events | Candidate song starts/ends, crossfades, partial tracks and speech overlays, independently of title identification |
| Track identity | Candidate recording/version, catalog or fingerprint evidence, unresolved alternatives and corrections |

These tracks can overlap. A presenter can speak over a song in a different language, and an advertisement can contain both music and speech. Preserve the original sample timeline and evidence relationships through resampling and decoding. Do not treat network chunks, file boundaries, station metadata changes or reconnects as programme boundaries.

## Live and batch behavior

Run bounded local observation windows with overlap where the selected detector requires it. Produce provisional events with stable IDs and allow later context to revise their boundaries or labels. Record unavailable or delayed processing explicitly. Detector settings, lookahead, latency and quality require measured profiles; no threshold or model has been selected by this contract.

Language detection should reconsider later speech and react to supported language shifts. Short utterances, code switching, names, low-resource languages and sung lyrics may remain uncertain. Translation uses each supported speech span's language evidence rather than forcing the whole station through one language model. Do not transcribe silence or instrumental music into invented speech.

Advertisement detection is a semantic classification. Product names, repeated clips, changes in loudness or language, and short duration are insufficient by themselves. Preserve evidence, confidence and corrections. Initially annotate candidate ads; do not automatically discard audio, skip transcription or suppress topic evidence based on that label.

Music boundaries and music identity are separate. Detecting a new song does not require knowing its title. Handle crossfades, DJ talkovers, remixes, live sets, medleys, repeated hooks and jingles. Provider/ICY metadata can lag, repeat or disagree with audio; retain it as a timed claim. Debouncing and deduplication prevent repeated metadata or revised boundaries from manufacturing extra plays. Music identification and sampled airplay rankings remain after the first release.

Processing receipts identify the completed stages and input revisions. Only the storage policy decides whether temporary media becomes eligible for deletion; pending analysis cannot silently expand retention. When originals expire, retained annotations and findings disclose that replay evidence is unavailable. Reprocessing creates revisions through existing resource and spending controls.

## Experience and qualification

Both clients expose the same inspectable events. The TUI shows aligned language spans, candidate ads, music intervals and provisional boundaries on the DVR timeline, with filtering and clear legends. Users can replay supporting context and correct a label without rewriting the original or losing prior results. A language shift should update the transcript/translation view without disrupting capture or stealing keyboard focus.

Evaluate multilingual broadcasts and mixtures, including French/English changes, within-sentence switching, unfamiliar languages, instrumental music, sung speech, quiet passages, jingles versus advertisements, product mentions in news, overlapping songs and DJ speech. Include codec changes, partial reception, repeated metadata, clock discontinuities and expired media. Measure detection delays, boundary error, false advertisements, missed changes, unknown coverage and correction behavior separately. One aggregate accuracy number cannot qualify these tasks.

See [language handling](languages.md), [interpretation lineage](signal-interpretation.md), [analysis and queues](../planning/10-analysis-and-knowledge.md) and [receiver metadata](recording-metadata.md).
