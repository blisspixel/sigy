# 0010: Directory clicks are explicit and do not open the stream

Date: 2026-09-21. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

Radio Browser counts a deliberate play with `GET /json/url/{stationuuid}` on a directory mirror. The provider document reviewed on 2026-09-21 says the same address is counted once per day, and the JSON body returns a stream URL. That URL is not a source, a recording, or permission to connect. Sigy checks that the acknowledgement names the requested station, then drops the URL and the message. Redirects on this call stay denied. The vote path `/json/vote/` is never requested.

`radio click` requires a cached station and the running service. An omitted mirror uses one discovered public mirror, with no failover, because a failed response may already have been counted. An explicit pinned mirror is for a local fixture or a chosen catalog. Reusing the request ID does not send again, including after failure or interruption. A new ID is required to try again, and clicks are at least two seconds apart. Search, show, favorite, refresh, playlist resolution, and file playback do not send this request.

The catalog and local IPC advance to v9. Stop older controllers before replacing the executable. Migration from v8 adds the click table without changing stations, sources, recordings, or budgets.
