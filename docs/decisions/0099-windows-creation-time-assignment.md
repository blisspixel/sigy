# 0099: Windows creation-time job assignment

Date: 2026-10-20. Status: reviewed limitation. This decision changes no runtime, dependency, schema, or IPC. Catalog schema stays v51 and local IPC stays v53. It does not close S-03B, S-04, or a roadmap stage.

## Problem

Windows native workers join a Job Object after `CreateProcess` returns. The child is created with `CREATE_SUSPENDED`, then `AssignProcessToJobObject` runs, then the primary thread is resumed. If the parent dies after creation and before assignment, kill-on-job-close cannot reach the child. The child stays suspended, so it does not run, and it is not a member of the job. One recognition fixture on 2026-09-25 observed that outcome: a stand-in stayed suspended for more than 14 minutes and survived service exit.

Microsoft documents a creation-time alternative. `PROC_THREAD_ATTRIBUTE_JOB_LIST` gives `UpdateProcThreadAttribute` a list of job handles. The new process is assigned to those jobs in order, before its initial thread is allowed to run. The attribute is supported on Windows 10 and Windows Server 2016 and newer. The value is a pointer to the handle list, and the size is the byte length of that list.

## Decision

Sigy keeps the existing suspended create, assign, and resume sequence. Creation-time assignment stays an explicit limitation and a release gate for recognition on user recordings.

The pinned toolchain is stable Rust 1.98.1. On Rust 1.98 and Rust 1.99, `std::os::windows::process::CommandExt::spawn_with_attributes` and `ProcThreadAttributeList` remain nightly-only under `windows_process_extensions_raw_attribute`, tracking issue 114854. A toolchain bump to 1.99 does not make that API available. Tokio 1.53.1 builds on `std::process::Command` and exposes no attribute list.

Vendored ProcessKit 3.3.4 is the latest crates.io release of that crate. Its Windows `Job::spawn` sets `CREATE_SUSPENDED`, calls the command's own `spawn`, and then calls `AssignProcessToJobObject`. The crate documents the parent-death gap on the job handle. Its pseudoconsole path builds an attribute list for the console handle and still assigns the job afterward. Sigy enables the `limits` feature only, so that pseudoconsole path is not in the product build. The ordinary path is the one recognizers, translators, decoders, and the audio helper use. The vendored tree contains no job-list attribute.

An early return or panic after `spawn` returns is covered by ProcessKit's uncontained-child guard, which reaps the still-suspended process. That guard does not run while `spawn` itself is blocked, and abrupt process death does not run `Drop`. A Sigy timer cannot name a child the library has not returned. Assignment failure still returns the operating-system error and reaps the suspended child. There is no uncontained fallback.

A Job Object remains a resource and lifetime limit. It is not a network sandbox. Nested-job and security restrictions can still refuse assignment. Memory the process allocates before it is a member is not charged back to the job limit. The suspended child has not run user code, so that accounting gap is the process object itself during the assign window.

## Alternatives

Using the nightly attribute API was rejected. The repository builds on the stable pinned toolchain, and Rust 1.99 still marks the API nightly-only.

A first-party `CreateProcessW` wrapper was rejected. Workspace lint forbids `unsafe_code` in first-party sources. A safe wrapper around a new unchecked creation call would not meet that boundary.

Replacing ProcessKit's `Job::spawn` with a raw creation call inside the vendor tree was rejected. The vendor override is a small reviewed patch for delegated Linux groups. A Windows rewrite would have to reproduce standard-library argument quoting, environment blocks, working directory, and stdio handle inheritance for every native worker. The existing pseudoconsole creation path explicitly skips ordinary handle inheritance. A partial replacement that loses that contract is a worse failure than the documented suspended window. Checksums would record the rewrite honestly, and honesty does not make the rewrite small.

Placing the service process in the worker job, so children inherit membership, was rejected. Kill-on-job-close would then apply to the service.

The earlier trusted-launcher bootstrap remains a cooperative timeout. It is not an operating-system guarantee that the child was in the job at creation, and the pinned safe wrappers still do not retain the primary thread for inspection before resume.

## Evidence

Reviewed on 2026-10-20 against the vendored ProcessKit 3.3.4 sources, the published Rust 1.98 and 1.99 standard-library docs, the crates.io 3.3.4 release, and Microsoft's `UpdateProcThreadAttribute` contract as updated 2026-08-05. Raymond Chen's 2023-02-09 note describes the same job-list attribute and the parent-death window it closes. No process was spawned for this review. No checksum, manifest, or runtime source changed. External spend is USD 0.

## Limits

The assign window stays open. Abrupt owner death inside it can leave a suspended unassigned child. This review does not measure that window, qualify native containment, or change kill-on-close for a child that was assigned. Language, platform, and acoustic qualification are unchanged.
