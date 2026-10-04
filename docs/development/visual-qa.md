# Production terminal visual QA

The reusable offline gallery runs the actual explorer renderer through Ratatui's
TestBackend. It does not open a library, contact a station or service, start audio,
or use a device. Every observation is a synthetic fixture, including recording
timelines and linked registrations. These pictures establish presentation
evidence, not source availability, native terminal fidelity or audio support.

From the repository root:

```text
cargo visual-qa
```

The command selects one explicitly ignored renderer test. It creates a unique
directory under `target/visual-qa/` and prints the path to `index.html`. Existing
artifacts are preserved. Open that index locally to review the grouped SVG
screenshots. `manifest.json` records each case, dimensions, plain-mode flags and
hashes of the compiled renderer, state, monitor, context and gallery sources;
each adjacent JSON file preserves every terminal cell's symbol, foreground,
background, modifiers, display width and wide-character continuation flag.

The bounded matrix contains 64 frames at 20 by 8, 80 by 24 and 132 by 40 cells.
It covers Explore, Live, Recordings, Monitors, Findings, System and Globe; filter
and country editors; frozen station context; linked pending, loaded and failed
reads; empty and unobserved cache; disconnected state; pending search; changed
catalog; monochrome and linear presentation. Podcast and job controls remain
CLI operations and have no dedicated explorer workspace to screenshot.

Review the actual user journey, not only whether a label exists. Check selected
identity, applied versus draft scope, bounded unknown and partial statements,
navigation hints, reachable content after scrolling, deliberate focus, original
scripts and connection state at each size. Revisit the same cases after repairs.
At the smallest size, inspect pending, loaded and failed linked reads separately;
the catalog-change warning must remain visible alongside the current read phase.
Independent ordinary tests verify intent is drawn before a synchronous read,
outcome redraw, clock freshness without observation rewrites, unknown versus
observed-empty cache and reachable wrapped monitor passages.

Monitor passages are bounded previews. A cut is labeled explicitly, and wrapping
and clipping preserve complete graphemes. The underlying original cue remains
separate from this display; the monitor view does not claim to display an entire
passage when its preview limit is reached.

SVG uses an explicit terminal palette, escaped text, wide-cell positions,
inverted selection and underline. RGB and indexed colors are supported. Font
fallback and shaping depend on the viewer; inspect the cell JSON when a glyph
looks wrong, and retain a native-terminal check for actual backend qualification.
Blink timing and native font rendering are not simulated. The existing private
cell raster can consume the JSON for separate PNG inspection.

The terminal still awaits each service read synchronously. Painting pending
intent before that read improves feedback but does not make quit, resize or
navigation responsive while a request is blocked. The view clock advances on
coarse idle ticks and fresh reloads; it grants no work and changes no immutable
observation timestamp.
