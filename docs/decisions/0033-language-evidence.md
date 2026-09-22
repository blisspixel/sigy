# 0033: Revision-bound language evidence

Date: 2026-09-22. Status: implemented storage and read-only inspection. Detection quality, recognition, and translation remain unmeasured.

## Decision

Store immutable language-evidence revisions against one exact published analysis-input revision. Acoustic evidence can exist without a transcript. Text-derived evidence additionally names an exact transcript revision and a nonempty source cue. Legacy empty cues cannot establish words, language, or non-speech.

Keep observation, processing outcome, and route capability separate. Observations distinguish identified language, mixed speech, unknown language, and positively observed non-speech. Mixed observations list known members, which may be zero, one, or several. They do not fabricate switch boundaries or imply all members are known. Overlapping spans are retained. Not attempted, failure, cancellation, interruption, and unavailable input require a reason and carry no observations. Successful detection requires observed coverage.

Each half-open span lies inside one immutable media interval and intersects no gap. A coarse processing block may be smaller than a retained segment; its resolution remains `block`. Text block evidence covers its entire source cue. Word resolution requires recognizer origin and a nonempty timed cue. These constraints check provenance and bounds, not whether an algorithm really measured that resolution. Worker admission and evaluation must establish that before production publication.

The producing method has a profile identity, checksum, origin, resolution, and alias-map version. Each proposed task route separately records its profile identity/checksum and a declared or measured capability artifact checksum. Route support is specific to that evidence; it is not a fluency claim. Source, directory, and publisher hints are not observation origins in this writer. Future corrections remain separate evidence.

Language labels use BCP 47 syntax and normalized case through `oxilangtag` 0.1.6. The boundary rejects repeated variants or extension singletons, preserves the original provider label, and bounds each label to 128 bytes. Registry validity and deprecated-alias canonicalization are not claimed: `iw` stays `iw`, and `i-klingon` stays distinct from `tlh`. An adapter must apply its recorded mapping. `und`, `mul`, and `zxx` use observation states instead. Acoustic observations cannot establish script subtags; script evidence requires recognizer or text origin. Regional labels such as `fr-CA` remain representable when evidenced. See the [parser review](../../research/30-language-pipeline-evaluation.md#language-tag-boundary).

## Publication and inspection

Validation precedes one immediate transaction that appends the revision. Exact replay returns the existing revision. Conflicting replay and skipped revisions fail. A track cannot move to another analysis identity. Database triggers enforce current published input, transcript parentage, sequential revisions, and immutable rows. Reads and startup audits validate semantic payloads and history again.

New observations require a current published pin whose retained catalog binding still matches. Span-free terminal outcomes remain recordable after retention expiry, but still require the current published input revision. Existing evidence and exact replay remain available after audio expires. Evidence does not hold media or reserve processing costs. This storage boundary does not read audio or authenticate a worker result. Later supervised workers must hash their input, pin the actual profile, and atomically publish terminal attempt state with the result.

Bounds are 64 revisions per identity, 256 identities, 64 MiB of serialized evidence per catalog, 64 KiB per revision, 1,024 spans per revision, and eight known labels per span. Byte limits can be reached before item limits. Publication refuses exhausted capacity; no historical pruning is implied.

`analysis languages list PIN --revision REVISION` lists the latest evidence revision for each track on that exact input revision. `--after ID` continues. `analysis languages show EVIDENCE --revision REVISION` reads up to 16 spans from one immutable revision; `--after ORDINAL` continues. Reads neither run a detector nor create evidence. Plain output uses the existing terminal-text sanitizer; stored and JSON provider labels preserve their original text. Immutable byte bounds and 16-record pages keep these views within the existing 256 KiB IPC envelope. There is no CLI or IPC evidence writer.

## Consequences and evidence

Catalog schema is v24 and local IPC is v25. The migration creates no observations for existing transcripts. Stop the older controller before replacing its binary. Fixtures cover mixed and overlapping evidence, partial detector blocks, gaps and bounds, empty-cue rejection, expiry, stale revisions, replay, rollback, capacity, corruption, migration failure, and bounded inspection. Supplied fixture evidence does not demonstrate detection quality.

Operation 24's storage contract is implemented. Its detector-quality gate and operation 23's speech recognition remain open. Follow the [language pipeline plan](../development/language-pipeline.md) and [verification record](../development/progress.md) before adding native workers or claiming quality. This increment adds no model assets or native libraries. `oxilangtag` has no mandatory dependencies and retains its MIT license; the domain core stays dependency-free.
