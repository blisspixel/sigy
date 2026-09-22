# Terminal stack measurement

Run: 2026-09-21 on Windows x86_64, inside Windows Terminal 1.24.11911.0, with Rust 1.98.1. This package is excluded from the Sigy workspace. It is a research artifact, not an application dependency. It does not open the catalog or a network connection.

The probe draws one local list and search field twice: once on Crossterm 0.29.0 and once on Termina 0.3.3, both through Ratatui 0.30.2. ConPTY supplies scripted resize and bracketed paste. Separate Windows Terminal windows cover 80x24, 120x36, and a requested 200x50 size. The recorded result is [2026-09-21.json](results/2026-09-21.json). The selection is [decision 0015](../../../docs/decisions/0015-terminal-stack.md).

From the repository root, set `CARGO_TARGET_DIR` to `target/terminal-measure` so the build stays under the ignored target directory, then run:

```text
cargo run --release --manifest-path research/experiments/terminal/Cargo.toml -- --out research/experiments/terminal/results/2026-09-21.json
```

`--only conpty` or `--only hosted` repeats one host. The command exits 0 after writing the report. Individual candidate checks can fail; the JSON `pass` field records that.

`font REPORT` writes the current console font face and whether `WT_SESSION` is set. The list explorer inspection on the same day is [2026-09-21-explorer.json](results/2026-09-21-explorer.json).
