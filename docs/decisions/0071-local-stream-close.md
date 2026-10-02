# Local stream close

Date: 2026-10-02. Status: implemented and locally verified on Windows x86_64. The Windows client heap failure it targets is reproduced on the previous build, but its corrupting write is not identified; the cause remains unproven. Catalog schema, local IPC version and the frame encoding remain v43. This establishes no platform qualification or stage exit.

## Decision

Every control and listen stream in `sigy-service` is owned by one `LocalStream` type from the moment it is accepted or connected. Dropping it clears the Windows dirty-pipe flag through the supported Interprocess 2.4.4 [`assume_flushed`](https://docs.rs/interprocess/2.4.4/x86_64-pc-windows-msvc/interprocess/os/windows/named_pipe/tokio/struct.PipeStream.html) API before the pipe is released. The pipe therefore closes on the runtime that owns it and is never handed to Interprocess's detached linger thread. These streams are never split or cloned, so the cleared flag cannot belong to another owner. Unix sockets need no flag and close normally.

The client sends one request frame and reads one complete response frame. The service writes a response only after reading the whole request, so a complete response proves nothing remains to deliver; the client then closes. On a transport error, remote failure, timeout or cancellation, the stream is dropped inside the timed future, abandoning any unread output without a detached owner.

The service writes its response and then waits for the client to close, within the same absolute five-second deadline that already bounds the exchange. End of stream ends the handler. Any further byte is refused as a protocol fault and closes the stream. At the deadline the handler and its stream are dropped. A stalled or hostile peer therefore holds no service task or thread beyond the deadline, and service shutdown still drains every handler within that bound.

Clearing the flag does not cancel a native write that is already pending. Mio keeps that write's buffer and pipe handle until the write completes, so bytes the stream accepted still reach a reading peer before end of stream. This is how the listen pipe delivers the tail of a transfer after its writer is dropped. A response larger than the pipe buffer that a client never reads stays a pending native write, owned by the runtime's completion port, until that client reads or closes. It holds no task or thread.

The wire protocol is unchanged. Earlier version 43 clients already close after reading their response, so the service's new wait ends promptly for them. A new client against an earlier service closes after its response; that service's own cleanup is unchanged. No operation is retried, acknowledged or replayed; a committed operation stays committed whether or not its response is read.

## Alternatives rejected

- A real flush on drop or in a blocking task. `FlushFileBuffers` on a pipe waits for the peer to read. A started blocking task cannot be cancelled, so an untrusted or stalled peer could hold it indefinitely and delay runtime shutdown.
- An acknowledgement byte after the response. It changes the visible protocol, needs a version bump and leaves older peers waiting on a byte they never send.
- Server-side close without waiting for the client. Small responses would still arrive, but after a stop the service process could exit before a client read a larger response, and process exit abandons a pending write.
- Disconnecting the server pipe instance. It discards unread data, and no safe wrapper exists in the dependency graph; this repository forbids `unsafe_code`.
- A dependency fork, upgrade or raw-handle workaround, sleeps, leaked runtimes or handles, retries or test exclusions. None is justified by the evidence, and several would hide rather than remove the detached owner.

## Evidence

Fixtures in `local_stream`, `control::server` and `recordings::pipe` cover a complete exchange ending on client close under a 120-second deadline, an unchanged version 43 exchange, a client that never reads, a client that reads only the length prefix, a client that never closes, early close, bytes after the request, a client cancelled with a read pending, a 3 MiB late reader after the writer is dropped with its write pending, listen nonce refusal and stop before accept. Two Windows fixtures use a synchronous client end, which posts no read of its own: a dropped writer and the service's refusal path must both close although their small output stays unread. With the previous dirty drop restored through a temporary switch, 7 of these 14 fixtures failed, including the client success-path regression and both synchronous fixtures; with the supported disarm all 14 passed. Under uncontrolled host load the previous code failed 4 times in 20,492 real client processes, and repaired ordinary and instrumented builds ran 24,224 without a failure. That supports the repair but does not establish the cause. The dated counts and the reproduction are in the [shutdown investigation](../../research/34-windows-control-shutdown.md).

## Limitations

- The corrupting write behind `0xc0000374` is not identified. Removing the detached owner and a clean repeated run are not proof that the defect cannot recur; it stays tracked as cause unproven.
- Mio cancels a pending read with `CancelIoEx`, which does not wait for completion. Its buffer stays owned until the completion is processed or the process exits. This second suspect is unchanged.
- A never-reading client can keep a large response's pipe instance and buffer alive until it reads or exits. Only same-user and system principals can connect.
- Older clients and services still use their own detached cleanup. Mixed versions interoperate but keep that behavior on the older side.
- Platform evidence is one Windows 11 host under uncontrolled concurrent load. The fixtures that are not Windows specific also target Unix sockets, but this increment was not run on Unix.
