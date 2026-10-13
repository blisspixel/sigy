# 0095: MCP evidence output schemas

Date: 2026-10-13

Status: implemented contract increment; integrated verification is recorded in [active work](../development/progress.md).

## Decision

`tools/list` declares an `outputSchema` for `doctor`, `analysis_search`, `monitor_matches`, `monitor_coverage`, `analysis_transcript` and `analysis_translation`. Each schema is a self-contained JSON Schema 2020-12 document embedded from [the schema directory](../schemas/). References resolve only within that document. Other tools do not advertise this contract.

The success branch describes the complete existing CLI JSON snapshot. It retains typed evidence identifiers, revisions, cue bounds, original scripts, translation state, pagination, coverage and uncertainty fields. Nullable projections remain explicit. Transcript and translation absence use their existing `empty` discriminator. Money remains an exact decimal string. The schema validates representation; it cannot establish semantic truth, language quality or current media availability.

The alternative is the existing CLI error envelope, `{ "error": "message" }`. CLI failures without JSON stdout preserve that stderr envelope in MCP `structuredContent`. Argument, process-start and deadline failures synthesize the same shape. A failed doctor check still returns its complete JSON report with MCP `isError: true`. Existing JSON stdout is preserved verbatim. A declared tool returning no JSON on success becomes an explicit tool error. Text remains available for clients that do not consume structured results.

No CLI result, catalog, IPC version, service authority, destination, budget or operation is added. The adapter still invokes the same fixed arguments against its startup library and observes the existing process deadline and output bounds.

## Selection and verification

Reviewed 2026-10-13: the [MCP tools specification](https://modelcontextprotocol.io/specification/2026-07-28/server/tools) defines the output-schema and structured-content contract. Checked-in schemas keep that public adapter contract visible without adding schema derives throughout service types. A generation framework would couple unrelated service representations to the protocol; a handwritten validator would duplicate a complex standard. The tests instead use the maintained Rust [jsonschema crate](https://docs.rs/jsonschema/0.58.6/jsonschema/) pinned to 0.58.6, exclusively as a development dependency.

Default features are disabled, including HTTP and file reference resolution. All schema references are local. The lock adds 38 packages, covering validation, regular expressions, exact numeric comparisons and their support crates. The selected crate requires Rust 1.85, below the pinned Rust 1.98.1 toolchain. The added graph offers permissive MIT, Apache-2.0, BSD-2-Clause, MIT-0 or Unlicense terms, with the applicable alternatives and notices retained in registry packages. It adds no product runtime dependency or native library. Locked dependency advisory checks remain part of `cargo verify`.

Offline fixtures discover schemas through the actual MCP process and validate structured results against those schemas, compare them with CLI JSON, and reject representative missing or mistyped evidence fields. Changes to the CLI snapshot must deliberately update the declared contract and fixtures. Full local gates and exact-commit hosted verification remain required before integration or source publication. This increment establishes no platform, language or release-stage qualification.
