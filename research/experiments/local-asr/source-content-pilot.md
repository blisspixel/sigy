# Pinned CPU source content pilot

Reviewed: 2026-09-23. Five files from whisper.cpp commit
`927cfce34f31707e17f2bff35c349632fb9e2c3a` were acquired through the
bounded route described in the [HEAD pilot](source-head-pilot.md). Each GET
published its exact declared length and passed an independent local SHA-256
check. The cumulative language-evaluation ledger now contains 17 charges and
12,349,608,954 policy bytes, including the explicitly unmeasured historical
10,737,418,240-byte charge. Paid spend remains USD 0. The original 12 ledger
rows and 73 prior nonjournal files were unchanged. These are source-review
inputs, not proof of binary provenance or permission to run native code.

| Pinned file | Bytes | Local SHA-256 |
| --- | ---: | --- |
| [Release workflow](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/.github/workflows/release.yml) | 40,031 | `5f93de57e9fc6b08364147b68dff0ff5e57a86d0057b96f5a318a353ce99d897` |
| [CLI source](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/cli.cpp) | 64,027 | `840f331f80a98c41fc21eb4cf109c4c6a5496b8f248e9bbce58dd733dece76b2` |
| [CLI build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/CMakeLists.txt) | 228 | `ea772eceff30b24f9d4fe96972d58d1f34e735152dfb4b52e0456bc23662a746` |
| [Examples build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/CMakeLists.txt) | 3,688 | `982075571238fee6d010c72acf3df2d6552bfa099058996c88248b89141613af` |
| [Backend registry](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/ggml/src/ggml-backend-reg.cpp) | 18,555 | `0f63c69ad083e0744872f57d59d049b069283303f5bbc646222907d779afe4d1` |

The Windows x64 release job requests shared libraries, dynamic backend loading,
all CPU variants and a non-native build. It archives the full Release directory.
Its Linux branch copies a license into its package; the reviewed Windows branch
has no corresponding copy step. This agrees with the read-only inventory of
the retained 40-member ZIP, which found no separate notice file. The CLI target
links the common and whisper targets, while SDL2 belongs to another example
target. Packaging SDL2 in the ZIP alone does not establish that the CLI needs
it. The source does not prove binary equivalence or complete third-party notice
closure.

The CLI calls `ggml_backend_load_all()` before parsing `--help`, `--no-gpu`,
or `--no-prints`. The registry searches the executable directory and current
directory, may search a compiled backend directory, and honors
`GGML_BACKEND_PATH`. It loads matching backend variants before scoring them.
Even a help probe therefore needs a fixed, hashed stage, a safe working
directory, a sanitized environment and an enforceable process boundary. The
Windows loader implementation is in an additional source file not yet retained.

Exit code zero alone does not prove recognition. The CLI can return zero for
argument and language errors, and can continue after an audio-read failure.
On Windows, `--output-file -` opens `CON`; redirected stdout cannot be assumed
to receive JSON. A first contained pilot would capture bounded ordinary output
and require a validated result and exact input coverage. Timestamp and token
semantics require separate validation before publishing aligned cues. No model
or native member has run.

The next bounded source candidates at this same revision are
`ggml/src/ggml-backend-dl.h`, `examples/common-whisper.cpp`, and the root
`CMakeLists.txt`. Their lengths and hashes are not yet known. Each needs an
independent bounded HEAD and a separately reserved exact GET before review.
The first resolves Windows DLL loading, the second the common audio reader and
incorporated code, and the third release build defaults. Further files and
their notices follow only when those sources establish a need. Native process
cleanup, resource and output caps, legal notice closure, and an end-to-end
three-file recognition pilot remain open.
