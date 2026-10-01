# Yanked derive dependency repair

Reviewed: 2026-09-30. Scope: the locked transitive `yoke-derive` dependency. The strict `cargo audit --deny warnings` gate refused version 0.8.3 because the registry marked it yanked. No vulnerability claim is inferred from a yank.

## Evidence and choice

The primary [crates.io metadata](https://crates.io/api/v1/crates/yoke-derive) reports 0.8.3 as yanked and 0.8.4 as available, with Rust 1.82 as its minimum version. The existing `yoke` 0.8.3 requirement is `yoke-derive ^0.8.2`, so 0.8.4 satisfies it without changing a first-party manifest. The pinned Rust 1.98.1 is above that minimum.

Both registry archives were checked against their published SHA-256 values. The [0.8.3 source](https://github.com/unicode-org/icu4x/tree/ce235d80cf7cbd028e8bd59920420e1203b205b3/utils/yoke/derive) and [0.8.4 source](https://github.com/unicode-org/icu4x/tree/a59ab860d4bda548e94dfbf992d87fe1f761bc55/utils/yoke/derive) differ in version, VCS metadata, explicit minimum Rust version and equivalent construction of an underscore string. The comparison found no change to derive output, lifetime or covariance checks, dependency requirements, licensing or native code.

The chosen repair is `cargo update -p yoke-derive --precise 0.8.4`, preserving the rest of the dependency graph. Version 0.8.4's registry checksum is `ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71`. Downgrading to 0.8.2 was an available alternative, but the reviewed compatible patch avoids reversing the prior update.

Final compiler, test and advisory results belong in [active work](../../docs/development/progress.md). Registry state and this source comparison do not replace those checks. External spend: USD 0.
