# Repository structure and engineering evidence

Reviewed: 2026-09-20. Status: primary-source review and proposed engineering implications. No stack, toolchain, CI workflow, or Scorecard result is selected or validated.

## Workspace conventions

Cargo workspaces can share a lockfile, dependency declarations, and lint configuration. Members opt into inherited workspace settings; declaring a root lint policy alone does not establish that every member uses it. For Sigy, a few packages with explicit dependency boundaries are a better initial hypothesis than one crate per integration. [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html).

Go's official module-layout guidance uses command directories and private `internal` packages. This supports the same domain/service/client ownership design without needing multiple separately versioned modules. [Go module layout](https://go.dev/doc/modules/layout).

Clippy documents warnings-denied operation. Go documents vulnerability and security tooling. Tool availability supplies candidate verification mechanisms, not evidence that Sigy's future configuration runs them. Exact commands and stable versions must be qualified once manifests and targets exist. [Clippy usage](https://doc.rust-lang.org/clippy/usage.html), [Go security](https://go.dev/doc/security/).

The CLI parser and async runtime are independent choices, with [clap](https://github.com/clap-rs/clap) and [Tokio](https://tokio.rs/) as Rust candidates. Existing [language research](07-language-and-stack.md) covers ownership, native boundaries, cancellation, Go runtime tradeoffs, and terminal candidates. No documentation-only comparison establishes the smaller assembled dependency graph or better workload performance.

## Persistent instructions and local state

The current instruction-loader documentation recognizes repository `AGENTS.md`, with more specific files applying within their directory scope. Without a detected project root, it searches the current directory. A root instruction file therefore fits this workspace; a second duplicated tool-specific manifesto is unnecessary. Actual reload behavior was not tested by starting another agent session. [Instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md).

Git documents directory ignore patterns and slash-relative matching. `/.agents/` excludes the root scratch directory. After Git initialization on 2026-09-20, `git check-ignore -v .agents/session-checkpoint.md` confirmed that the root rule excludes the local session receipt. [Git ignore patterns](https://git-scm.com/docs/gitignore).

## OpenSSF Scorecard

Scorecard checks observable supply-chain practices such as testing, branch protection, review, dependency pinning, and workflow permissions. Checks evolve and detection has limits. Bot or automated reviews do not count as human review for the review check. Its documentation also acknowledges constraints for small maintainer teams. A high result is useful evidence about these practices, not a correctness certificate. [Check definitions and limitations](https://github.com/ossf/scorecard/blob/main/docs/checks.md).

The official action supports external publication of results through a distinct configuration option. Assessment and publication should therefore remain separate decisions. The private `blisspixel/sigy` repository now hosts the planning checkpoint; there is no release history or Scorecard assessment. Private-repository visibility also limits which evidence can be assessed or published. [Official Scorecard action](https://github.com/ossf/scorecard-action).

GitHub's security guidance recommends minimum token permissions and full commit-SHA pins for actions. It also discusses untrusted input and dangerous interactions between privileged workflows and contributed code. These controls belong in actual workflow configuration and hosting policy when created. [Secure use of Actions](https://docs.github.com/en/actions/reference/security/secure-use).

## Proposed application

Keep the complete direct, transitive, native, build, and model-asset dependency inventory visible. Prefer mature security-sensitive implementations over an artificially tiny manifest. Make local verification and CI share commands, qualify releases on their actual platforms, and preserve evidence for crash, cost, multilingual, and extension behavior.

Plan periodic Scorecard reviews once the project has observable engineering history. Start with individual findings and their applicability, then track regression and remediation. Do not copy a moving list of checks or numeric scoring formula into standing instructions. Recheck current stable tools and primary guidance at implementation and release gates. [Repository organization proposal](../docs/planning/12-repository-and-engineering.md).
