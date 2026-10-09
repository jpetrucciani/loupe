# loupe

Semantic Unix filters for text. Ask a decision model a question, then filter,
label, score, or select the original input. Use Loupe in shell pipelines or expose
its read-only tools to an assistant over MCP.

```sh
printf '%s\n' 'Checkout is down.' 'Heartbeat completed successfully.' \
  | loupe keep 'Does this report an unresolved outage?'
```

Example output:

```text
Checkout is down.
```

Loupe calls an external decision API; it does not run a model locally. System One
is the default wire format. OpenAI Decisions format is also supported through
`--api-format decisions` or `LOUPE_API_FORMAT=decisions`.

## Install

Release archives target **Linux x86_64, Linux ARM64, and macOS ARM64** and include
CLI, MCP, terminal review, Parquet support, shell completions, and a man page.
No Nix installation or model weights are needed to run it.

Download the archive and matching checksum from [GitHub Releases](https://github.com/jpetrucciani/loupe/releases), then:

```sh
sha256sum -c loupe-0.0.1-x86_64-unknown-linux-musl.tar.gz.sha256
tar -xzf loupe-0.0.1-x86_64-unknown-linux-musl.tar.gz
mkdir -p ~/.local/bin
install -m755 loupe-0.0.1-x86_64-unknown-linux-musl/bin/loupe ~/.local/bin/loupe
export PATH="$HOME/.local/bin:$PATH"
loupe --version
```

The commands above install the Linux x86_64 archive. Choose the archive for your
platform in [installation](docs/install.md). Loupe is experimental; Windows
binaries are not available.

## Connect your API

Create a config for your endpoint. This example selects the Jev-compatible H2O
Lightning shim; replace the address and model with your deployment:

```toml
# loupe.toml
[profile.my-api]
backend = "jev"
base_url = "http://127.0.0.1:8015"
path = "/v1/systemone"
model = "h2oai/h2o-lightning-4b"
encoder = "estimate"
concurrency = 2
```

```sh
printf '%s\n' 'Checkout is down.' 'Heartbeat completed successfully.' \
  | loupe --config loupe.toml keep -p real-failure
```

Use `LOUPE_API_KEY` if your server requires a bearer key. CLI flags and environment
variables can override the endpoint and model; see the [configuration reference](docs/install.md).
For a persistent default, put your config at `$XDG_CONFIG_HOME/loupe/config.toml`
(or `~/.config/loupe/config.toml` when XDG_CONFIG_HOME is unset).

Wire format and backend schema are separate settings. For OpenAI Decisions,
select `--api-format decisions`; Loupe defaults its request path to `/v1/decisions`
when no path is explicitly configured. Override the path if your gateway uses a
different route. With the explicit System One path above, also pass
`--path /v1/decisions` when switching formats. See [hosted profiles](docs/hosted-profiles.md)
for backend schema and gateway configuration.
For Clef's exact encoder, use the pinned settings in [examples/config.toml](examples/config.toml).

## What can I do with it?

Commands below assume your default config is set. Otherwise add
`--config loupe.toml` before the command.

| Job | Example |
| --- | --- |
| Branch on a whole-document decision | `loupe is 'Does this report customer impact?' incident.txt` |
| Keep or drop matching lines | `loupe keep -p real-failure events.log` / `loupe drop -p real-failure events.log` |
| Label and count alerts | `loupe tag -p severity events.log --json` / `loupe count -p severity events.log` |
| Score and sort review comments | `loupe rate -p actionable comments.txt` / `loupe sort -p actionable comments.txt --reverse` |
| Find evidence or rank candidates | `loupe find 'Which line describes the outage?' events.log` / `loupe pick 'Which comments request changes?' comments.txt -n 2` |
| Extract a value from its source | `loupe extract 'Which version introduced the regression?' release.txt --candidates semver --normalize --json` |
| Ask several questions or make a feature table | `loupe ask -f questions.toml report.txt` / `loupe featurize -f questions.toml events.log -I lines -O csv` |
| Explain a weighted rubric | `loupe explain -f examples/incident-factors.toml incident.txt --json` |
| Match tables or consolidate duplicates | `loupe join customers.jsonl accounts.jsonl --on name,location --json` / `loupe dedupe < reports.txt` |
| Route events into queues | `loupe route -p severity events.log --out queues` |
| Act on a stream | `tail -F app.log \| loupe on -p severity --when page --append pages.log` |
| Inspect reusable questions | `loupe presets list` / `loupe presets show severity` |
| Plan or inspect connectivity | `loupe tokens -p severity events.log` / `loupe doctor --json` |
| Enable Bash tab completion | `source <(loupe completions bash)` |

The [subcommand cookbook](docs/subcommand-examples.md) has copyable examples and
sample inputs for every command, including review, hooks, cache management,
evaluation, and calibration. The [usage guide](docs/usage-guide.md) explains
formats, uncertainty, and recovery. Use `loupe COMMAND --help` or `loupe man`
for the full grammar.

Questions can be positional text, a preset (`-p NAME`), or a TOML/JSON file
(`-f FILE`). No input files means stdin. `-I` selects input units and `-O` selects
output format. Per-unit `--json` output is JSONL with the original unit, raw answer,
model, and cache status. `keep` and `drop` preserve original matching bytes.
`is` exits 0 for yes, 1 for no, and 2 for uncertain.

## MCP

Add preset grants to your config:

```toml
[mcp]
presets = ["real-failure", "severity"]
allow_paths = []
```

Configure your MCP client to launch the installed binary:

```json
{
  "command": "/absolute/path/to/loupe",
  "args": ["--config", "/absolute/path/to/loupe.toml", "mcp"]
}
```

The server exposes classification, structured questions, token planning, presets,
line ranking, and extraction over stdio. Preset and file access require explicit
grants. It exposes no route, exec, or file-writing tools.
See [MCP setup and tool schemas](docs/mcp.md).

## Current limits

- Predictions depend on the model and rubric. Inspect probabilities and handle
  uncertainty before automating important decisions.
- H2O/Jev token counts are estimates. Exact counting, hard token budgets, Git
  hooks, and token-aligned truncation require the Clef encoder.
- Cache reuse requires fresh authoritative model/encoder identity. The current
  H2O shim does not supply it, so caching is disabled. `doctor` can exit 69 for
  missing identity even when inference works.
- Unverified H2O evaluation/calibration runs require `--allow-unverified` and
  cannot ratify baselines or publish fitted policies.
- API limits apply to the complete prompt, including questions. Configure client
  ceilings for your server; the supplied H2O profiles use conservative estimates.
  Whole-document splitting requires an explicit combine rule and changes the
  question being answered.
- Input is sent to your configured API. Requested caches, journals, review labels,
  and evaluation reports can contain original text; keep them private.

Detailed behavior is in the [CLI reference](docs/cli-reference.md) and
[failure/recovery guide](docs/failure-policy.md).

## Development and releases

The pinned Nix environment includes the toolchain and helper commands. Without
Nix, building requires Rust 1.90 or newer plus the native build tools used by the
dependencies.

```sh
# Build CLI, MCP, review, and Parquet support.
nix shell -f default.nix --command cargo build --features mcp,parquet

# Match CI: format, full-feature Clippy, and offline tests.
nix shell -f default.nix --command loupe-check -- --workspace --all-features -- --test-threads=1

# Build the current platform's archive and checksum in dist/.
nix shell -f default.nix scripts.loupe-dist --command loupe-dist
```

[Offline CI](.github/workflows/check.yml) also regenerates the pinned Clef encoder
fixtures and builds the native Nix package. Live inference tests are opt-in.
[Build notes](docs/rust-builds.md) explain Crane dependency reuse and compilation settings.

The [release workflow](.github/workflows/release.yml) runs on pushed `v*` tags.
The tag must match the Cargo version. It runs offline checks, then builds and
smoke-tests each platform on a native runner. Archives and checksums are uploaded
to a draft, which is published after all platforms succeed. Ordinary version tags
become regular releases; versions such as `v0.1.0-alpha.1` become prereleases.
Manual runs must select a version tag. Failed builds leave the draft available for
a retry; published release assets are preserved.
Hosted CI and publishing require a pushed GitHub repository; local builds alone
do not verify that workflow.

The `jpetrucciani/nix` source-build derivation will be added after the first release
so its source tag and hash can be pinned. The repository already exposes a native
package through `nix-build default.nix -A package --no-out-link`.

## License

Loupe's project code is [MIT licensed](LICENSE). Derived Clef encoder portions are
covered by Apache-2.0; see [third-party notices](THIRD_PARTY_NOTICES.md). Model and
provider licenses are separate, and model weights are not redistributed.
