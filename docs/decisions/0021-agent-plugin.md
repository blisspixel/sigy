# 0021: Agent plugin

Date: 2026-09-22. Status: implemented; local validation evidence belongs in [progress](../development/progress.md).

`sigy mcp` speaks [MCP 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28) on stdio. Each request carries its own protocol metadata. `server/discover` advertises that version, the tools capability, and server identity. `initialize` remains available for a client that still uses the 2025-11-25 handshake. An unsupported version returns JSON-RPC `-32022` and lists `2026-07-28`.

The package at `agent-plugin/` is an [Agent Plugins 1.0.0](https://agent-plugins.org/specification) directory: `plugin.json`, `mcp.json`, and one Agent Skill. The stdio command is the `sigy` executable. Its library is `--data-dir`, defaulting in the package to `${PLUGIN_DATA}/library`. A client that does not search `PATH` must set `command` to the installed executable. The server does not embed credentials.

Tools are the existing commands. A call runs `sigy --data-dir LIBRARY --json` with a fixed argument list. The model cannot pass a shell command, another library path, a budget change, or a recording deletion. Subscribe, refresh, download, and record start still use the service rules: no DNS on subscribe, no enclosure fetch on refresh, a 512 MiB and 30 minute reservation before an enclosure connect, and radio attempts inside 15 minutes and 256 MiB. `listen_file` requires a destination and stops after 120 seconds, leaving the recording in place. The process allows 60 tool calls in 60 seconds.

This surface does not add a catalog, an HTTP stack, or a paid provider. It does not exit a roadmap stage. `podcast_text` fetches one publisher document when asked. See [publisher text](0022-publisher-text.md).

Amended 2026-09-24: analysis tools let an agent pin and publish a recording, queue local recognition and translation with profiles the user already configured, inspect and cancel jobs, and read transcripts and translations. No tool configures a recognition or translation profile, names an executable or model path, or enables a paid provider. A job id is an idempotency key, so a repeated call never reruns work.
