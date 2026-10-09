# Interface dependencies

The native Nix builder now uses Crane v0.24.0, explicitly requested by the user on
2026-10-07. Its [release](https://github.com/ipetkov/crane/releases/tag/v0.24.0)
was published 2026-08-21; `default.nix` pins commit
`692f7e9ef2ece8125b466f66f2af532b3edaed0d` and the fetched archive hash. Crane's
dependency/application split and toolchain override fit the existing workspace,
lockfile and custom Nix entrypoint. The alternative was retaining
`buildRustPackage` and recompiling dependencies after application changes, or
maintaining a custom dependency-artifact builder. No Cargo dependency changes are
needed. See [build mechanics](rust-builds.md).

AGENTS.md requires confirmation before adding dependencies. The user approved all choices below on 2026-10-06. Their exact versions are recorded in Cargo.lock; review is the default feature, while MCP and Parquet are optional.

For Parquet feature output, prefer Apache Arrow's `parquet` 60.0.0 with `default-features = false`, behind Loupe's optional `parquet` build feature. Its [release announcement](https://arrow.apache.org/blog/2026/09/29/arrow-rs-60.0.0/) documents current maintenance; the [crate manifest](https://github.com/apache/arrow-rs/blob/60.0.0/parquet/Cargo.toml) separates its low-level writer/reader from optional Arrow, codecs, async/cloud and CLI functionality. Its [workspace MSRV is Rust 1.88](https://github.com/apache/arrow-rs/blob/60.0.0/Cargo.toml), below Loupe's 1.90 floor. The flat feature schema can use the low-level writer with bounded row groups and verify actual files through the reader. Initially uncompressed files keep the dependency graph smaller. Arrow plus Snappy is a larger alternative that supports convenient column batches/compression. A handwritten Parquet encoder would add maintenance and compatibility risk.

For the specified review TUI, prefer `ratatui` 0.30.2 with defaults disabled and the Crossterm/backend and required widgets enabled, behind Loupe's `review` feature. The [current installation guide](https://ratatui.rs/installation/) specifies Rust 1.88, Crossterm 0.29 and the backend re-export. The [backend API](https://docs.rs/ratatui/0.30.2/ratatui/backend/struct.CrosstermBackend.html) accepts an explicit writer, allowing all display bytes to go to `/dev/tty`. Application code must still verify terminal presence, preserve stdin as data, and restore raw mode/screen/cursor state on completion/cancellation. A smaller custom interface is possible, with more terminal lifecycle code and a reduced widget UI. Approval does not authorize host deployment or publication.

MCP uses the official `rmcp` 3.5.0 SDK with defaults disabled and only `server`/`transport-io`, behind the optional `mcp` feature. Its [maintained repository](https://github.com/modelcontextprotocol/rust-sdk) documents support for the current 2026-07-28 protocol and older initialize-based clients. The [pinned manifest](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.0/crates/rmcp/Cargo.toml) shows the selected server/stdio features, while HTTP transports, OAuth, child-process transports, macros and base64 defaults remain disabled. Rust 1.88 fits Loupe's toolchain. Loupe owns path/preset confinement, read-only cache policy and classifier behavior. Downloads succeeded after Codex restarted with network access; the native interface has passed real stdio and server tests. See [mcp.md](mcp.md).

Extraction normalization uses [chrono-tz 0.10.4](https://docs.rs/chrono-tz/0.10.4/chrono_tz/) with only `std`, alongside the already approved Chrono dependency, and [semver 1.0.28](https://docs.rs/semver/1.0.28/semver/). Chrono-TZ carries IANA timezone rules and exposes ambiguous/nonexistent local times rather than selecting an offset silently; fixed-offset-only parsing would omit requested named zones. Semver follows Cargo's established interpretation, preserving prerelease/build identifiers and rejecting invalid complete versions. Decimal money normalization uses strings and existing parsers, avoiding binary-float rounding and an additional numeric dependency. The registry confirms maintained, non-yanked releases and Rust floors below Loupe's 1.90.
