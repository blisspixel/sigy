# 0027: Segment playback sessions

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

A playhead is an in-memory session on the service. It is not a catalog row and not a direct-listen receipt. Many sessions can name one capture. Restart drops the sessions and does not resume them. Closing a session does not stop the capture.

Playback reads one sealed segment file in the client. The decoder receives that local file only. The open tail is visible and not readable. Return to live parks at the end of the newest published segment and does not start the decoder. Seek moves one playhead only inside a retained published segment, and only inside that segment's decoded duration. A seek inside a gap fails before the decoder starts. Playback pause stores the playhead. It does not signal the capture worker and does not write a gap. `record pause` remains the command that interrupts capture.

A paused position is expired when published audio no longer covers it. The stored position stays until the listener seeks inside retained audio or returns to live. `listen play` decodes the current segment and then drops that playhead. Leaving the command does not stop the capture. Rolling deletion of segments is a later operation.

The segment lease is two receive windows so the close can renew before the lease expires. An expired lease is still not extended. The 5000 ms receive cut is unchanged.

Catalog schema stays v18. Local IPC is v19. This does not exit stage 4.
