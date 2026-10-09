# Installing Loupe

Release archives include CLI, MCP, terminal review, Parquet support, examples,
shell completions, and a man page. Choose the archive for your platform:

| Platform | Archive for v0.0.1 |
| --- | --- |
| Linux x86_64 | `loupe-0.0.1-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 | `loupe-0.0.1-aarch64-unknown-linux-musl.tar.gz` |
| macOS ARM64 | `loupe-0.0.1-aarch64-apple-darwin.tar.gz` |

Every archive has a matching `.sha256` file attached to the same GitHub release.
Loupe is experimental; this is not a 1.0 release. Windows binaries are unavailable.

Check and unpack the archive, then install the binary in your user directory.
These commands use the Linux x86_64 archive; substitute your target from the table:

```sh
sha256sum -c loupe-0.0.1-x86_64-unknown-linux-musl.tar.gz.sha256
tar -xzf loupe-0.0.1-x86_64-unknown-linux-musl.tar.gz
cd loupe-0.0.1-x86_64-unknown-linux-musl
mkdir -p ~/.local/bin
install -m755 bin/loupe ~/.local/bin/loupe
export PATH="$HOME/.local/bin:$PATH"
loupe --version
```

On macOS, use `shasum -a 256 -c` in place of `sha256sum -c`.

The executable needs no Nix store. Linux binaries are static; macOS binaries link
only to system libraries. Inference still uses a
reachable API server. Install the machine's usual CA certificates, which the HTTP
client loads at initialization; `SSL_CERT_FILE` can select a PEM bundle. For H2O Lightning through a Jev-compatible endpoint:

```sh
mkdir -p ~/.config/loupe
cp examples/mcp.toml ~/.config/loupe/config.toml
export LOUPE_URL=http://127.0.0.1:8015
export LOUPE_PATH=/v1/systemone
export LOUPE_MODEL=h2oai/h2o-lightning-4b
export LOUPE_ENCODER=estimate
printf '%s\n' 'Checkout is down.' 'Heartbeat completed successfully.' \
  | loupe keep -p real-failure
```

Use your own reachable hostname or IP in `LOUPE_URL`. Remote APIs may also need
`LOUPE_API_KEY`; model selection depends on the server. H2O needs no local
weights or tokenizer. Counts are estimates, hard token budgets are unavailable,
and authoritative identity is currently missing, so caching is disabled.
`doctor --json` reports those limitations and currently exits 69 even when
inference is ready. Keep records below the configured 32K state-token limit;
older shim deployments silently truncate oversized state.

For Clef, use `backend = "clef"`, `encoder = "clef-v1"`, and its pinned
model/tokenizer configuration from `examples/config.toml`. Prepare the tokenizer
with `doctor --prefetch` before using exact counts or `--strict`.

For persistent configuration, copy `examples/mcp.toml` to
`~/.config/loupe/config.toml`. It selects a local H2O endpoint and grants MCP access to the
`real-failure` and `severity` presets. File reads remain disabled until you add
explicit path grants. Have your MCP client launch the absolute installed binary
with arguments `["mcp"]`. See [MCP configuration](mcp.md) in the repository.

CLI flags override environment variables, then profile values, then defaults.
`--config FILE` selects another config file; there is no `LOUPE_CONFIG` variable.
The default location respects `XDG_CONFIG_HOME`.

| Variable | Setting |
| --- | --- |
| `LOUPE_URL` | HTTP(S) base URL |
| `LOUPE_PATH` | Request path, normally `/v1/systemone` |
| `LOUPE_API_FORMAT` | `systemone` (default) or `decisions` |
| `LOUPE_MODEL` | Requested model alias |
| `LOUPE_API_KEY` | Bearer API credential |
| `LOUPE_PROFILE` | Configured profile name |
| `LOUPE_CONCURRENCY` | Positive maximum concurrent request count |
| `LOUPE_ENCODER` | `clef-v1`, or `estimate` for Jev profiles |
| `LOUPE_TOKENIZER` | Tokenizer source or local file |
| `LOUPE_TOKENIZER_REVISION` | Pinned tokenizer revision |
| `LOUPE_CACHE_DIR` | Response cache and downloaded tokenizer directory |

Loupe supports System One and OpenAI Decisions request/response formats. Select
the format with `--api-format`, `LOUPE_API_FORMAT`, or `api_format` in a profile
or `[defaults]`. System One remains the default. With no configured path,
System One uses `/v1/systemone` and Decisions uses `/v1/decisions`.
An explicit path, including one inherited from a profile, always wins.

For H2O Lightning through a LiteLLM proxy, replace the example URL with your gateway:

```sh
export LOUPE_URL=https://llm.example.com
export LOUPE_MODEL=h2o-lightning-4b
export LOUPE_API_KEY="$LLM_API_KEY"
export LOUPE_API_FORMAT=decisions
export LOUPE_PATH=/v1/decisions
export LOUPE_ENCODER=estimate
printf '%s\n' 'Checkout is down.' 'Heartbeat completed successfully.' \
  | loupe keep 'Does this describe a service outage?'
```

To use System One, set `LOUPE_API_FORMAT=systemone` and
`LOUPE_PATH=/v1/systemone`. LiteLLM also accepts `/systemone` as an alias.

Presets, commands and Loupe's JSON output retain their System One shapes in both
modes. Decisions converts structured state/instructions to JSON text, includes
Noul criteria in predicate instructions, and preserves question IDs, choice
distributions, original score levels and input/output token usage. Refusals are
unavailable answers, not negative predictions; normal failure policy applies.
Decisions requires 2–255 choices or 2–10 score levels and uses estimated token
counts. It does not use System One's optional HTTP batch endpoint; `--pack`
still sends multiple named questions in one request. Existing profiles with
`encoder = "clef-v1"` need `LOUPE_ENCODER=estimate` when switching formats.

Native Nix packaging uses `nix-build default.nix -A package --no-out-link`.
To rebuild the portable archive from the repository, use
`nix shell -f default.nix scripts.loupe-dist --command loupe-dist`. Both builds use `Cargo.lock`.
The packager checks architecture and linkage and emits deterministic archive
metadata and a SHA256 checksum alongside the archive. Building source still
requires the pinned toolchain and dependencies.

After the first tagged release, Loupe will also get a source-build derivation in
`jpetrucciani/nix`, using a pinned source tag and hash like Ferry and aq.
