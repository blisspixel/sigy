# 0032: Local transcript of one published recording

Date: 2026-09-22. Status: historical persistence groundwork for one empty original-script revision. The production writer is superseded by [retained-input verification](0034-retained-input-verification.md); `analysis transcribe` now fails until a measured recognizer is configured. Existing rows remain readable. Speech recognition, measured language detection, translation, and provider dispatch stay open.

## Decision

The original `analysis transcribe` increment read one published analysis pin and hashed each retained local file. It did not open a source URL, decode speech, or contact a network. No measured recognizer was selected. The following paragraphs describe that preserved legacy format.

The stored revision is original script, not a translation. Each published interval gets one cue. The cue script is empty, because unmeasured speech is not invented. The wording label is `uncertain`. A gap gets no cue.

The analysis decision stores 0 USD and a null request id. The command does not reserve a paid request and does not change the global limit. The transcript, its cues, and the decision commit in one transaction. Replay of the same pin returns that revision. A rollback before commit leaves no transcript.

## Consequences

Catalog schema is v23. Local IPC is v24. Stop an older service before replacing its binary. This does not store language spans, translate, or exit stage 5.

The [language pipeline plan](../development/language-pipeline.md) completes operation 23's recognition gate after a measured runtime decision. It must preserve these empty revisions as unmeasured history, admit asynchronous supervised work, and publish actual speech output with finite resources and media provenance. This decision records the existing storage slice; it does not claim that operation 23's full recognition exit has passed.
