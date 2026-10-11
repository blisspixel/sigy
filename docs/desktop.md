# Desktop radio controls

Sigy's desktop integration is a development preview. On ordinary Linux desktops, including Omarchy, native containment currently blocks playback and recording. Cached discovery, the explorer and the read-only status bar remain useful. Run `sigy doctor` before trying audio; [platform limits](install.md#platform-limits) explain the remaining adapter work.

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
