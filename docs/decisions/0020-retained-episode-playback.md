# 0020: Retained episode playback

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`listen file` plays one retained episode recording through the existing retained-file player. The episode has no live edge. `listen source` on that enclosure revision fails before a listen receipt and before any request. Subscribe and refresh still start neither download nor playback. The list explorer does not play it.

The recording stays under the library retention policy. The tested policy is the default 14 days and 50 GB. The tested recording is temporary. Playback does not change charged bytes, reserved bytes, or retention. A seek at the published duration fails. `--destination null` is the tested path. The decoder receives the local published file, not the enclosure URL.

An unpublished recording is not playable. Transcript and chapter URLs are not fetched. Catalog schema stays v14. Local IPC stays v15. There is no migration. This does not exit stage 4. Publisher text is recorded in [publisher text](0022-publisher-text.md).

Verification covers refusal of a live listen on an admitted episode revision while a radio revision can still start, and one local WAV fixture. That fixture subscribes, inspects the subscription and episode list, downloads one enclosure, and plays the retained file. It contacts only 127.0.0.1. The policy stays 14 days and 50 GB, the recording stays temporary, playback advances inside the published duration, charged bytes do not change, and a later refresh does not fetch the enclosure again.
