# Interface language

Updated: 2026-10-21. Status: selected contract for the interface issues under [tracker 23](https://github.com/blisspixel/sigy/issues/23). The words stay authoritative. Color only reinforces them. This file records the rules those issues implement. Issue 25 applies the vocabulary, shared counts, compact USD and the recording timeline text. Issue 24 applies the color roles, favorite and live markers, and high contrast. Issue 26 applies one next command, the readable doctor report, journey help, and the key overlay. Issue 27 applies the explorer layout, including a hint line that ends in `? more`. Issue 28 applies globe motion. Issue 30 applies the exit codes under Agent and JSON parity. Issue 31 refuses a non-interactive explorer. Nothing here grants network, playback, capture or spending authority.

Low floor: a curious newcomer can turn the globe, pick a station and press play in the first minute. High ceiling: an analyst can trace any line to a field, a revision and an exact interval, and script all of it. Character: a calm, modern instrument with retro roots. The feel comes from real geography, real timelines and crisp alignment, with a few quiet flourishes that explain what changed.

## Voice

Lead with what happens. State a boundary once, where someone might fear it: help, first run, and the confirm prompt. One action has one name. Command and flag names may stay for compatibility. Copy uses the canonical verb.

| Use | Meaning | Retired in copy |
| --- | --- | --- |
| play / stop | start or end audible output now | quick-play, quick listen |
| listen session | the receipt for a play | playback receipt |
| fetch | an explicit directory network request (`R`, `--fetch`) | refresh, in prose |
| reload | re-read local state (`r`) | refresh, for a local re-read |
| inspect | open detail (`Enter`) | read, open |
| favorite | mark a station (`f`) | save |
| turn | move the globe | rotate |
| first page | `g` | restart |
| record | keep audio | capture, in UI copy |

Plain words come first. The exact value is one keypress or `--json` away: `Covered 3 h 10 m of 4 h`, with the microsecond interval in detail and JSON. Labels are sentence case and end with a colon. No internal ID sits on a primary line. Numbers for people use `50 GB`, `$0.00`, `m:ss`, `10,000` and `1 candidate`. JSON keeps exact values.

Money shown to a person uses compact exact USD: at least two decimal places, and every meaningful sub-cent digit up to the existing six-place precision. Examples are `$0.00`, `$3.20`, `$0.000001` and `$3.200001`. A positive allowance, reservation or balance is never displayed as zero. Formatting stays integer and string work. `--json` and accounting are unchanged. `Paid processing: unavailable` states the existing dispatch fact. A configured allowance is not evidence that paid dispatch can run.

Next steps read `Next: <full copyable command>`. The command starts with `sigy`, includes the user's filters, and is identical for identical situations. It never asks the user to invent a value Sigy can generate. The deliberate `PATH_TO_LIBRARY` placeholder in `init` guidance stays.

Errors use one line that says what failed, then `Next:`. Extra detail goes on a labelled `Detail:` line or in `--json`. Play and humor live only in optional exploration copy. Failures, money, evidence and JSON stay literal. No emoji, no em dashes, and no "successfully".

Issue 25 owns the copy change. Issue 26 applies the shared next-step helper, the doctor report and journey-grouped help.

## Color

Each role means one thing, on every surface, in the user's terminal theme. The CLI, the explorer, the globe and the panel read one role table. `sigy panel desk` uses those roles. `sigy panel status` text stays uncolored. Sigy ships no palette and no `--theme` flag. There is no `SIGY_THEME`. Light and dark mean the same ANSI slots read on the user's light or dark palette.

Built only on the 16 ANSI colors plus the terminal's default foreground and background. No RGB, no 256-color indexes, and no magenta (ANSI 5 or 13). No background color except reverse video for selection. Never use ANSI 0 for text. Never use ANSI 7 or 15 for text or thin lines.

| Role | ANSI | Means | Kept off this role |
| --- | --- | --- | --- |
| text | default foreground | content and field labels | accent on every label |
| accent | 6 cyan | where you are: brand, current workspace, focused section, key letters in hints | labels on every line |
| ok | 2 green | verified good | nothing to check, unmeasured |
| attention | 3 yellow, bold | the user should act or know | favorites, running, scheduled |
| fail | 1 red | failed, refused, blocked, disconnected | recording that is proceeding |
| live | 2 green with `● Playing`; 9 bright red with `● REC` | playing or recording now | health, failure |
| favorite | 12 bright blue with `★` | the favorite marker | any status |
| muted | 8 bright black | IDs, revisions, ages | the only copy of required text |
| selection | reverse video plus `>` | the focused row | color alone |
| map | coast, night and rim per the rules below | the globe canvas only | outside the canvas |

Color the status word, dot or key letter, not the whole line. Bold marks the focused section and the brand. `attention` is always bold, because yellow is weak on many light palettes. The globe rim uses `muted`, not ANSI 7.

State mapping: `running`, `playing` and `recording` use `live`. `scheduled` and `queued` use `text`. `paused` and `stale` use `attention`. An unmeasured check says `not measured yet` and is not prefixed `ok`. The doctor JSON `state` enum stays as stored.

These are the selected choices for issue 24: decision A option 1, decision B option 1, and decision T. Issue 24 implements the table. Linear mode keeps these roles unless monochrome or `NO_COLOR` is also set. High contrast is the accessibility mode: failure and live keep their color, and other emphasis uses the terminal's default foreground.

## Glyphs

ASCII first. Unicode is used where it carries data or a functional marker, each with an ASCII fallback under linear mode and monochrome. `TERM=dumb` refuses the explorer before a frame. No emoji and no icon font. Every glyph has a legend or a word next to it. No ambiguous-width glyph sits in an aligned column.

| Meaning | Marker | Fallback |
| --- | --- | --- |
| selected row | `>` plus reverse video | same |
| favorite | `★` in the favorite role | `*` |
| live now | `● Playing` / `● REC` | `(live)` / `REC` |
| retained audio | `█` | `#` |
| unavailable | `░` | `x` |
| gap | `╳` with a `gap` label | `!` |
| not yet published | `·` | `.` |
| globe | braille | linear text route |

Light box drawing (`─` and `│`) is for at most one rule between regions.

## Layout

| Size | Layout |
| --- | --- |
| under 80 columns, including 60 | compact: short tab labels, stacked labelled values, all seven workspaces reachable |
| 80x24 | one focused pane plus a persistent strip of at most 3 to 4 rows: header (connection and anything live), tabs, one hint line |
| around 120x40 | results beside detail |
| around 160x50 | results, geography and evidence together |

The composition target around 120x40 is the layout breakpoint. The 2026-09-21 terminal-stack probe requested 120x36 because that was a ConPTY measurement size in [Decision 0015](../decisions/0015-terminal-stack.md). That probe size is not a second breakpoint. Current production inspection uses 80x24 and 132x40.

Spare space goes to content, never to blank rows. Mode flags appear in the header only when non-default, and always in System. Quota stays in System. The header shows it only at attention level, such as low space or a near limit. That is the selected choice for issue 27, decision C option 1.

The hint line is `key action` pairs, two spaces apart, most useful first, ending in `? more`. `?` opens the full key overlay for the focused workspace, and Escape closes it. Drop pairs from the right. Never cut a pair mid-word. Status messages wrap or truncate in the middle. They never lose their actionable tail. A boundary sentence appears at most once on a screen.

CLI help wraps to the real width. CLI reports use one summary line, a fixed-width state column, grouped idle checks, and `Next:` last.

Issue 27 applies that layout. Below 80 columns, the recording hint says `select row` and the monitor hint says `inspect row`, so `? more` still fits at 20 columns. The workspace body still says arrows select and Enter inspects. Issue 26 applies help wrapping and the `?` overlay. The overlay lists the keys this workspace uses now. On the Globe canvas, arrows turn. Elsewhere, Left and Right change workspace. `h`, `j`, `k` and `l` remain globe aliases.

## States

| State | Reads like | Role |
| --- | --- | --- |
| Pending | `Fetching one directory page (up to 15 s)...` before the wait, then a final line | text |
| Empty | `No stations cached yet.` plus `Next:` | text |
| Unknown | `Cache size: not read yet` | muted |
| Partial | `Showing 16 of 10,000 cached stations` | attention only if it blocks the task |
| Stale | `Directory page from 11 days ago` plus `Next:` | attention |
| Unmeasured | `not measured yet` | muted |
| Blocked | what is held, why in plain words, `Next:` | fail |
| Disconnected | `Disconnected 10 s ago. Recordings keep running. r reconnects.` | fail |
| Live | `● Playing <name>`, `● REC <name>` | live |

Unknown is never shown as 0. Unmeasured is never prefixed `ok`. Every briefing, monitor and timeline states what it covered and what it missed in one line.

## Motion

Motion explains what changed, then stops. Rotation is off by default. A key step, center and explore glide to the target along the shorter longitude path in at most 400 ms with ease-out. Any key interrupts and lands on the target immediately. Where motion is off, the jump stays instant. Rotation ramps up and down over about 400 ms and keeps 6 degrees per second. These are the selected choices for issue 28, decisions E, F and G, each the recommended option.

A startup flourish, if shown, is the `Sigy` wordmark and the globe turning to the last viewed position. It settles within about 800 ms, plays at most once per session, never delays input, and any key skips it while that key is still delivered. A setting turns it off. The playing station may show `●` with a pulse of at most 1 Hz and at most 2 frames per second, frozen when reduced motion is on.

At most 15 frames per second. Zero redraw when idle. Nothing flashes more than 3 times a second. Never animate under `NO_COLOR` or `CI`, in linear mode, or with reduced motion. No spinner is ever the only signal. Each frame is wrapped in synchronized output where the terminal supports it. Terminals without that mode render as they do today. A pipe, a missing terminal, or `TERM=dumb` refuses `sigy tui` before any mode change. Normal quit, recoverable failure, panic, and an external stop restore the cursor, terminal modes, and main screen. A hard kill can skip that restore. An external stop exits 130. The pipe and `TERM=dumb` refusal exits 1.

On the Globe canvas, arrows turn the globe. Left and Right change longitude. Up and Down change latitude. Tab moves focus to the station list, where Up and Down select. Digits and Tab still switch workspaces. `h`, `j`, `k` and `l` stay aliases. No help line requires them. Header and help say `turn`.

Issue 28 applies this motion. On the Globe canvas, arrows turn. A turn eases for 400 ms when motion is allowed, and it stays instant when motion is off.

## Accessibility

Arrows do the obvious thing in the focused pane. Vim keys are aliases.

Linear, monochrome, reduced motion and high contrast are independent switches. Each has a flag and an environment variable: `SIGY_LINEAR`, `SIGY_MONOCHROME`, `SIGY_REDUCED_MOTION` and `SIGY_HIGH_CONTRAST`. Any non-empty value turns the switch on. Turning one on never forces another. `NO_COLOR` also selects monochrome. `--color=auto|always|never` overrides the environment: `never` wins over `FORCE_COLOR`, `always` works in a pipe, and `NO_COLOR` is still honored under `auto`.

High contrast renders every role in the default foreground, with bold for emphasis and reverse video for selection and focus. `fail` and `live` keep their ANSI slot, made bold, and always sit next to their word or glyph. High contrast is an accessibility mode, not a Sigy palette.

Focus is reverse video plus `>`, never color alone. Test drawn frames under `NO_COLOR=1`, 60x20, 80x24 and 132x40, one light palette and one dark palette, linear mode read top to bottom, and CJK and RTL names. A pipe and `TERM=dumb` are refusal checks: one plain line, `Next:`, and no escape sequence. The gallery may compute contrast from fixture hex values. Those hex values stay in the gallery. Product code does not store a theme.

Without a terminal on stdin and stdout, `sigy tui` prints one line saying the explorer needs an interactive terminal, plus `Next:` with a non-interactive command, exits non-zero, and emits no escape sequence. Under `TERM=dumb`, `sigy tui` refuses the same way. That is the selected choice for issue 31, decision H option 1. A failed terminal setup leaves the terminal as it was found.

## Agent and JSON parity

Every human line comes from a JSON field. If a line has no field behind it, add the field first. Next steps live in data. CLI, TUI and MCP render them from one source. `--json` writes one document to stdout. Errors go to stderr as JSON with `error`, a stable `code` and, when known, `next`. Human text and JSON come from the same value. Human labels never become JSON keys. Translations never change IDs.

MCP descriptions keep the `Verb object. Boundary.` pattern, with one fixed phrasing for the boundary. MCP failure results keep their existing structured content and gain the same `code`.

Exit codes, selected for issue 30:

| Code | Meaning |
| --- | --- |
| 0 | the command completed |
| 1 | failure that is not one of the codes below |
| 2 | usage, including clap |
| 3 | the background service is unavailable |
| 4 | the service refused the job |
| 5 | a partial result |
| 130 | the command was interrupted |

A failed, refused or partial run is never exit 0. A broken pipe while writing output may still exit 0, because the reader went away. A command exits 0 when it completes, 2 for usage, 3 when the background service is unavailable, 4 when the service refuses the job, and 5 when the returned task result is partial. Other failures exit 1. An external stop exits 130. JSON errors use `error`, `code`, and `next` when a next command is known. Long-wait terminal progress indicators are not emitted.

## Proposed, not implemented

Configurable key bindings, with conflict validation and reset, remain proposed. The current keys are fixed. A searchable action palette remains proposed. The contextual footer and help are the current way to discover actions.

Shell completions are not added. A typo suggestion uses a hand-written edit distance of 1 or 2 over the command names clap already declares. Sigy prints `Next:` only when one name is uniquely that close, then clap's usage exits 2. No completion crate is added.

## Review checklist

- The same fact has one phrasing across CLI, TUI, panel and README.
- No action has two names, and no key has two meanings in one workspace.
- No good news is painted as a warning, and no unknown is painted as ok.
- No line hides its actionable tail at the terminal width.
- No internal ID, revision or mode flag sits on a primary line.
- No boundary sentence appears more than once on a screen.
- Next steps are full commands, identical for identical situations.
- No emoji, gradients, RGB, neon or decorative borders.
- Every color comes from the one role table and reads on a light palette and a dark palette.
- Monochrome and linear frames say everything the color frame says.
