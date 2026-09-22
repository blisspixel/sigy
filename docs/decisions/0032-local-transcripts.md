# 0032: Local transcript of one published recording

Date: 2026-09-22. Status: implemented for one original-script revision. Language spans, translation, and provider dispatch stay open.

## Decision

`analysis transcribe` reads one published analysis pin and hashes each retained local file. It does not open a source URL, decode speech, or contact a network. No measured recognizer is selected.

The stored revision is original script, not a translation. Each published interval gets one cue. The cue script is empty, because unmeasured speech is not invented. The wording label is `uncertain`. A gap gets no cue.

The analysis decision stores 0 USD and a null request id. The command does not reserve a paid request and does not change the global limit. The transcript, its cues, and the decision commit in one transaction. Replay of the same pin returns that revision. A rollback before commit leaves no transcript.

## Consequences

Catalog schema is v23. Local IPC is v24. Stop an older service before replacing its binary. This does not store language spans, translate, or exit stage 5.
