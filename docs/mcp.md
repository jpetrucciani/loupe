# MCP stdio

Configure your MCP client's stdio server entry with the installed binary and your config:

```json
{
  "command": "/absolute/path/to/loupe",
  "args": [
    "--config",
    "/absolute/path/to/loupe.toml",
    "mcp"
  ]
}
```

The example profile uses H2O Lightning with estimated counts and no local tokenizer.
Authoritative cache identity is unavailable, so inference works with caching disabled.

These are the launch fields to place inside your client's server configuration. Replace both paths with your installed files. The example config permits `real-failure` and `severity`; inline questions also work. File/path inputs require explicit grants. Starting the command in a terminal waits for an MCP client.

Build the optional interface before starting the server:

```sh
cargo build --features mcp
target/debug/loupe --config examples/mcp.toml --profile server mcp
```

Configure a client's stdio command with the absolute binary path and those arguments. Stdout contains newline-delimited MCP messages; diagnostics use stderr. The approved official Rust SDK handles current `server/discover` and older `initialize` lifecycles. EOF stops the server. SIGINT/SIGTERM cancel pending work and exit 130/143, including when the client's stdin pipe remains open.

```toml
[mcp]
presets = ["real-failure", "severity"]
allow_paths = ["/absolute/project/documents"]
max_input_bytes = 16777216
```

Both allowlists default empty. Inline questions and states still work with empty allowlists. Each call supplies exactly one `state` or `path`; paths resolve relative to the server's working directory. Classification also supplies exactly one `question`, `questions` map, or allowlisted `preset`. Explicit `state: null` is preserved for Clef. Jev applies its stricter typed schema. Raw question instructions remain literal; only trusted presets expand templates. File templates additionally require an MCP path grant. Preset content and trust are rechecked before execution; changed presets require a server restart.

Weighted `[[factors]]` presets return a tool error with exit code 64 before inference; these reports use CLI `explain`, and their raw full-schema plans/answers use CLI `tokens`/`ask`. MCP accepts ordinary classification rubrics and explicit typed question maps. It never silently omits a preset's weighted questions.

| Tool | Arguments | Structured result |
|---|---|---|
| `classify` | State/path and question/questions/preset; optional preset `args`, `ensemble`, `min_agreement` | Raw response, policy decisions, ensemble reports, counts, cache status, usage and warnings |
| `ask` | Same as classify | Exactly `{model, answers, usage}` |
| `tokens` | Same request arguments | Actual request and exact/estimated counts, without inference |
| `presets` | Empty object | Only trusted, explicitly allowlisted preset snapshots |
| `find` | Text state/path, `query`, optional positive `take` | Original lines, stable IDs, locations, probabilities and raw tournament history |
| `pick` | Same as find | Choice-marginal ranking; `take` defaults to one |
| `extract` | Same as find, plus `candidates` and optional `normalize` object | Verbatim spans/byte locations, plus deterministic normalized values when requested |

Candidate kinds are `dates`, `money`, `semver`, `emails`, `hostnames`, and `tickers`. Selection uses the shared presence/none gates, bounded tournaments, a 64-round cap, and at most 10,000 source lines/candidates. Empty/absent results have an empty `selected` array. Inline requests and expanded requests must fit `max_input_bytes`; oversized protocol messages close the transport. Questions and model limits are checked before submission, with strict reported-token verification for the exact encoder.

Example `tools/call` arguments:

```json
{"name":"classify","arguments":{"state":"Checkout is down.","preset":"real-failure"}}
```

```json
{"name":"ask","arguments":{"state":"Checkout is down.","questions":{"outage":{"type":"noul","instructions":"Are customers unable to check out?"},"severity":{"type":"score","instructions":"How severe is this incident?","criteria":["Low","Medium","High"]}}}}
```

`mcp --deadline 30s` bounds each inference call, including discovery/retries. `--budget TOKENS` supplies a separate exact input budget to each call, charging every submitted retry and retaining unknown usage as spent. Hard budgets require `clef-v1`. Backend admission is shared across concurrent calls. Cancellation ends a call's work; it does not erase already submitted usage.

Every inference call requires fresh authoritative identity before cache lookup. MCP reads existing valid caches without creating, repairing or updating them; unavailable identity disables caching. Scalar calibrated presets require their fitted execution view and are rejected here. Authentication is fixed by trusted startup configuration; tool arguments cannot change profiles/endpoints or run commands. No route, exec, hooks, review capture, file-writing tools or HTTP listener are exposed. The [filesystem capability rules](mcp-access.md) apply to every path/template read.
