# Classification off and monitor inspection

Date: 2026-09-30. Status: implemented and locally tested. This is the narrow classification-off gate in roadmap operation 35 and a read-only terminal view of the existing monitor operations. It does not exit stage 6.

## Decision

Classification remains off. The catalog accepts only `off` on a stored briefing. A proposal to enable classification is refused through the existing monitor policy and cannot reserve money, queue analysis or publish a relationship. Pausing a monitor stops future processing admission while keeping its transcripts, coverage, stored findings and briefing history readable. A user can publish a new coverage-first briefing while that monitor is paused. Repetition groups and unresolved relationships retain their existing meaning.

The terminal explorer's Monitors workspace now reads existing monitor operations. Press `4` to list monitor identities, select with the arrows, and press Enter to read the selected monitor's current version, coverage and literal passages from captures started in the last 24 hours. In the detail view, arrows scroll, Escape returns to the list, and `r` reloads. A list reload preserves a still-present selection. Opening and selecting never admit processing or capture. This client adds no monitor mutation.

Coverage lists each source's captures, published audio, gaps, pins, transcripts with and without text, and translated and untranslated cue counts separately. Saved schedule counts remain separate. Matching passages retain original script, English text or its absence, recording identity, transcript and translation revisions, and media times. They are explicitly labeled as passages rather than stored findings. No passage is promoted to a finding by this view. The Findings workspace directs users to the implemented named-finding CLI read because bounded finding enumeration has not been implemented.

Reads use the existing local catalog owner or IPC connection. The monitor list retains at most the existing 256 identities; match reads use the existing service response limit. Detail pages are applied together only when their monitor identities, policy versions and windows agree. A refusal or disconnect keeps the previous detail, and the connection message identifies the saved snapshot. The reads are sequential and are not an atomic database snapshot across processing completions. All untrusted text is sanitized before display. Wording remains uncertain, classification is off, and result truncation is visible.

## Verification

New browser fixtures check bounded navigation, selection preservation, scrolling, separate coverage stages, Arabic text, terminal-control sanitization and rejection of mismatched policy versions without replacing the previous detail. Command-effect fixtures reject mutations and permit only the implemented monitor reads. Render fixtures inspect 80 by 24 and 132 by 40 frames, including summary, coverage, passages and disconnection. Their artifacts can be written under ignored `.agents/` with `SIGY_TUI_SNAPSHOT_DIR` during the explicit fixture run.

The classification fixture attempts non-off SQL inserts, submits an enabling proposal, pauses the monitor, reads existing evidence, publishes another off briefing, and reopens the catalog. It compares analysis jobs, translation jobs, provider attempts, requests and ledger events before and after. Synthetic fixtures establish behavior and policy enforcement, not recognition, translation or classification quality.

## Remaining work

Monitor-driven capture scheduling remains open. Existing saved schedules have independent capture authority, monitor audio caps currently bound processing, and monitor pause does not stop those captures. This increment does not reinterpret those permissions. A future measured classifier profile needs its own bounded contract, allowance, abstention and pause behavior before it can be enabled. No classifier is installed or run here.
