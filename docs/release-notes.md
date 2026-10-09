# Loupe experimental release

Semantic Unix filters and a read-only MCP stdio interface for System One decision
APIs. Includes Boolean filtering, labels, scores, extraction, routing, joins,
dedupe, evaluation, terminal review, and Parquet output.

Binary archives target Linux x86_64, Linux ARM64 and macOS ARM64 and include installation instructions,
examples, shell completions, a man page, and the usage guide. It needs an external
API endpoint; no model weights are included. Windows binaries are unavailable.
Each archive has a matching SHA256 checksum. Ordinary version tags publish regular
releases; tags with a prerelease suffix are marked as prereleases.

The H2O Lightning example uses estimated token counts. Hard token budgets and
exact-count features require Clef. Caching requires authoritative identity
discovery, which the current H2O shim does not provide. Older H2O shim versions
silently truncate state beyond their configured limit; use a deployment with
oversized-state rejection before relying on whole-document decisions.

See the packaged `docs/usage-guide.md` and `INSTALL.md` to get started.
