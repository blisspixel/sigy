# 0002: Local controller and detached lifecycle

Date: 2026-09-20. Status: implemented and locally tested on Windows x86_64. Native Unix and OS service installation remain unqualified.

## Decision

Use one local controller owning the existing library lock and SQLite connection. A bounded catalog actor serializes application operations. The CLI performs the same operations in maintenance mode when no controller owns the library and reconnects when it does. Client exit does not own service lifetime.

Use Interprocess 2.4.4 local sockets with Tokio 1.53.1. Windows uses a random named pipe with a protected owner/SYSTEM descriptor and network logons denied. Remote pipe clients are disabled. The client checks the server process ID against discovery metadata. Unix uses a filesystem socket inside a private user-owned directory and checks effective peer UID; Linux additionally sets socket mode 0600 before binding. No TCP listener is exposed.

The current protocol is v2: length-prefixed JSON, strict request fields, 16 KiB requests, 256 KiB responses, 32 clients, a bounded actor queue, and five-second client deadlines. Only status, budget limits and stop operations are exposed. A lost reply does not roll back a committed mutation; clients do not automatically retry mutations. A protocol mismatch fails explicitly rather than guessing compatibility.

`service run` runs in the foreground. `service start` spawns a detached process and checks readiness. `service status` never starts one; `service stop` requests orderly shutdown. Startup reconciles uncertain paid submissions and abandoned active capture attempts before publishing readiness. Endpoint cleanup occurs before the last library ownership guard is released, including actor failure paths.

## Process creation and dependencies

On Windows, stable Rust process creation with null standard streams still allowed unrelated inheritable handles to reach a detached child. A real pipe-based CLI test exposed a caller waiting for EOF until the service exited. Use winsafe 0.0.29 with only its kernel feature to call process creation with handle inheritance disabled and no window. Executable and working-directory paths are separate API arguments; the command line is constant. Only the inherited-environment path is used. Any move to a custom environment block requires a fresh review of the wrapper's native buffer lifetimes.

On Unix, the child creates a new session before initializing runtime threads. Failed startup cleanup targets only the owned child process. No startup registration, administrative installation, automatic update or global service management is performed.

The source/lockfile records these dependencies plus getrandom, widestring and rustix where needed. Loopback HTTP would add network authentication and exposure concerns for an initially local operation set. Protected OS IPC is the smaller boundary for this milestone. Binary serialization and a general RPC framework are unnecessary for the current small control messages; media payloads do not belong in this channel.

## Evidence and limits

Tests cover real separate clients, lock ownership, detached return, output-pipe EOF while the service stays alive, repeated start, graceful stop, stale discovery after process kill, restart uncertainty, capture recovery, oversized/truncated frames, unknown fields, unsupported versions and mismatched server PID. Strict empty struct request variants are intentional: unit variants did not reject extra JSON fields in the tested serialization behavior.

Windows control checks do not audit every directory ACL or ancestor reparse point. Use a private directory owned by the current user. Unix code requires native Linux/macOS tests, and the named-pipe descriptor needs separate-account qualification before broader release claims. Current malformed-client failures close the connection; persistent protocol metrics and richer diagnostics remain to build. Logout, reboot, sleep/resume and packaging are separate acceptance gates.

API references reviewed for this boundary: [Interprocess local sockets](https://docs.rs/interprocess/2.4.4/interprocess/local_socket/index.html), [Tokio runtime builder](https://docs.rs/tokio/1.53.1/tokio/runtime/struct.Builder.html), [Windows process creation](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw), and [stable Rust Windows command extensions](https://doc.rust-lang.org/1.98.1/std/os/windows/process/trait.CommandExt.html). Local tests establish the behavior described above; API documentation alone does not qualify a platform.
