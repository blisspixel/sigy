# Pinned CPU source content pilot

Reviewed: 2026-09-23. Eight files from whisper.cpp commit
`927cfce34f31707e17f2bff35c349632fb9e2c3a` were acquired through the
bounded route described in the [HEAD pilot](source-head-pilot.md). Each GET
published its exact declared length and passed an independent local SHA-256
check. The cumulative language-evaluation ledger now contains 20 charges and
12,349,630,658 policy bytes, including the explicitly unmeasured historical
10,737,418,240-byte charge. Paid spend remains USD 0. The original 12 ledger
rows and 73 prior nonjournal files were unchanged by the first five GETs. The
later three preserved the first 17 journal rows and 108 prior nonjournal files.
These are source-review inputs, not proof of binary provenance or permission to
run native code.

| Pinned file | Bytes | Local SHA-256 |
| --- | ---: | --- |
| [Release workflow](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/.github/workflows/release.yml) | 40,031 | `5f93de57e9fc6b08364147b68dff0ff5e57a86d0057b96f5a318a353ce99d897` |
| [CLI source](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/cli.cpp) | 64,027 | `840f331f80a98c41fc21eb4cf109c4c6a5496b8f248e9bbce58dd733dece76b2` |
| [CLI build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/CMakeLists.txt) | 228 | `ea772eceff30b24f9d4fe96972d58d1f34e735152dfb4b52e0456bc23662a746` |
| [Examples build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/CMakeLists.txt) | 3,688 | `982075571238fee6d010c72acf3df2d6552bfa099058996c88248b89141613af` |
| [Backend registry](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/ggml/src/ggml-backend-reg.cpp) | 18,555 | `0f63c69ad083e0744872f57d59d049b069283303f5bbc646222907d779afe4d1` |
| [Backend loader header](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/ggml/src/ggml-backend-dl.h) | 791 | `225bd83a197c4e8b5f985717176824d1144e2b74a79f04838f51b84115c14dc5` |
| [Common audio source](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/common-whisper.cpp) | 8,141 | `852fbc77d2461322a82b9c571cf4703bac3c78c5c51d3a90e80792ce0c04e313` |
| [Root build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/CMakeLists.txt) | 12,772 | `2f48f0a240f2ad9e6ae7e239ed63ca60bc2f5b431a7f01b56f8d0b4a395b9f3f` |

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
retained loader header only declares platform calls. The Windows loader
implementation is in an additional source file not yet retained.

Exit code zero alone does not prove recognition. The CLI can return zero for
argument and language errors, and can continue after an audio-read failure.
On Windows, `--output-file -` opens `CON`; redirected stdout cannot be assumed
to receive JSON. `--output-json` writes a file, so a future structured result
route also needs an enforced output-storage bound. A first contained pilot
would capture bounded ordinary output and require a validated result and exact
input coverage. Timestamp and token semantics require separate validation
before publishing aligned cues. No model or native member has run.

The final three table rows first passed one bounded HEAD each on 2026-09-23.
All returned 200 through checked public peers and verified TLS, with no
redirect, content encoding, or application body read. Separate exact GETs
reserved 21,704 bytes in total. Their observed body hashes above match the
publication receipts; their ETags are opaque response metadata, not content
hashes.

| Pinned file | Declared bytes | Observed ETag |
| --- | ---: | --- |
| [Backend loader](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/ggml/src/ggml-backend-dl.h) | 791 | `"9274f1ab5da7c6c00c443c631b165cb158c592a05c032cba7efa5de3281f17d4"` |
| [Common audio source](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/common-whisper.cpp) | 8,141 | `"fa4138651db4daf8b57b75884d6a3631ffccaaa9264c0604307cb7295a0ab232"` |
| [Root build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/CMakeLists.txt) | 12,772 | `"be2809d526fb5298c0d23b88315d62998b6a72423a9f8ba737476aacc6a39de4"` |

The common audio source decodes through miniaudio and includes `stb_vorbis.c`.
Its stdin branch appends until EOF without an application byte limit, and its
decoder allocates according to reported frame count. It also contains a
`system`-based speech helper; the reviewed CLI source does not call that
helper. `WHISPER_COMMON_MINIAUDIO_SKIP` can change its decoder path, so a
contained run needs a fixed environment. The bounded pilot should pass only
the verified local WAV path and enforce process memory, output and descendant
limits independently. The root build defaults turn `WHISPER_CURL` and
`WHISPER_SDL2` off, but build defaults do not prove the configuration or binary
equivalence of the retained release ZIP.

The next exact-path review is the loader implementation and build membership
in `ggml/src/ggml-backend-dl.cpp` and `ggml/src/CMakeLists.txt`, plus the
incorporated `examples/miniaudio.h`, `examples/stb_vorbis.c` and `ggml/LICENSE`
for notices. Their lengths, source hashes and route permissions are not yet
qualified. Native process cleanup, resource and output caps, legal notice
closure, and an end-to-end three-file recognition pilot remain open.
