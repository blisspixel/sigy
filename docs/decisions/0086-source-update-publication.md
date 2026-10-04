# 0086: Source installation and update publication

Date: 2026-10-04. Status: implemented repair increment; exact verification evidence belongs in [active work](../development/progress.md). Source installation follows `main`; this creates no supported binary distribution or platform qualification.

## Decision

Keep the copy/paste installers and `sigy update` on the existing fixed repository and pinned toolchain. Refuse a dirty or different-origin managed checkout. A checkout installer can intentionally build local changes, but records no clean commit identity for them. The installed binary's embedded commit takes precedence over the legacy per-user marker; that marker cannot prove a custom installation's identity.

Every updater prepares a separate local clone without hardlinks, checks out the exact full fetched commit and fixes the canonical origin. Later movement of the managed checkout cannot retarget that build. Prepared work stays under the per-user metadata directory, with at most 16 operation directories and an explicit refusal when full. Retain uncertain work for inspection rather than deleting a workspace an existing helper could still own.

Windows scripts and updates share an operating-system-owned exclusive installation lock. Process death releases ownership. Scheduling a helper records a pending operation, then the fixed hidden helper waits up to 60 seconds for its parent and 30 seconds for that lock. It proceeds only if the bounded receipt still names its exact pending operation. A direct installer fences an outstanding pending or running operation while holding the same lock, before building. An older helper cannot overwrite a later installer or its receipt. A helper that never acquires the lock cannot publish another operation's outcome.

The Windows helper validates clean source, exact commit and canonical origin before and after a staged Cargo install. It freezes absolute Cargo-home and installation-root paths before changing directories. Explicit installation-root configuration wins; otherwise an executable under `bin`, including Windows case variants, identifies its installation root before the Cargo-home fallback. Configuration-only Cargo roots are not inferred. Copy the staged executable to a unique temporary path beside the installed executable, then replace it with a same-volume backup. Hash the published file against the staged artifact before recording success. Keep build failure, publication failure and failure after successful replacement distinguishable.

Pre-publication validation and build refusals leave the installed executable untouched. File replacement is a separate filesystem boundary: documented replacement failures can move the original to the backup name. Preserve remaining target, temporary and backup artifacts for inspection; do not claim universal rollback, power-loss durability or automatic recovery. A successful replacement followed by bookkeeping failure can leave the new executable installed without a succeeded receipt. Never equate scheduled work or an elapsed deadline with successful installation.

## Inspection and callers

`sigy update --status` reads one bounded local receipt without fetching, building, opening a library or contacting a service. `--status` conflicts with `--check`. Receipts have an 8 KiB read cap, strict fields and bounded protocol, operation, commit, state and absolute install-root identity. Human output sanitizes untrusted formatting controls; structured output preserves the original fields. Pending and running records explicitly leave helper liveness and completion unproven. They are not an automatic recovery or retry mechanism.

The PowerShell installer restores its caller's location and temporary environment values even on failure. The shell installer runs in a subshell. Both resolve relative Cargo paths before changing directories and propagate failed child commands. First-use guidance names `sigy init --radio` and the terminal explorer. FFmpeg remains separately configured and is never downloaded by these scripts.

Non-Windows updates remain synchronous Cargo installs with an operating-system lock and source revalidation. Independent POSIX script concurrency and Windows-style staged publication are not qualified there. No installer performs catalog migration in an old running service. The operator must stop that service with its existing binary before replacement.

## Acceptance and references

Require independent local Git and child-process witnesses for exact frozen source, custom relative paths, source drift before and during build, Cargo failure, locked executable refusal, successful publication/hash agreement, process-death lock release, stale helper fencing against the actual direct installer, bounded receipt failures and caller-state restoration. The README wrappers must propagate failed fetch and installer execution. Stubs isolate installer orchestration; they do not establish fresh Rust bootstrap, remote source provenance or native Linux/macOS support. Run the ordinary workspace, lint, advisory and per-crate coverage gates on the final source.

Primary references reviewed on 2026-10-04: [File.Replace](https://learn.microsoft.com/en-us/dotnet/api/system.io.file.replace?view=netframework-4.8), [ReplaceFileW failure outcomes and same-volume requirement](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew), [PowerShell scopes](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_scopes?view=powershell-7.6) and [Rust file locking](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock). API descriptions guide the contract; actual platform and fault receipts establish its tested scope.
