# Native dependency override

## libsqlite3-sys 0.38.2 with SQLite 3.53.4

The current stable rusqlite 0.40.2 and libsqlite3-sys 0.38.2 releases bundle SQLite 3.53.2. The current stable SQLite patch on 2026-09-20 is 3.53.4. Sigy uses a Cargo path patch to keep the stable Rust binding and compile the current official SQLite engine.

The crate was copied from the crates.io libsqlite3-sys 0.38.2 package. Its code, build settings, generated Rust bindings, license, and upstream notices are retained. The local registry completion marker and package-local lockfile are omitted. Only `sqlite3/sqlite3.c`, `sqlite3/sqlite3.h`, and `sqlite3/sqlite3ext.h` are replaced, byte-for-byte, from the official 3.53.4 amalgamation. No SQLCipher, extension loading, or build-time binding generation feature is enabled by Sigy.

- Source archive: [SQLite 3.53.4 amalgamation](https://www.sqlite.org/2026/sqlite-amalgamation-3530400.zip).
- Archive SHA3-256: `628a44cfe82c66aed1ccbbe85a562d2e33ebe64b3288981ed76285612227934e`.
- `sqlite3.c` SHA3-256: `67f423e9ebbbdc473cbc4772c872ee6b89f31fde4ed0279a5c25d5f65c043a16`.
- [Official release and source identity](https://www.sqlite.org/releaselog/3_53_4.html).
- Binding license: [upstream MIT license](libsqlite3-sys/LICENSE). SQLite's public-domain dedication is preserved in its sources.

This override is temporary. Remove it when a qualified stable upstream binding bundles at least SQLite 3.53.4, after the catalog/ledger tests pass and the runtime version test is updated. Review all changes against the upstream package; do not independently maintain or rewrite the FFI implementation. Run normal dependency advisory checks as well as verifying native source provenance. Avoid formatting or linting third-party source as first-party workspace code.

`checksums.json` records SHA-256 for every retained crate file after the verified replacement. The shared verification script checks it before compilation. The unchanged generated bindings describe the stable SQLite ABI from the upstream crate; compile-time version constants still reflect that upstream binding snapshot. Runtime engine identity is read through SQLite itself, and no product version decision uses those constants.
