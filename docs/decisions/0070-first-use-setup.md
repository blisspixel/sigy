# First-use setup

Date: 2026-10-01. Status: implemented and locally verified; verification outcomes are recorded in [active work](../development/progress.md). Exact-commit hosted CI is required before source-only publication. Catalog schema and local IPC remain v43. This does not qualify a platform or exit a release stage.

## Decision

`sigy init` prepares the local library and starts or reconnects to its existing service. The default library is `USERPROFILE/.sigy/library` on Windows and `HOME/.sigy/library` on Unix. The home path must be nonempty, absolute and below the filesystem root; failure tells the user to supply `--data-dir`. An explicit path always wins and can appear before or after a subcommand. If supplied in both positions, the subcommand value wins through the existing argument parser. No active-library pointer, new configuration store, schema or dependency is introduced.

Library commands share this default. Reading a missing library does not initialize it and tells the user to run `sigy init`. Help, version, updates, backup verification and restore do not resolve a default library. MCP continues to require an explicit `--data-dir`, preserving its startup-selected library boundary. The lower-level `library init` remains available.

Setup reuses canonical library ownership, catalog initialization and service startup. It does not replace an existing library or reset budgets, profiles, recordings, retention, schedules or policy. A new library has paid processing disabled. A live service is inspected through existing local IPC; setup does not open a second catalog writer. Starting an existing service can resume previously authorized work and network policies.

`--no-start` prepares storage without starting a process. It does not stop a service that is already running. `--radio` conflicts with this flag and explicitly admits one public Radio Browser page with a 100-row limit through the existing acquirer and supervisor. It grants no station playback, capture, click, inference or recurring refresh.

The first-use refresh has the fixed request ID `sigy-init-radio-v1`. Exact replay reuses its stored state and cannot fetch twice. After service readiness, setup waits for the refresh for at most 15 seconds, reports completion, failure, interruption or pending work honestly, and provides the status command when work remains. A failed refresh leaves setup storage and the service available; a separate `radio refresh NEW_ID` requests another fetch. Setup never submits another refresh automatically or expands a budget. The existing directory adapter retains its bounded mirror fallback.

## Interface and evidence

Human output identifies the library, service state and next steps. JSON emits one structured setup response. There are no interactive questions, inferred location, decoder downloads, model downloads, analytics or diagnostic uploads. FFmpeg configuration and the planned jurisdiction setup retain their separate contracts.

Verification exercises isolated child home environments, explicit path precedence, paths with spaces and original scripts, missing or relative home paths, missing catalogs, repeat setup, service ownership, exact budget and retention preservation, flag conflicts, structured failures and unchanged MCP selection. Directory fixtures must remain bounded and local. Real internet refresh success is a separate operational observation, not established by these fixtures.

The README leads with the product description and a current interface render. Detailed setup and capabilities stay in [installation](../install.md), [usage](../usage.md) and active work. Interface renders use current production drawing over retained real station metadata; they do not establish live coverage or terminal latency.
