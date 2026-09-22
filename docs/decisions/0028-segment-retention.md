# 0028: Hold and prune sealed segments

Date: 2026-09-22. Status: implemented for the rolling buffer. The globe and stage 4 stay open.

## Decision

A running or finished radio capture can seal many segments. `record hold` saves one half-open timeline range. In one transaction it protects every published segment that range intersects, and it records each existing gap inside that range. The open tail is not a segment and is not protected. Keep and Archive exempt the whole recording and still count toward the quota.

The service sweep deletes an aged temporary segment file before it records the release. A second sweep does not charge those bytes again. A running reservation stays charged until the capture completes, and completion charges only the segments that still have files. Quota pressure releases the oldest retained temporary segment before it deletes a whole recording. A processing receipt does not protect a segment and does not start analysis. A playhead does not pin retention.

The interval row stays after the file is gone. Playback treats that segment as no longer retained, so the earliest retained position moves forward.

## Consequences

Catalog schema is v19. Local IPC is v20. Stop an older service before replacing its binary. This does not refresh the station cache, draw the globe, or exit stage 4.
