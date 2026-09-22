# Using Sigy

This is the command reference for the current checkout. Examples use `sigy` after [installation](../README.md#install). From the repository without installing, put `cargo run --locked -p sigy --` in front of the same arguments. Planning documents describe later behavior. A command is current only when it appears here or in `sigy --help`.

`--data-dir` is required except for `sigy update`, help, and version. Use a private directory outside the checkout. `--json` emits one structured response for automation. Amounts in JSON are exact decimal USD strings. Displayed origins omit paths and queries. Full URLs stay in the private catalog in plaintext. Do not put access credentials in source URLs.

Catalog schema is v18 and local IPC is v19. Stop an older service with its existing binary before replacing that binary, then start it again. `service run` keeps the controller in the foreground. `service start` detaches it from the client. Neither command installs an operating-system startup service. The service holds the library lock. Other commands reconnect to it while it is running.

## Update

```text
sigy update --check
sigy update
```

`sigy update` fetches `main` from <https://github.com/blisspixel/sigy> and installs that commit with Cargo. When GitHub CLI is logged in, Git uses that login for the private repository. `--check` only reports the recorded commit and that tip. It exits with an error when no commit is recorded or a newer commit is available. The command does not select a library, contact a station, or run `cargo verify`. On Windows the install finishes after this process exits, because Windows cannot replace the running executable. Stop a running service before that replacement.

## Doctor

```text
sigy --data-dir PATH_TO_LIBRARY doctor
sigy --data-dir PATH_TO_LIBRARY doctor --strict
```

`doctor` reads the local library and does not refresh, delete, or contact a network. It reports each check as ok, attention, or blocked. Attention means the library still works and names the command that would improve it. Blocked means a configured check is unusable, and the process exits with an error. `--strict` also exits with an error when any check needs attention.

An empty station cache, a cached station older than 24 hours, or a cached station with no observation time is attention. Those stations stay searchable. Favorites stay. Doctor also runs SQLite `quick_check` and does not print the engine's own error text. A refresh merges one page and does not delete unseen stations. The report suggests `radio refresh NEW_ID --limit 100`. Choose a new id for each fetch. A running refresh is reported instead of starting another one. Podcast snapshots older than 24 hours are attention and are not downloaded by that suggestion. A missing decoder is attention until recording is required. A configured decoder path that is not a file is blocked, and recording refuses to start.

## Library and budget

```text
sigy --data-dir PATH_TO_LIBRARY library init
sigy --data-dir PATH_TO_LIBRARY library status
sigy --data-dir PATH_TO_LIBRARY budget show
```

`library init` creates the catalog with paid processing disabled. `budget set` can store a finite limit. It does not configure or call a provider. Provider dispatch is unavailable.

## Service

```text
sigy --data-dir PATH_TO_LIBRARY service start
sigy --data-dir PATH_TO_LIBRARY service status
sigy --data-dir PATH_TO_LIBRARY service stop
```

`service run` is the foreground form of the same controller. Shutdown lets admitted changes finish. Client exit does not stop the service.

## Sources

```text
sigy --data-dir PATH_TO_LIBRARY source add demo:v1 --name "Radio example" --url https://radio.example/audio
sigy --data-dir PATH_TO_LIBRARY source list
sigy --data-dir PATH_TO_LIBRARY source show demo:v1
```

Registration stores the configuration and does not contact the station. Reusing a revision key cannot change its URL, name, network permission, or redirect policy. Public-internet access is the default. `--pin-address` binds a revision to one supported IP, including a private or loopback address. Lists are paginated with `--limit` and `--after`.

See [source authority and transport](decisions/0004-source-authority-and-http.md).

## Radio directory

Start the service, then request one directory page:

```text
sigy --data-dir PATH_TO_LIBRARY radio refresh french-news-001 --language french --tag news --limit 100
sigy --data-dir PATH_TO_LIBRARY radio refresh-status french-news-001
sigy --data-dir PATH_TO_LIBRARY radio search --language french --tag news
sigy --data-dir PATH_TO_LIBRARY radio show STATION_UUID
sigy --data-dir PATH_TO_LIBRARY radio add STATION_UUID --revision selected:v1 --redirects public
```

Wait until the refresh status is completed, then replace `STATION_UUID` with an id from search. Search supports `--name`, `--country`, `--language`, `--tag`, `--healthy`, and pagination. It reads the partial local cache and does not use the network. Refresh uses the network only when requested. Choose a new refresh id to fetch again. Refresh and registration do not contact station streams, play audio, or run analysis.

Observation age and directory health are separate from stream compatibility. Directory languages are hints, not detected speech. No broadcast detector is implemented. See [broadcast analysis](design/broadcast-analysis.md) for the planned contract.

```text
sigy --data-dir PATH_TO_LIBRARY radio favorite STATION_UUID
sigy --data-dir PATH_TO_LIBRARY radio search --favorites --language french
sigy --data-dir PATH_TO_LIBRARY radio unfavorite STATION_UUID
```

Favorites work offline and survive catalog updates and service restarts. Removing a favorite preserves the station, registered sources, and recordings. See [favorites](decisions/0008-radio-favorites.md).

```text
sigy --data-dir PATH_TO_LIBRARY radio click heard-001 --station STATION_UUID
sigy --data-dir PATH_TO_LIBRARY radio click-status heard-001
```

A click does not play the station. The stream address in the provider response is discarded. Search, show, favorite, and refresh do not send this request. Reusing the click id does not send it again. See [directory clicks](decisions/0010-directory-clicks.md).

`radio add` stores a metadata snapshot and registers the chosen public stream as an immutable source. Use that revision with `record start --source`. `--redirects public` permits at most three redirects to checked public-internet destinations. Omit `--redirects` to deny them, or choose `same-origin` to stay within the original scheme, host, and port. HTTPS cannot downgrade to HTTP. Existing revisions keep their policy. Choose a new revision key to change it. See [directory behavior](decisions/0006-radio-discovery.md) and [redirect policy](decisions/0007-authorized-redirects.md).

## Playlists

```text
sigy --data-dir PATH_TO_LIBRARY source playlist resolve playlist-001 --revision playlist:v1
sigy --data-dir PATH_TO_LIBRARY source playlist status playlist-001
sigy --data-dir PATH_TO_LIBRARY source playlist accept playlist-001 --index 0 --revision chosen:v1 --name "Chosen stream"
```

One request reads one playlist document, allows at most 32 entries, and does not open those entries. HLS tags and a directory HLS flag fail the resolve and store no candidates. Accepting an index registers a new audio revision and does not connect. Reusing the request id does not fetch again. Reusing the same accept does not register again. Resolving or accepting a playlist does not play it. See [playlist resolution](decisions/0009-playlist-resolution.md).

## Podcasts

```text
sigy --data-dir PATH_TO_LIBRARY podcast subscribe show:v1 --url https://show.example/feed.xml
sigy --data-dir PATH_TO_LIBRARY podcast show show:v1
sigy --data-dir PATH_TO_LIBRARY podcast unsubscribe show:v1
```

The feed URL, network scope, address pin, and redirect policy are immutable. Reuse of the same id cannot change them. The default redirect policy is deny. Subscribe does not resolve DNS, download the document, register an audio source, or start a capture. Unsubscribe stops future polls and deletes nothing.

```text
sigy --data-dir PATH_TO_LIBRARY podcast refresh show:v1 --id feed:v1
sigy --data-dir PATH_TO_LIBRARY podcast refresh-status feed:v1
sigy --data-dir PATH_TO_LIBRARY podcast episodes show:v1
```

Refresh reads one RSS 2.0 document on the shared acquirer. A compressed body is at most 2 MiB, the decoded document is at most 8 MiB, and the expansion ratio is at most 16. At most 500 items are committed. Further items mark the snapshot truncated. A failed document leaves the last good snapshot. Omission from a later snapshot does not delete an episode. Transcript and chapter URLs are stored and are not fetched by refresh. A live item is counted and not opened. Episode titles are not identity. Listing episodes works offline.

```text
sigy --data-dir PATH_TO_LIBRARY podcast download show:v1 --episode EPISODE_ID --id episode:v1 --revision enc:v1
```

`EPISODE_ID` comes from `podcast episodes`. The service registers an immutable audio revision for that URL under the subscription grant, reserves 512 MiB and 30 minutes before connecting, and publishes a clean ended file. Radio attempts stay 15 minutes and 256 MiB. A declared length above 512 MiB is rejected before connect. Replay of the same recording id does not download again. Download does not fetch transcripts or chapters.

```text
sigy --data-dir PATH_TO_LIBRARY podcast text show:v1 --episode EPISODE_ID --kind transcript --index 0 --id text:v1
sigy --data-dir PATH_TO_LIBRARY podcast text-show text:v1
```

`podcast text` fetches one stored transcript or chapter document when asked. `--kind` is `transcript` or `chapters`. The result is unverified publisher text. Cue times are publisher times, not media time, and the text is not an automatic transcript. The bytes do not change the recording quota. Replay of the same id does not fetch again. HTML is rejected before connect. Nested chapter image and link URLs are not requested. Restart marks a running fetch interrupted and leaves the previous snapshot.

```text
sigy --data-dir PATH_TO_LIBRARY listen file episode:v1 --destination null
```

The episode has no live edge, so `listen source` on that revision fails before a request. Subscribe and refresh do not start playback. The tested recording stays temporary under the default 14-day and 50 GB policy, and playback does not change charged bytes.

See [subscriptions](decisions/0017-local-podcast-subscriptions.md), [RSS refresh](decisions/0018-rss-feed-refresh.md), [enclosure download](decisions/0019-episode-enclosure.md), [retained playback](decisions/0020-retained-episode-playback.md), and [publisher text](decisions/0022-publisher-text.md).

## Recording and retention

```text
sigy --data-dir PATH_TO_LIBRARY dvr configure --decoder ABSOLUTE_PATH_TO_FFMPEG --quota-gb 50 --retention-days 14
sigy --data-dir PATH_TO_LIBRARY service start
sigy --data-dir PATH_TO_LIBRARY record start morning-001 --source demo:v1 --seconds 60 --max-mib 64
sigy --data-dir PATH_TO_LIBRARY record show morning-001
sigy --data-dir PATH_TO_LIBRARY record path morning-001
sigy --data-dir PATH_TO_LIBRARY record metadata morning-001
sigy --data-dir PATH_TO_LIBRARY record keep morning-001
sigy --data-dir PATH_TO_LIBRARY dvr status
```

Pass the installed FFmpeg executable to `dvr configure --decoder`. Sigy does not download it. `record start` returns while the service continues. Poll `record show` until the attempt is completed or failed. `record path` prints a verified retained file's path. Add `--icy` only when interleaved stream titles should be stored as observations. The default is off, and those titles are not written into the audio file. See [ICY observations](decisions/0013-icy-observations.md).

```text
sigy --data-dir PATH_TO_LIBRARY record hls segment-001 --source media:v1 --seconds 60 --max-mib 64
```

`record hls` records one finite media playlist. The document must include `#EXT-X-ENDLIST`. At most 32 segments share that recording's time and byte ceiling. The service publishes one local file through the same decoder check, hash, and quota. A master playlist fails before a variant request. A live playlist, encryption, a media map, a byte range, discontinuity, and partial segments are rejected. The decoder receives the published file, not an `.m3u8` address. See [HLS media playlists](decisions/0012-hls-media-playlist.md).

A published file has one measured interval: decoded duration and published byte length, both starting at zero. The requested window stays the plan. A recording that has not been published has no interval. See [measured intervals](decisions/0023-recording-intervals.md).

```text
sigy --data-dir PATH_TO_LIBRARY record pause morning-001
```

`record pause` stops a running capture. The part of the plan with no published audio becomes a gap. A disconnect, service recovery, codec change, refused segment renewal, and a backward clock write a gap with that cause. A gap is not a silence file. `listen file` refuses a seek inside a gap and plays one sealed segment, including while the capture is still running. The open tail is not readable. See [capture gaps](decisions/0026-capture-gaps.md) and [segment playback](decisions/0027-segment-playback.md).

```text
sigy --data-dir PATH_TO_LIBRARY listen attach listener-a --recording morning-001
sigy --data-dir PATH_TO_LIBRARY listen pause listener-a
sigy --data-dir PATH_TO_LIBRARY listen seek listener-a --seek-us 200000
sigy --data-dir PATH_TO_LIBRARY listen live listener-a
sigy --data-dir PATH_TO_LIBRARY listen play listener-a --destination null
sigy --data-dir PATH_TO_LIBRARY listen detach listener-a
```

`listen pause` stores that playhead. It does not stop the capture and does not write a gap. `listen seek` moves it only inside one published segment, and only inside that segment's decoded duration. A gap and the open tail are refused. `listen live` parks at the end of the newest published segment and does not read the open tail. `listen play` plays that sealed file in this client and then drops the playhead. Leaving it does not stop the capture. `listen detach` drops a playhead that is still attached. A second attach is another playhead on the same recording. A paused position that falls before the earliest retained segment is expired until the listener seeks or returns to live.

Default retention is 14 days and 50 GB of managed media. A temporary recording may stay while it is processed. `record keep` and `record archive` keep it, and those bytes still count toward the quota. Archive does not create a backup. New recordings fail when protected media fills the allowance. `record temporary` restores rolling retention. `record processed ID --receipt RECEIPT_ID` records that processing finished. It does not run analysis. The service sweep then deletes that temporary file. The same sweep deletes other temporary recordings older than the retention window. Quota pressure removes the oldest temporary recordings sooner. `dvr prune` runs that same reclaim. `record delete` deletes inactive media even when it is protected and keeps the catalog history.

Direct audio, explicitly permitted redirects, one finite HLS media playlist, and explicit ICY metadata on `record start --icy` are the current recording profile. A radio attempt is at most 15 minutes and 256 MiB, with up to two active attempts. On Windows, with FFmpeg 9.0.1, one local session decoded direct WAV, MP3, AAC served as `audio/aac`, FLAC, and Ogg Vorbis. The same session decoded WAV from one accepted playlist entry behind one same-origin redirect and played that revision to `--destination null`. The fixture contacted only 127.0.0.1. One session is not a support matrix. See [decoded formats](decisions/0014-decoded-formats.md).

Playlist media types still fail on `record start`. A listen, an HLS recording, and `record start` without `--icy` still reject `icy-metaint` before writing audio. A body that hits a ceiling without a clean end is not playable. Continuous segmented recording is not qualified yet. Failed and partial bytes keep their reservation and are not offered as playable media. See [recording and retention](decisions/0005-recording-and-retention.md) and [recording metadata](design/recording-metadata.md).

## Listening

```text
sigy --data-dir PATH_TO_LIBRARY listen file morning-001 --destination null
sigy --data-dir PATH_TO_LIBRARY listen file morning-001 --destination system --seek-us 500000
```

`--destination null` discards samples and is the tested path. `--destination system` uses a local audio output when that FFmpeg build has one, and fails when it does not. `--seek-us` starts inside the published duration. Run the command again to restart at another offset. Leaving it stops playback and leaves service-owned capture running. Playback does not stop a recording, change retention, or reserve quota. Partial files are not playable. `record metadata` emits a versioned JSON sidecar. Reusing the same recording id and parameters reconciles the prior request. Choose a new id for another recording.

```text
sigy --data-dir PATH_TO_LIBRARY listen source live-001 --revision demo:v1 --destination null
sigy --data-dir PATH_TO_LIBRARY listen status live-001
sigy --data-dir PATH_TO_LIBRARY listen stop live-001
```

The service fetches the bytes. This client decodes a private local pipe and does not receive the source URL. Reusing the listen id does not open the source again. A playlist response or interleaved ICY metadata fails before decode. The session is not a recording and does not reserve quota. Restart marks a running listen interrupted and does not resume it. Stopping it does not stop a recording. See [direct listen](decisions/0011-direct-listen.md).

## List explorer

```text
sigy --data-dir PATH_TO_LIBRARY tui
sigy --data-dir PATH_TO_LIBRARY tui --reduced-motion --monochrome
```

`sigy tui` is the list explorer on Ratatui 0.30.2 with the Termina 0.3.3 backend. Selection does not start audio, capture, refresh, or a click. Quit does not stop the service. The default drawing uses the terminal palette for connection, health, favorites, and focus. The words stay, so the screen still reads with color removed. `--linear` uses one reading order and draws no semantic color. `NO_COLOR`, `--monochrome`, and `TERM=dumb` also select monochrome. Human command output uses the same roles on a terminal and stays plain when piped or when `--json` is set. `FORCE_COLOR` can request color for a pipe, and `NO_COLOR` still wins. `--inspect PATH` draws one frame, writes a size report, and exits. The globe, map, monitors, and findings are unavailable. See [the list explorer](decisions/0016-list-explorer.md).

## Agents

`sigy mcp` speaks MCP 2026-07-28 on stdio. The portable package is `agent-plugin/`, in the Agent Plugins 1.0.0 layout. The server library is the `--data-dir` from startup. A tool cannot point at another directory, change a budget, or run a shell command. See [agent plugin](decisions/0021-agent-plugin.md).

## Verification

From the repository root, `cargo verify` checks native-source hashes, formatting, tests, warnings-denied Clippy, the build, and dependency audits. It requires cargo-audit. A push to `main` runs that command on GitHub-hosted Windows. `cargo verify-media` runs the native recording and playback tests. It needs FFmpeg on `PATH`, or `SIGY_TEST_FFMPEG`. That media check stays local. The install scripts do not run these checks.

## Lawful use

Sigy is intended for lawful listening, research, learning, and analysis of sources you are authorized to access. You are responsible for complying with the laws and permissions applicable to your location, equipment, and use, including rules governing reception, interception, recording, privacy, decryption, radio transmission, copyright, and redistribution. A signal being receivable or a stream being accessible does not by itself establish permission to record, decrypt, publish, or reuse it.

Do not use Sigy for unauthorized access or interception, unlawful decryption or disclosure, harmful interference, or transmission without any required authorization. Respect source and service terms. Hardware integration is initially scoped to reception. Modern decryption workflows use supplied keys and supported protocols.

Transcriptions, translations, identifications, and generated findings can be incorrect. Check important results against the original material. This documentation is not legal advice, and this notice does not make an otherwise unlawful activity permissible. It does not add restrictions to the Apache License.
