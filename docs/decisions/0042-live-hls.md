# 0041: Live HLS recording and master playlist candidates

Date: 2026-09-24. Status: implemented; local loopback validation only, recorded below.

The [live station pilot](../../research/experiments/local-asr/live-station-pilot.md) found that 3 of 8 public news stations (Chinese, Hindi, and one Arabic broadcaster) publish only live HLS. [Finite HLS](0012-hls-media-playlist.md) accepted only a media playlist with `#EXT-X-ENDLIST`, and [playlist resolution](0009-playlist-resolution.md) rejected every HLS document. This record keeps the existing authority model: a person chooses one candidate, that choice becomes an immutable revision, and the decoder never receives a URL.

## Master playlists are candidate lists

`source playlist resolve` on an HLS master playlist lists its `#EXT-X-STREAM-INF` variants and its `#EXT-X-MEDIA` audio renditions that have their own URI. The read is unchanged: one document on the shared acquirer, 64 KiB, the directory document deadline, the revision's redirect policy, identity encoding, no cookies, and no retry. At most 32 candidates are kept. Each URI resolves against the final response URL and must pass the parent network scope and redirect policy, as an M3U entry does. No variant, rendition, or nested playlist is fetched, and nothing is chosen automatically.

Each candidate stores its kind (`hls_variant` or `hls_audio`), the declared `BANDWIDTH`, the declared `CODECS`, and an audio-only flag. The flag is true when every declared codec is an audio codec, false when the variant declares `RESOLUTION`, `VIDEO`, or a non-audio codec, and unknown when `CODECS` is absent. These are publisher claims, not measurements. Ordinary views show the origin and these attributes. The resolved URI stays in the private catalog, as entry paths already do. `source playlist accept` registers the chosen candidate as a new immutable `http_audio` revision exactly as it registers an entry, with no network I/O.

A variant needs `BANDWIDTH` and must be followed by its URI line. `#EXT-X-SESSION-KEY`, `#EXT-X-DEFINE`, media segment tags, and unknown tags fail the resolve. `#EXT-X-I-FRAME-STREAM-INF`, `#EXT-X-SESSION-DATA`, `#EXT-X-START`, `#EXT-X-CONTENT-STEERING`, other rendition types, and comments are ignored. A media playlist fails the resolve with a pointer to `record hls` and stores no candidates. A directory HLS flag is now only a hint: the document decides, so a flagged station's master playlist can be resolved.

## Live media playlists

`record hls --live` records a live media playlist. The first document must be a media playlist without `#EXT-X-ENDLIST` and with `#EXT-X-TARGETDURATION` from 1 to 60 seconds. Without `--live`, a live playlist still fails, and with `--live` an ended playlist fails. Capture starts three segments before the live edge. Each media sequence number is fetched at most once and appended in order. The service reloads the same revision about once per target duration after new segments and after half a target duration otherwise, never sooner than one second, and stops reloading at the time ceiling. Segments and reloads share the requested `--seconds` and `--max-mib` ceilings, and the capture ends at the first ceiling or stop.

Only whole segments are published. A segment that a ceiling, a stop, or a transport failure cuts short is removed from the staging file before publication. A capture that ends before one whole segment fails and publishes nothing.

A live capture ends at the first hole it would otherwise hide:

| Event | Gap cause |
| --- | --- |
| The next media sequence number expired from the window before it was fetched | `sequence_skip` |
| The next segment starts a new discontinuity sequence | `discontinuity` |
| A reload fails, is not a usable media playlist, or adds no segment for three target durations | `reload_failure` |
| A segment request fails after audio was received | `disconnect` |
| A segment declares a different media type | `codec_change` |

The whole segments received before the event are published through the existing decoder check, hash, and quota path with the end reason `stream_gap`, and the rest of the plan is recorded as one [capture gap](0026-capture-gaps.md) with that cause, starting at the decoded end of the published file. The capture does not resume after a gap. A published HLS recording is still one file with one measured interval, and the interval model does not allow a hole inside it, so a mid-capture gap followed by more audio is not supported. There is no retry: a single failed reload ends the capture. Reloads and segments use the shared acquirer's two attempt slots, so a reload refused for capacity is a reload failure. A sequence restart on the server looks like a stalled playlist and ends as a reload failure.

## Segments and formats

HLS segments, finite or live, may be MPEG transport streams served as `video/mp2t` or ADTS AAC served as `audio/aac`, besides the existing audio types. `video/mp2t` is accepted only for HLS segments: `record start` and `listen source` still reject it. A transport stream recording has the format `mpegts`. The decoder check, retained playback, and local recognition pass `-f mpegts` with a local file or pipe and decode the first audio stream only. Video packets inside a transport stream are stored and count toward the quota, so an audio-only variant is the better choice.

Media playlists still reject `#EXT-X-KEY` with any method, `#EXT-X-MAP` (fragmented MP4), `#EXT-X-BYTERANGE`, `#EXT-X-DEFINE`, `#EXT-X-GAP`, the low-latency tags `#EXT-X-PART`, `#EXT-X-PART-INF`, `#EXT-X-PRELOAD-HINT`, `#EXT-X-RENDITION-REPORT`, and `#EXT-X-SKIP`, and unknown tags. `#EXT-X-PROGRAM-DATE-TIME`, `#EXT-X-DATERANGE`, `#EXT-X-ALLOW-CACHE`, `#EXT-X-START`, `#EXT-X-SERVER-CONTROL`, `#EXT-X-BITRATE`, and comments are ignored. Every segment needs `#EXTINF`. A finite playlist still rejects `#EXT-X-DISCONTINUITY` and keeps the 32-segment limit. A live window may list up to 1,024 segments within the 64 KiB document limit.

Segment URLs resolve against the final URL of the playlist document that listed them and pass the revision's network scope and redirect policy, including cross-origin rules. Segments do not become source revisions. The stored route is the first segment's response chain. Nothing reports a click or a vote. An exact replay of the recording id starts no second worker; as with finite HLS, the `--live` flag is not part of the stored capture plan.

## Storage and protocol

Catalog schema is v30. Migration 030 follows the v29 translation schema. `playlist_entries` gains `kind`, `bandwidth`, `codecs`, and `audio_only`, and existing rows become plain entries. `recordings.format` and `recording_intervals.format` accept `mpegts`, `recordings.end_reason` accepts `stream_gap`, and `recording_gaps.cause` accepts the three new causes. SQLite cannot alter a CHECK constraint, so each of those three tables is rebuilt from its own stored definition with exactly one textual change per widened list. Rowids and every row are copied, the table's own indexes and triggers are recreated verbatim, child foreign keys are deferred inside the migration transaction and checked before and after commit, and a definition that does not match the expected text fails the migration and leaves the catalog at its old version. Local IPC is v30 because the recording operation gained `live` and playlist views gained candidate attributes. Stop an older controller before replacing the executable. No dependency was added.

## Evidence and limits

Parser tests cover master variants and renditions, sliding live windows with sequence and discontinuity numbers, hostile attribute lists, tags, durations, sequence overflow, and window limits. Loopback acquirer tests cover one fetch per sequence, a skipped sequence, a discontinuity, a failed and an unusable reload, a stalled playlist, a media type change, a failed segment, transport stream acceptance, and whole-segment byte limits. A service fixture on Windows with FFmpeg 9.0.1 cut a local tone into one-second AAC segments with the configured FFmpeg, served a live playlist over 127.0.0.1 that advanced once per reload and then skipped a sequence, and recorded it with `--live`: the published file equals the five whole transport stream segments fetched, decoded to about five seconds, has one `sequence_skip` gap from its decoded end to the planned end, and played to `--destination null`. A second fixture recorded ADTS segments until the time ceiling with no gap. A playlist fixture resolved a master playlist to two variants with no variant request and accepted one.

This does not qualify any public station, including the three from the pilot, and does not cover encrypted media, fragmented MP4, low-latency parts, alternate audio groups beyond listing them, long captures, reload timing under load, or clock drift. The start point, reload interval, and stall rule are design choices, not measured values.

## Repeated CODECS

Amended 2026-09-24 after a live check: a national broadcaster's master playlist repeats `CODECS` in each `EXT-X-STREAM-INF`, which RFC 8216 does not allow. Because `CODECS` is a publisher claim that grants nothing, a repeated `CODECS` makes the variant's codecs and audio-only flag unknown instead of rejecting the playlist. Any other repeated attribute, such as `BANDWIDTH` or `URI`, still rejects the document.
