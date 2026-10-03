# HackRF Pro Windows identity experiment

Observed: 2026-10-03. Status: one bounded private device-identity experiment, separate from Sigy's service and planned receiver adapters. It establishes native utility loading and identity-read interoperability on this Windows host. It qualifies no RF reception, transmission silence, throughput, decoder, deployment or supported hardware profile. External spend was USD 0 under the existing cumulative USD 20 ceiling.

## Device and result

The user connected a HackRF Pro to the current Windows machine. OS inspection found one present board in USB family `1D50:6089`, named HackRF Pro, with status OK, service WINUSB and Microsoft driver `10.0.26100.9444` through `winusb.inf`. That shared USB family ID alone does not distinguish Pro from One. The subsequent native identity operation reported:

| Field | Observed value |
| --- | --- |
| Board | `5 (HackRF Pro)` |
| Hardware revision | `r1.2` |
| Firmware | `2026.01.3` |
| USB API, as printed by the utility | `1.10` |
| Host utility | `git-7f96cc8` |
| Host library | `git-7f96cc8 (0.10.0)` |
| Process outcome | Exit 0; child wait observed; actual empty Job Object snapshot |
| Supervised execution interval | 64 ms, excluding asset preparation |
| Output | 505 stdout bytes; zero stderr bytes |
| Job peak committed memory | 2,637,824 bytes |
| Job total CPU time | 31,250 microseconds |

Exactly one board was reported. Neither output reported a diagnostic failure, self-test failure or close error. This is the observed output, not an independent hardware self-test or RF measurement. OS inspection after the run still found one present healthy HackRF-family board. Serial and part identifiers remain private. No driver installation, firmware write, physical modification or RF acquisition was performed.

## Pinned tooling and supervision

[Upstream Windows installation guidance](https://hackrf.readthedocs.io/en/latest/installing_hackrf_software.html), reviewed 2026-10-03, offers Windows Actions artifacts as an alternative to a broader installation. The experiment retained official artifact `10969359093`, `hackrf-tools-windows`, from successful [run36422064815](https://github.com/greatscottgadgets/hackrf/actions/runs/36422064815), tied to source commit `7f96cc8e3fa625c4263a71ba8dd44d1f6110e4fa`. This is a main-branch build, distinct from [release2026.01.3](https://github.com/greatscottgadgets/hackrf/releases/tag/v2026.01.3). It is provisional experiment tooling, not a selected product dependency.

The ZIP hash is `71d24f859d8164af33fb97a88a88b6a06da5570d1667c1292ca0e41782a26671`: 679,768 compressed bytes, 15 entries and 1,524,812 expanded bytes. Extraction checked canonical confinement, duplicates, links and streamed sizes against separate compressed, expanded, entry-count and file limits. PE inspection found unsigned x64 binaries; source review and retained hashes do not prove source-to-binary reproducibility. The minimum identity runtime contains only these four checked files:

| File | SHA-256 |
| --- | --- |
| `hackrf_info.exe` | `24f61f041d56358b5c11c08ab63e0c23ebd45f0a5a45dc6aeb4f169a5ff1097d` |
| `hackrf.dll` | `144a341e59dd7a428669a31091f4d856a634c86ce6600de093a3849cdca95753` |
| `libusb-1.0.dll` | `c33bfdc9e41d958248648b09cd697022a236da881c72c2a639e376b91b2aa4d3` |
| `pthreadVC3.dll` | `e0014c9cef03ba55d90a3fce4ba62be274a98f86e8273d8a57b67cc55ab25a4d` |

Declared dependencies also use existing Windows KERNEL32, Visual C runtime and UCRT components. FFTW is unnecessary for this identity operation. PE imports do not enumerate every dynamically loaded component. The original archive and source legal headers remain private and unchanged; the artifact includes no standalone notices. Resolve matching dependency notices and distribution compatibility before any bundling. No native utilities or DLLs are distributed with Sigy.

A private Rust 1.98.1 helper uses the existing pinned `processkit` 3.3.4, `tokio` 1.53.1, `sha2` 0.11.0 and `serde_json` 1.0.151 in a separate workspace. It accepts only fixed modes, never an arbitrary command. Native mode runs the exact identity utility without options. It hashes its own binary before dispatch with a separate 32 MiB ceiling, rechecks each native file against a 4 MiB ceiling before and after staging, and holds Windows read handles denying writes/deletion through completion and receipt publication. The fresh runtime contains exactly the four listed files. Environment is cleared; its PATH contains that runtime and System32, its working directory is private, stdin is closed and no window is shown.

The Job Object requests one process, 128 MiB committed memory and a rounded one-core CPU quota. A 15-second post-spawn execution deadline covers pipe reads and child wait. Each output stream is capped at 32 KiB before growth, for at most 64 KiB aggregate raw output. Timeout, overflow or a pipe error requests whole-group termination, with a three-second child wait and a separate three-second search for an actual empty-group snapshot. Unknown completion remains explicit. These bounds do not force blocked synchronous filesystem or OS calls to return, isolate the network, prove creation-time containment or account for kernel/driver resources.

Formatting, warnings-denied Clippy/build and three private tests passed. The final helper's success, overflow and timeout cases each observed child wait and an empty Job Object. Their execution intervals were 16, 14 and 515 ms. Overflow retained 32,768 bytes on each pipe, exactly the 65,536-byte aggregate ceiling. An additional buffer check exercised exactly-full and one-byte-over input; rejected arbitrary targets and extra arguments admitted no native work. These controlled fixtures qualify the tested helper behavior only.

## Device-state distinction

[The inspected identity source](https://github.com/greatscottgadgets/hackrf/blob/7f96cc8e3fa625c4263a71ba8dd44d1f6110e4fa/host/hackrf-tools/src/hackrf_info.c) requests identity and diagnostic reads after USB interface setup. [The library](https://github.com/greatscottgadgets/hackrf/blob/7f96cc8e3fa625c4263a71ba8dd44d1f6110e4fa/host/libhackrf/src/hackrf.c) can set USB configuration and host RAW_IO behavior during setup, and normal close requests transceiver OFF. The inspected call path requests no RX/TX start, IQ bulk submission, frequency/gain/clock/antenna setting or firmware write. This operation is therefore identity inspection with a normal-close OFF request, not state-read-only hardware access.

The utility can print self-test or close failures while still exiting successfully. Earlier failures can return before normal close. Process exit and group drain cannot prove successful device-state cleanup or RF silence; both output streams were inspected separately for this run. Independent emissions instrumentation, actual receive/stop behavior, unplug/reconnect, ownership, slow consumers and sustained resource use remain separate [receive-only qualification gates](../../docs/design/recording-metadata.md#receive-only-source-qualification).

## Retained evidence and next step

Private evidence resides in `.agents/hackrf-pro-qualification/20261003/`: the original archive, bounded extraction receipt, PE graph, pinned source/diffs, helper workspace and final-binary self-check receipts. Native receipt `native-identity-1791063906121764400/identity.json` has SHA-256 `458305023ab416afb055b90c55966e44d2cd44586bfb2efe14f5497797924d81`. Helper SHA-256 is `d98708ebdf6ede9a9216c8cc1c1821d6e8f6df8be7bb597237a52ccdb212159f`. Raw output remains private; the redacted summary contains only the fields above.

The useful next receiver increment is still typed bounded IQ-file replay with exact sample format, rate, tuning epochs, clock uncertainty and derivative ancestry. A subsequent actual receive profile needs its own finite sample/byte/storage contract and measured stop, disconnect and RF behavior. No fake recognition job or separate service scheduler should be used to obtain device authority. Windows identity now has local evidence; moving to Linux is unnecessary for this check and would begin a separate profile. The main product priority remains [focused listening context and protected passage playback](../../docs/development/near-term-implementation.md#ex-02a-focused-listening-context).
