# Delegated Linux worker containment

Reviewed: 2026-09-30. Status: source audit and implementation proposal, not a Linux execution result. No dependency was changed, no native model ran, and external spend was USD 0.

The existing Windows executor can enforce its requested Job Object limits. Linux needs a different cgroup topology and a precise distinction between process and thread limits before native recognition can be enabled. Setting systemd delegation alone does not repair the pinned implementation. Preserve the current `limits-unavailable` refusal until the checks below pass on a real Linux host.

## Exact source reviewed

The local Cargo registry copy of ProcessKit 3.3.4 identifies upstream commit `ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6` in `.cargo_vcs_info.json`. The workspace pins that package to `=3.3.4` with the `limits` feature. Line references below describe that package, rather than upstream `main`.

| Source | Observation |
| --- | --- |
| [Public group options, lines 42 to 109](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/group.rs#L42) | Shutdown options and resource limits are exposed. There is no public delegated-parent directory option |
| [Linux group creation, lines 1446 to 1554](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/sys/linux.rs#L1446) | Each job is created under the service process's own cgroup. Controllers are enabled in that parent; `max_processes(n)` writes `n` to `pids.max` |
| [Linux spawn, lines 160 to 230](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/sys/linux.rs#L160) | A dependency-owned pre-exec hook joins the cgroup before executing the native binary. An optional parent-death signal is armed afterward |
| [Raw group spawn, lines 266 to 279](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/group.rs#L266) and [spawn defaults, lines 235 to 269](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/sys/mod.rs#L235) | Sigy's raw `group.spawn(tokio::process::Command)` path uses default spawn options, with parent-death signaling off |
| [Linux crash caveat, lines 41 to 59](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/sys/linux.rs#L41) | A cgroup persists after its creator is abruptly killed. Rust `Drop` cannot cover that event; a live descendant can remain |
| [Windows assignment caveat, lines 106 to 119](https://github.com/ZelAnton/ProcessKit-rs/blob/ba1a6fe77cedad7e1ceeb9f30b158c94b5dc1bb6/src/sys/windows.rs#L106) | A child created suspended can become an inert orphan if the parent dies before assigning it to the job |
| [Current Sigy executor](../../../crates/sigy-service/src/execution/local.rs), [recognition](../../../crates/sigy-service/src/execution/asr.rs), and [translation](../../../crates/sigy-service/src/execution/translate.rs) | One supervisor owns launch, deadline, cancellation, and the empty-group check. Decoder limits request one process. Recognition and translation pass the task's process and CPU limits into ProcessKit |

The local links above point to the implementation in this checkout. The current execution paths are `asr.rs:456` for decoder creation, `asr.rs:548` for recognition, and `translate.rs:192` for translation. `local.rs:142` publishes the snapshot only after its active-member count is zero. These line numbers will move as the code changes.

## Required topology and host

Use an explicit delegated parent with a service leaf and sibling worker leaves:

```text
sigy.service/                 delegated, no resident service process
  service/                    service and its trusted control processes
  worker-<generation>-<id>/    one bounded job, including all native threads
```

A non-root populated domain cannot distribute domain controllers to its children. The PID controller counts kernel tasks, including threads. Thus a Linux `pids.max=1` is unsuitable for a decoder or recognizer that creates threads; increasing it also permits additional processes unless another boundary rejects them. This is a separate limit-contract problem, not a reason to remove the cap. [Kernel cgroup v2 rules](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html#controlling-controllers), [PID controller](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html#pid).

The candidate host must supply a writable cgroup v2 delegation with `cpu`, `memory`, and `pids` available, and an independent owner-death cleanup mechanism. A proposed profile requires Linux 5.14 or later and checks for the actual `cgroup.kill` interface instead of trusting the version alone. It must resolve the real mount and membership without mistaking a container's namespace root for a writable delegated hierarchy.

For the first rehearsal, use a real systemd Linux host or VM. A user service or system service needs explicit controller delegation. `DelegateSubgroup=service` is available from systemd 254; older managers require the service to move into its own leaf before controller enablement. Delegation makes controllers available but does not enable them automatically. Neither root privileges nor a writable private cgroup namespace alone establishes the needed contract. [systemd delegation guidance](https://systemd.io/CGROUP_DELEGATION/).

Rehearse `KillMode=control-group`, enabled final SIGKILL, and a bounded stop timeout so the service manager can stop the complete delegated subtree. Prove what happens on abrupt service death, rather than assuming an ordinary group drop runs. [systemd killing semantics](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd.kill.xml).

## Implementation alternatives

| Alternative | Feasible direction | Remaining evidence |
| --- | --- | --- |
| Extend the maintained containment dependency | Add an explicit delegated-parent handle through a reviewed upstream release or documented local patch. Keep its existing pre-exec join, kill, and member enumeration. Sigy's Rust calls remain safe, and all TaskSpec, lease, result, and supervisor logic stays shared | No suitable public option exists in the pinned package. A changed dependency requires full source, license, distribution, and hostile-path review, followed by Linux tests |
| Trusted Rust launch wrapper in the existing executor | A small single-threaded helper joins a preconfigured worker leaf, verifies membership and limit readback, then uses safe `CommandExt::exec` to replace itself with the hashed native program. Cgroup file operations can use safe standard or maintained fd-relative Rust APIs. The native program never executes before the join | The trusted helper begins in the service leaf. Bound that interval, eliminate unrestricted model/shell arguments, protect cgroup path ownership and permissions, define owner-death cleanup, and prove all failure paths. This remains one executor backend, not a second scheduler |
| systemd transient worker services | Request manager-created worker units through a reviewed Rust D-Bus binding, with fixed argv, finite resource properties, output plumbing, and generation-scoped identity. The manager supplies process creation and lifecycle ownership | More integration and dependency work: user-manager availability, permissions, exact unit identity, cancellation, signal ordering, output bounds, timeout, owner dependencies, and empty-unit confirmation. Unit construction must not become a general shell or permission surface |

The first alternative minimizes changes to the existing executor, but it is not available merely by changing Sigy's options today. The wrapper is a viable experiment when dependency changes cannot supply the parent handle. A process born into a cgroup through `clone3(CLONE_INTO_CGROUP)` would remove even the trusted helper's initial accounting interval, but it needs a maintained safe spawning API. Calling a raw syscall or adding first-party unsafe pre-exec code would violate repository policy. [Linux clone3 interface](https://man7.org/linux/man-pages/man2/clone3.2.html).

Keep a finite task/thread cap separate from the product's process-count limit. For a one-process native profile, a maintained syscall-filter implementation could prohibit new processes while permitting bounded native threads. The policy would need to distinguish `clone` thread flags and evaluate `clone3` fallback behavior on each runtime. The [current seccompiler documentation](https://docs.rs/seccompiler/latest/seccompiler/) exposes argument conditions and a safe filter-install interface; its old repository moved to the [Rust VMM monorepo](https://github.com/rust-vmm/rust-vmm). This is a candidate for dependency review, not a selected dependency or a working policy.

Cgroup migration permissions must also be considered: if the native process has the same UID and write access to the delegated hierarchy, resource placement alone does not prevent it from moving out or relaxing a cap. Do not claim a hostile-code sandbox from cgroup membership. Any separate UID, filesystem namespace, syscall policy, or network denial needs explicit implementation and real tests. Network isolation remains its own release gate.

On Linux, the existing dependency joins before native exec. That differs from the Windows suspended-create assignment window, which leaks an inert child on abrupt death. Improving Linux topology cannot close the Windows gap. Evaluate a maintained atomic job-assignment spawn API separately; do not report either platform's cleanup as proven from the other's fixtures.

## Rehearsal matrix

Run these checks on a disposable library with loopback fixtures and the fault recognizer before downloading models or enabling recognition on user recordings.

| Case | Required observation |
| --- | --- |
| Valid delegated hierarchy | Service leaf stays independent. Worker membership and finite CPU, memory, task, and process policy are established before native exec |
| Missing, readonly, or incomplete delegation | `limits-unavailable` before native launch; no fallback to an uncapped process group |
| Namespaced root and non-root populated parent | Refusal or correctly resolved delegation, never an assumption based on `0::/` |
| Ordinary multithread decoder and two-thread recognizer | Thread startup succeeds within measured task allowance; a forbidden new process cannot escape the process bound |
| Fork, clone, and repeated thread creation | Each enforced dimension refuses at its own boundary; account for library and runtime-created threads |
| Memory exhaustion and CPU load | Limits are read back and exercised; host and capture remain responsive. Record memory-controller semantics separately from Windows committed memory |
| Cancellation and wall deadline | Kill the full worker subtree, then observe recursive `populated=0` before releasing the input lease or publishing a terminal result |
| Service SIGKILL with a worker and grandchild | Manager or independent owner removes all native descendants; restart invalidates the generation and does not replay an old completion |
| Death before join and before exec | No native payload executes outside the configured worker leaf; trusted helper cleanup is bounded |
| Corrupt paths, symlinks, wrong-owner files, stale job identity | Refusal without writing to another delegation or killing another job |
| Unreadable membership and failed kill | Cleanup stays unproven. Keep liability and lease; do not translate an I/O error into an empty group |
| Cgroup tree exhausted or leftover after crash | Admission fails within bounded time. Cleanup touches only the library's established generation and identity |
| Restart, concurrent jobs, and capture | Existing generation, fair-queue, retained-input, translation, and capture independence invariants still hold |

Empty Linux groups currently publish no worker CPU or peak-memory numbers through Sigy's ProcessKit snapshot. A future backend could read persistent cgroup counters, but those meanings and schema bindings need their own evidence before doctor claims a measurement. The [native recognition decision](../../../docs/decisions/0039-native-recognition-worker.md) and [worker-cost decision](../../../docs/decisions/0054-recognition-worker-cost.md) remain authoritative.
