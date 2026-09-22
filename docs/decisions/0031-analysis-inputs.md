# 0031: Bind analysis to a published recording

Date: 2026-09-22. Status: implemented for one input pin. Transcription, translation, and topic monitors stay open.

## Decision

`analysis admit` pins one completed recording. The pin stores the retained checksum and the media clock: published interval bounds and the gaps that cover the rest of the planned window, including time with no capture gap. The pin does not store a source URL, a stream endpoint, or the cleanup receipt from `record processed`.

An unpublished recording is refused. A retained checksum that does not match the published interval is refused. Replacing an unpublished worker creates the next revision and leaves the old one unable to publish. Publishing the current revision is idempotent. The admission does not reserve a paid budget and does not start a model.

## Consequences

Catalog schema is v22. Local IPC is v23. Stop an older service before replacing its binary. This does not transcribe, translate, or exit stage 4 or stage 5.
