# Pinned CPU source metadata pilot

Reviewed: 2026-09-23. Five exact files at whisper.cpp revision
`927cfce34f31707e17f2bff35c349632fb9e2c3a` returned HTTP 200 to one
HEAD request each. The requests checked DNS answers, selected a public peer,
verified the TLS hostname and certificate chain, denied redirects, and retained
only bounded response metadata. No source body was requested or read by the
application. These results are acquisition planning evidence, not source review,
legal closure, runtime qualification, or permission to execute the CPU archive.

| Source file | Declared bytes | HTTP ETag |
| --- | ---: | --- |
| [Release workflow](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/.github/workflows/release.yml) | 40,031 | `db33ec48a8bccdeb7291d92d6ed4d9264cb2d3ba2ab96b89ef0ee8cb36759f1e` |
| [CLI source](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/cli.cpp) | 64,027 | `defdf655ec8738de9417702ffa9fb261330756251a2175e7ad0fc646ee899bbf` |
| [CLI build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/CMakeLists.txt) | 228 | `48e82eefe1232d5f95f55be950d62577d7f3f6ffbeb6d80cd3719206e630ea53` |
| [Examples build file](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/CMakeLists.txt) | 3,688 | `3751590a4a53d29135a6a4f023d71f10e345b24a4edc993f2ac9afbce9c95ef2` |
| [Backend registry](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/ggml/src/ggml-backend-reg.cpp) | 18,555 | `8f2af02691c42066b5654f5f4f797da6d2fad39a48d04d13d46314e90b6e80a1` |
| Total | **126,529** | Five independent responses |

Each receipt binds the exact URL, source revision, frozen probe binary and
control hashes, checked DNS answers, selected peer, response length and ETag.
All five receipt sidecar hashes matched an independent local hash. The probe's
frozen binary SHA-256 is
`11944a8ac6650bcb7cacd64d1d7fefc26685039bb3f2fe4baef024a69e0f40ec`;
its source aggregate is
`731d952774db62cba8f8b49191d6583d5a13b57d748abb67608c309b4f6f4c13`.
Seventeen offline tests, formatting, warnings-denied Clippy, build, and cached
advisory checks passed before the real requests. The observed peers were
`185.199.111.133:443` and `185.199.110.133:443`. No HEAD response had a
Content-Encoding or Location header. The shared body-download ledger did not
change, and external paid spend remains USD 0.

HEAD metadata does not authenticate future body bytes. The ETags are server
observations, not independently verified file hashes. TLS or the OS may read
ahead even though the application requested no decrypted body bytes. A later
GET must reserve its full finite bound in the shared ledger before DNS, check
the representation again, publish exact bytes and SHA-256, and preserve the
source evidence. Source inspection must then identify any additional common
code, backend dependencies and required notices. No downloaded native member
or model has run.
