# Windows native recognition boundary

Reviewed: 2026-09-23. Status: design and trusted-fixture work, not an enabled
recognition worker. No model, release binary, or native member has run.

The first local recognizer needs a worker that can prove its entire process tree
has stopped before releasing a recording read lease or publishing a transcript.
The existing checksum worker supervises Rust work, but it does not establish
this native boundary. The staged recognition writer remains test-only.

## Launch choice

Use one preconfigured Windows job object and create the child with an extended
startup attribute list containing that job in `PROC_THREAD_ATTRIBUTE_JOB_LIST`.
The same creation must apply the exact zero-capability AppContainer attributes,
restricted inherited handles, a hidden console, and suspended start. Retain the
process and primary-thread handles for inspection before resume. Refuse any
failed attribute or membership check, with no less-restricted fallback.

The older create-suspended, assign-to-job, resume sequence has a parent-crash
window before assignment. [Microsoft's explanation](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812)
describes that orphan risk and the at-create job-list solution. The
[attribute contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
specifies the job-list and child-process-policy inputs. A one-process limit
with child creation restricted is a candidate for the first runtime, subject
to a fixture proving that the runtime needs no children. A separate two-process
fixture must still exercise descendant cleanup.

The cached safe Rust wrappers do not collectively expose the required atomic
job-list launch, exact retained-child token inspection, and read-only security
descriptor checks. Resolve those API gaps through a reviewed maintained safe
interface before enabling native execution. Moving new unchecked native calls
behind a first-party wrapper would not satisfy the workspace boundary.

An alternative trusted-bootstrap design is under fixture review. Start a small
Rust launcher with a finite authorization timeout, no model access and no
native dispatch. The service assigns that live launcher to a job through its
retained process handle, verifies membership and liveness, then sends one
private authorization frame. A normal child without breakaway flags should
inherit the job during creation, including when the service dies before the
child reports its PID. [Windows job inheritance](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
and [assignment semantics](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject)
support this candidate. Unlike a suspended launcher that awaits assignment,
an unassigned running launcher can exit on authorization timeout or control
pipe EOF. That pre-assignment cleanup is a trusted cooperative claim, not an
OS-enforced guarantee. Memory allocated before job assignment is not
retroactively checked against the job limit.

This route has not solved the full native boundary. The current safe
AppContainer wrapper inherits only its three configured standard handles,
starts an executable by path, and does not expose the exact child token or
primary thread for independent pre-resume inspection. Fixed paths, retained
read pins and ACL checks would need a separate reviewed contract. Keep the
zero-capability, effective-access, network-denial and cleanup gates below.
Do not use a non-escalating process-group shutdown that clears kill-on-close
or resource limits while members survive. A trusted fixture must falsify the
inheritance and authorization sequence before this candidate can replace the
at-create job-list design.

An ignored Windows trusted fixture has now exercised this bootstrap sequence
with ordinary Rust children on the current host. Eleven finite cases and
three pure tests passed. The normal child was observed in the same job while
still suspended. Authorization refusals, dead or unadoptable launchers and a
one-process cap produced no child. Every case reaped its launcher, joined
bounded output readers and reached zero active job members. The exact local
receipts remain ignored. This supports job inheritance and the authorization
protocol for trusted code. It
does not test AppContainer composition, token or ACL inspection, abrupt
supervisor death, hostile descendant escape, network denial, resource-limit
enforcement under load, or durable lease recovery. No model ran.

The follow-up safe-API review found no qualified path to complete that proof
with the pinned wrappers. The current AppContainer wrapper keeps the process
handle private, closes the primary thread handle and exposes no exact-child
resume. The process-group wrapper resumes every member thread. The available
token wrapper's direct AppContainer SID query is unimplemented; an alternate
structure path has layout and type concerns. ACL enumeration also does not
establish the child's effective access because Windows checks the complete
token and descriptor. Follow the [token information contract](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation),
[thread resume contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-resumethread)
and [AccessCheck contract](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-accesscheck).
The next native-boundary experiment is a compile-only contract proof for a
maintained safe adapter that retains exact creation handles, returns owned
bounded token snapshots and resumes only the retained primary thread. A
runtime fixture can follow only if that interface exists. Do not substitute
child self-report, a PID-only query, or broad job resume for this gate.

A further 2026-09-23 source review ruled out using WinSafe 0.0.29's
`AccessInformation` result as a shortcut. Its [token structure source](https://docs.rs/winsafe/latest/src/winsafe/advapi/structs.rs.html)
declares
`TokenType` as `LUID`, while the [Windows structure contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_access_information)
requires `TOKEN_TYPE`. Its token reader also recasts a dynamically allocated
byte buffer as a typed box without establishing the typed allocation layout.
The direct AppContainer SID query remains unimplemented in that version. This
is a wrapper qualification finding, not evidence that a child escaped any
restriction. A contained Linux or WebAssembly route would require a separate
runtime, asset and isolation review; neither is ready for a model run here.

## Evidence before resume

Inspect the suspended child through retained handles: AppContainer status,
exact freshly created package SID, zero capabilities, job membership and
configured limits. Inspect the task-owned input/model/staging descriptors and
the loopback exemption list. The required file evidence is owner, DACL,
mandatory integrity label, exact-token effective access, and positive and
negative file-open controls. Full SACL inspection would require additional
privilege and is not a prerequisite for this initial proof. See Microsoft's
[security-information access rules](https://learn.microsoft.com/en-us/windows/win32/secauthz/security-information)
and [loopback-exemption API](https://learn.microsoft.com/en-us/windows/win32/api/networkisolation/nf-networkisolation-networkisolationgetappcontainerconfig).
A child's self-report can diagnose a fixture but cannot independently certify
its own restrictions.

For the first trusted fixture, test 512 MiB aggregate committed memory, a
finite CPU hard rate, a 15-second attempt, a two-second cleanup period, and
65,536 captured bytes plus one overflow byte per output stream. These are
fixture controls, not measured ASR capacity or an exact CPU-time quota. A
model profile needs its own measured limits, including GPU or shared memory.
Use actual job accounting, resource pressure, output flooding and negative
controls to verify that the configured limits take effect.

## Cleanup and recovery

The supervisor owns `Prepared`, `Suspended`, `Running`, `Draining`, and `Reaped`
states. Normal exit, cancellation, timeout, panic and output overflow converge
on job termination, zero active-member observation, retained-process waits and
bounded pipe drain. A successful root-process wait is insufficient. Job
completion notifications can wake the supervisor, but ordinary notifications
are not guaranteed, so they cannot be the sole cleanup proof. See the
[job-object lifecycle](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
Only a proven `Reaped` state may create the internal completion capability.
Unproven cleanup retains the read lease and refuses terminal success.

Persist exact attempt and generation before launch. Restart never resubmits
the attempt. A missing PID, failed named-job lookup or reacquired library lock
does not prove that descendants are gone. A qualified recovery owner or
separately tested restart mechanism is required before automatic recovery can
release a lease. Test supervisor death at each launch, drain and publication
boundary without an observer that accidentally holds the job handle open.

The retained CLI calls its dynamic backend loader before argument parsing and
writes `--output-json` to a file. A future structured result route therefore
needs a fixed hashed DLL stage, a restricted environment and a bounded output
mechanism enforced while the worker writes. A size check after exit is too
late. The [source-content review](source-content-pilot.md) records these
dependencies. Network-denial tests must include contained IPv4, IPv6, DNS and
proxy attempts with positive controls, separately from setup downloads.

The immediate implementation sequence is a safe at-create launcher, a trusted
fixture with independent restriction readback, drain and crash tests, then the
staged catalog mutations and a single verified WAV pilot. Native recognition,
quality and any release support remain open until that evidence exists.
