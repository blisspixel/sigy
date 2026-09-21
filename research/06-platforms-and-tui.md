# Platforms, persistent services, and terminal experience

Reviewed: 2026-09-20. Status: primary-source research; platform acceptance testing remains future work.

## Service behavior

Windows services run outside the interactive user session and cannot directly interact with the user in the normal desktop model. A separate client communicating with the service is the documented pattern. This supports separating collection from the user interface and playback lifecycle. [Windows service guidance](https://learn.microsoft.com/en-us/windows/win32/services/interactive-services).

Apple distinguishes system launch daemons from per-user launch agents. The archived official guide explicitly associates agents with a logged-in user and termination at logout. Surviving terminal closure and surviving logout are different requirements. Current packaging behavior still needs testing on chosen supported macOS versions. [Launch agents and daemons](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html).

systemd provides service supervision on many Linux installations, including lifecycle and restart controls. Sigy should support a supervised foreground process independently so the core does not assume one init system. [Project documentation](https://systemd.io/), [service manual source](https://github.com/systemd/systemd/blob/main/man/systemd.service.xml).

## Terminal findings

Windows documents virtual-terminal control sequences and their console behavior. Terminal capability and input behavior still vary across hosts and sessions. [Console terminal sequences](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences).

Unicode text segmentation and East Asian width provide relevant rules, but width properties alone do not guarantee identical rendering in every terminal. A global-radio application must test combining characters, wide characters, truncation, selection, and right-to-left material on its actual terminal matrix. [Text segmentation](https://unicode.org/reports/tr29/), [East Asian width](https://unicode.org/reports/tr11/).

Rust has Ratatui and Crossterm as terminal candidates; Go has Bubble Tea and its ecosystem. Each provides an approach for interactive terminal applications. Library demos and marketing do not establish Sigy's usability or performance. [Ratatui](https://ratatui.rs/), [Crossterm](https://github.com/crossterm-rs/crossterm), [Bubble Tea](https://github.com/charmbracelet/bubbletea).

## Proposed experience standard

Use predictable navigation, progressive detail, strong typography through spacing and hierarchy, restrained color, and explicit health states. Geographic exploration should complement a fast keyboard-accessible list. Decorative animation must have a measurable purpose and an off switch.

Separate selection, focus, audible playback, and recording state. Keep status visible without intrusive repeated notifications. Search cancellation and stale-result protection must be designed before choosing the widget implementation.

Provide a focused small-terminal mode, plain CLI output for assistive use and automation, searchable commands, and terminal-state restoration after normal exit or recoverable errors. A rendered screenshot alone does not prove keyboard accessibility.

The globe is part of the planned TUI, with flat day/night map and list alternatives. [Terminal explorer research](18-terminal-explorer-and-radio-dvr.md) defines geometry, truthful location/activity display, rendering tiers, and the DVR/CLI parity experiments needed before selecting libraries.

## Packaging options

| Strategy | Strength | Validation burden |
| --- | --- | --- |
| Self-contained application plus managed worker assets | Predictable versions and onboarding | Signed updates, asset size, legal notices, architecture-specific builds |
| Application using installed system tools | Lower packaged size and user control | Version drift, missing features, path and configuration differences |
| Hybrid with verified optional bundles | Flexible installation profiles | Compatibility rules and diagnostics must remain understandable |

No installer or distribution channel is selected. Plan checksummed releases, dependency inventories, model asset verification, stable data locations, migration recovery, and removal of service integration without deleting the library by default.

## Platform matrix to establish

Native Windows, macOS, and Linux are confirmed targets. Exact OS versions, architectures, terminals, package formats, and accelerator profiles remain open. A proposed starting matrix considers Windows x64, macOS arm64, Linux x64, and Linux arm64 for small hosts; Intel macOS and Windows arm64 need explicit support decisions.

WSL is not a substitute for native Windows acceptance. SSH can host a TUI, but speaker playback on a different client machine requires a separate media-transport feature; terminal forwarding alone does not provide it.

## Required experiments and near future

After planning, test install/start/stop/uninstall, client closure, logout, reboot, sleep/resume, multiple clients, permissions, Unicode paths, and recovery from failed updates. Exercise rapid resize, large transcripts, network delay, raw-mode cleanup, keybindings, redirected output, and screen-reader workflows.

Evaluate terminal libraries by these cases before selection. Recheck OS signing/notarization and service-installation requirements at packaging time, rather than assuming today's installation commands remain the permanent distribution contract.
