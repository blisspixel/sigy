# 0015: Termina is the terminal backend

Date: 2026-09-21. Status: selected for the later list explorer. No terminal crate is in the application. The client still opens no second catalog.

## Decision

When the list explorer is added, use Ratatui 0.30.2 with one backend: Termina 0.3.3. Do not also depend on Crossterm. This operation does not add that crate.

## Measurement

The host was Windows x86_64. The driver ran inside Windows Terminal 1.24.11911.0. Rust was 1.98.1. The probe is the excluded package [research/experiments/terminal](../../research/experiments/terminal/README.md). It draws one local list and a search field. It does not open the catalog, a source URL, or a network connection. The resolved crates are Ratatui 0.30.2, ratatui-termina 0.1.0, Termina 0.3.3, and Crossterm 0.29.0. Termina 0.4.0 was current on crates.io, but ratatui-termina 0.1.0 requires `termina ^0.3`, so 0.4.0 was not the backend under test.

Each backend ran the same workload on two hosts. ConPTY, the console host Windows Terminal uses, started at 80x24 and was resized to 120x36 and 200x50. Separate Windows Terminal windows were requested at 80x24, 120x36, and 200x50. The acceptance run set `NO_COLOR` and reduced motion. Search was the six keys in `navajo`. Paste was the bracketed sequence for `q` plus the two wide characters in `東京`. A separate run allowed color. Another turned reduced motion off. Clean exit and panic both restored console modes.

Both backends met these checks:

- Idle draws were 0, and ConPTY output during that window was 0 bytes. Reduced motion was on.
- With reduced motion off, the same probe drew 7 animation frames in 400 ms.
- The combining acute in `Cafe` plus U+0301 stayed in one cell.
- `東` occupied one cell, the next cell was the reset spacer (`Cell::symbol` reports that as a space), and `京` began two columns later.
- The `NO_COLOR` run emitted no color SGR. The color run did.
- After clean exit and after panic, input mode was still 503, output mode was still 7, and both code pages were still 437.
- ConPTY resize events reported 120x36 and 200x50. The Windows Terminal window reported 80x24 for that request. It clamped 120x36 to 115x36 and 200x50 to 115x37. Both backends reported those same sizes. `GetCurrentConsoleFontEx` reported Consolas. The Windows Terminal settings profile has no font override.
- Release key-to-frame time is from the event read to the draw flush. It is not glass-to-glass, and the sample is 18 search keys. On ConPTY the Termina samples were 123 to 315 microseconds. Nearest-rank p95 is 315 microseconds. Inside Windows Terminal the Termina samples were 76 to 1071 microseconds. Nearest-rank p95 is 1071 microseconds.

Termina treated the bracketed payload as one paste event whose text was `q東京`. The probe did not quit. The query contained that text. This happened on ConPTY, in all three repeats, and inside Windows Terminal at each requested size.

Crossterm 0.29.0 recorded no paste event. On ConPTY, all three repeats finished search, both resizes, and mode restoration, and the driver then timed out waiting for paste. The report has no paste record. Inside Windows Terminal, at each requested size, the query became `navajo[20~` and the report still has no paste event. Search, resize, idle, the Unicode cells, color, reduced motion, and mode restoration otherwise matched.

## Limits

The list explorer is not rendered. The globe, capture under a slow terminal, screen readers, and other operating systems were not measured. A hard kill still cannot restore modes. Operation 9 still has to build the explorer on this backend and inspect that client in Windows Terminal.
