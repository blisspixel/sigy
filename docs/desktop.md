# Desktop radio controls

Sigy's desktop integration is a development preview. Linux playback and recording require the experimental delegated systemd user-session adapter and an accessible output device. Cached discovery, the explorer and the read-only status bar remain useful independently. Run `sigy service start`, configure FFmpeg and inspect `sigy doctor` before trying audio; [platform limits](install.md#platform-limits) distinguish native bounds from desktop audibility.

## Waybar on Omarchy

Install Sigy, run `sigy init --radio`, then confirm `sigy panel bar` prints one JSON object. Add `custom/sigy` to your existing Waybar module list and merge this module into your config. Keep your other modules and settings.

```json
"custom/sigy": {
  "exec": "sigy panel bar",
  "return-type": "json",
  "format": "{text}",
  "interval": 10,
  "max-length": 28,
  "tooltip": true,
  "on-click": "omarchy-launch-or-focus-tui sigy tui",
  "on-click-right": "sigy panel stop"
}
```

The launcher is an [Omarchy command](https://github.com/omacom/omarchy/blob/master/bin/omarchy-launch-or-focus-tui). On other desktops, replace it with your installed terminal's command for opening `sigy tui`. Ensure the graphical session can resolve `sigy` on PATH, or use its absolute installed path. Add the same explicit `--data-dir` to all three commands when using a custom library. Restart/reload Waybar using your desktop's normal controls.

Add these rules to your existing Waybar stylesheet and adapt colors to your theme:

```css
#custom-sigy { padding: 0 8px; }
#custom-sigy.offline { color: #999999; }
#custom-sigy.failed { color: #ff7777; }
#custom-sigy.playing { color: #88ddaa; }
#custom-sigy.recording { color: #ffbb66; }
#custom-sigy.stale { color: #dddd88; }
#custom-sigy.idle { color: #bbbbbb; }
```

State priority is offline, failed, playing, recording, stale, idle. Hover text summarizes bounded favorites, recordings and qualification limits; directory text is escaped for Waybar markup. Polling neither starts a service nor refreshes the directory, plays audio or sends a directory click. See [Waybar's custom module contract](https://github.com/Alexays/Waybar/wiki/Module:-Custom).

## Choose, play and stop

Open the explorer, browse a cached station, press `P` and confirm with `y`. `S` requests a listen stop. A country/tag page can be requested explicitly with `sigy radio search --country CA --tag jazz --fetch`, then reloaded in the explorer. Opening the bar or selecting a row grants no stream contact.

`sigy play STATION_UUID` is the equivalent direct command. It requires a running service, a configured decoder and enforceable limits. It registers a first direct-audio revision only when explicitly invoked, denies redirects by default and prints the listen/revision identities. `sigy stop` addresses the latest quick/panel session; use `sigy listen stop LISTEN_ID` for an exact receipt. Stops leave recordings running.

For a launcher that already selected a registered favorite, `sigy panel play STATION_UUID --cancel-on-stdin --parent-pid PID` retains stricter admission: cached favorite, non-HLS and exactly one audio revision. Keep stdin open and pass the actual launcher's PID. Closing stdin or losing that parent requests cleanup. An optional `--id` preserves explicit request identity; reuse does not reconnect. Inspect an existing receipt with `sigy listen status LISTEN_ID`, or omit `--id` for a fresh listen. Never interpolate directory names or URLs into shell commands. The explorer supplies the station UUID as a separate process argument.

The session file is a hint. Service receipt, decoder outcome, native drain and acoustic output are distinct. Linux/Omarchy qualification flags remain false, and this configuration does not qualify those platforms.

## Later Omarchy listening check

Install the updated development release using the [source installation guide](install.md). Confirm `sigy --version` and the installer's recorded commit match the intended build. In a terminal on your Omarchy desktop, use a fresh private library so the check leaves your existing library alone:

```sh
qa_library=$(mktemp -d /tmp/sigy-qa.XXXXXX)
printf 'Test library: %s\n' "$qa_library"
sigy --data-dir "$qa_library" init --radio --tag jazz
sigy --data-dir "$qa_library" dvr configure --decoder "$(command -v ffmpeg)"
sigy --data-dir "$qa_library" doctor
sigy --data-dir "$qa_library" tui --tag jazz
```

In Explore, select a station, press `P`, then `y`. Listen through your normal desktop output, then press `S` to stop. Press `R`, then `y` to explicitly fetch another matching page if needed. Note the station UUID, any visible refusal and whether audio starts and stops. A redirect refusal or unavailable station is a separate source result; try another direct station rather than relaxing its policy silently.

After quitting with `q`, compare the same station through the CLI in one terminal and stop it from another:

```sh
sigy --data-dir "$qa_library" play STATION_UUID
# In another terminal, substitute the printed test-library path:
sigy --data-dir /tmp/sigy-qa.PRINTED_SUFFIX stop
```

Finally run `sigy --data-dir "$qa_library" service stop` in the first terminal. Keep the test directory if you want to inspect receipts. Report the Sigy commit, `systemd --version`, FFmpeg version and audio backend with the outcome; avoid posting your library or station URLs without checking them. This manual device check is pending and is separate from automated loopback decoding and containment tests.
