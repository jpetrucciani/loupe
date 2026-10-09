# Interface compatibility checks

The [v0.1 fixture directory](../crates/loupe-cli/tests/compatibility/v0.1) records the current development interface. It provides a reviewable starting point for the 1.0 freeze. It does not declare that freeze or release qualification complete.

| Artifact | Contract checked |
| --- | --- |
| [flags.md](../crates/loupe-cli/tests/compatibility/v0.1/flags.md) | Native long help for every declared command and nested command, with inherited flags and current environment values suppressed. |
| [grammar.json](../crates/loupe-cli/tests/compatibility/v0.1/grammar.json) | Names, aliases, actions, arity, positional indices, enum values, defaults, environment names, argument groups, conflicts and parser switches. Null, false and empty-array argument properties are omitted. |
| [parser.json](../crates/loupe-cli/tests/compatibility/v0.1/parser.json) | Accepted command paths and rejected grammar combinations, including required groups, conditional requirements, old aliases and global flag placement. These are parsing checks; runtime applicability has separate native checks. |
| [parsed-values.json](../crates/loupe-cli/tests/compatibility/v0.1/parsed-values.json) | Resolved raw values at every command level, including bare-option defaults, global quiet/verbosity and the ordered `on` action declarations. Custom typed parsers are invoked. Ambient profile/endpoint environment variables are disabled for these checks. |
| [errors.json](../crates/loupe-cli/tests/compatibility/v0.1/errors.json) | The core error-to-exit mapping, including authentication and interruption. This test checks the mapping without inventing HTTP responses. |
| [offline-exits.json](../crates/loupe-cli/tests/compatibility/v0.1/offline-exits.json) | Native help/version, syntax, configuration, filesystem, UTF-8, runtime applicability, empty-result, budget, deadline and manual-path behavior. A real refused local endpoint is used for failure checks. |
| [live-exits.json](../crates/loupe-cli/tests/compatibility/v0.1/live-exits.json) | Real inference checks for `is` 0/1/2, uncertainty failure 3, uncertain filter absence 1, and shared unit/count envelopes. The same test also sends a joint raw `ask` and validates its exact envelope, requested heads and input-token usage. |
| [actual-response.json](../crates/loupe-cli/tests/compatibility/v0.1/actual-response.json) | A genuine joint Noul/Choice/Score request and response captured from a locally hosted model. The sanitized capture's BLAKE3 digest is checked. The native encoder recomputed its 298-token plan. |
| `request.json`, `ask.json`, `tokens.jsonl` | Typed request/response serialization and byte-identical native offline request planning. `ask` emits exactly `model`, `answers`, `usage`; raw answer objects are preserved. |
| `noul.*`, `choice.*`, `score.*` | Exact JSONL, TSV, plain and annotated bytes emitted by the production writers using the captured response. The original state and unterminated final unit are preserved. These are codec replays, not live model-quality expectations. |
| [mcp-tools.json](../crates/loupe-cli/tests/compatibility/v0.1/mcp-tools.json) | The production `tools/list` catalog, including every input schema, annotation and tool description. The snapshot exists in MCP-enabled test builds; separate native stdio checks exercise negotiation and actual tool calls. |

The public Rust API is internal to this workspace. Fixture versions describe the CLI/protocol contract independently of package version bumps. Changes to these files require reviewing the visible interface differences. During development, an intentional change can update v0.1 after review. At the 1.0 freeze, promote the approved artifacts to a new directory and keep earlier fixtures and version-specific compatibility checks. Later incompatible changes need an explicit contract version and migration instructions.

Ordinary tests only compare files. An explicit update flag regenerates the generated help, grammar, resolved values, error mapping, MCP catalog and writer snapshots:

```sh
# In the existing development environment, with its pinned tokenizer available.
cargo test --locked --offline -p loupe-cli --all-features --bin loupe compatibility_
cargo test --locked --offline -p loupe-cli --all-features --test compatibility_contracts

# Developer operation, followed by reviewing changed artifacts and rerunning normally.
LOUPE_UPDATE_CONTRACTS=1 cargo test --locked --offline -p loupe-cli --all-features --bin loupe compatibility_

# Real backend checks stay opt-in.
LOUPE_TEST_URL=http://127.0.0.1:8015 cargo test --locked --offline -p loupe-cli --all-features \
  --test compatibility_contracts -- --ignored
```

The authored parser and exit matrices, original response capture, and independently recomputed token plan are not rewritten by this flag. Replacing a captured response requires another genuine capture and a matching provenance digest. Do not edit probabilities to make a test pass.

The shared fixtures cover the ordinary typed API and decision envelopes. They do not claim exhaustive schemas for every optional provenance branch, evaluation report, recovery frame or command-specific output. Their existing per-command tests and documentation remain applicable; the comprehensive schema/freeze review remains open. Authentication exit 77 is pinned by the real error mapping, but a live authenticated-provider qualification is separate. Signal delivery, crash recovery and broken-pipe behavior are exercised by their existing native process contracts. No hosted CI, cross-platform artifact or authoritative cache identity is inferred from these local tests.

The global quiet/verbose regression is rejected before configuration loading or action execution, including when flags occur at different command levels. The native process test deliberately uses a missing configuration and an executable action declaration to verify that precedence. Clap handles declaration-level conflicts; Loupe validates the final resolved globals as well. See [Clap's global argument contract](https://docs.rs/clap/4.6.7/clap/struct.Arg.html#method.global).
