# 0027: Segment playback sessions

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

A playhead is an in-memory session on the service. It is not a catalog row and not a direct-listen receipt. Many sessions can name one capture. Restart drops the sessions and does not resume them. Closing a session does not stop the capture.

Playback reads one sealed segment file in the client. The decoder receives that local file only. The open tail is visible and not readable. Return to live parks at the end of the newest published segment and does not start the decoder. A seek inside a gap still fails. Playback pause stores the playhead. It does not signal the capture worker and does not write a gap. `record pause` remains the command that interrupts capture.

A paused position is expired when published audio no longer covers it. The stored position stays until the listener seeks or returns to live. Rolling deletion of segments is a later operation.

Catalog schema stays v18. Local IPC is v19. This does not exit stage 4.
