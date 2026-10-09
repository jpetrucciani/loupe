# Git hooks

Install trusted hooks with the profile you use for classification:

```sh
loupe --config examples/config.toml --profile server hook install
loupe --config examples/config.toml --profile server hook run pre-commit --dry-run
loupe --config examples/config.toml --profile server hook run commit-msg .git/COMMIT_EDITMSG --dry-run
```

Installation resolves the effective hooks directory with `git rev-parse --path-format=absolute --git-path hooks`, including `core.hooksPath`. `--directory DIR` selects an existing directory explicitly. Positional `pre-commit` / `commit-msg` select individual hooks; omission selects both. Existing hooks, symlinks and non-executable files are conflicts unless they are byte-identical executable Loupe hooks. All selected conflicts are checked before creation. To preserve an existing hook, add `loupe hook run pre-commit` or `loupe hook run commit-msg "$1"` to it manually and retain its exit handling.

Each new script is fully written, synced, closed and linked into place without replacement beneath the held directory descriptor. A concurrent creator is never overwritten. Installation can leave already published hooks if a later filesystem operation fails; rerunning accepts unchanged complete scripts. Generated scripts pin the current Loupe executable and explicit global configuration/profile/endpoint options as literal shell arguments. Replacing the executable or changing these options requires inspecting and updating the existing hook. Install does not execute credentials or inference. Hook installation tests use disposable directories and do not modify this repository's Git state.

The hook policy comes from trusted global/XDG configuration or an explicit `--config FILE`:

```toml
[hooks]
budget = 8192
deadline_seconds = 60
fail_open = true
secret = "secret"
secret_threshold = 0.9
breaking_change = "breaking-change"
conventional_commit = "conventional-commit"
```

Pre-commit sends the staged diff to the configured Noul secret rubric. A certain yes at `p >= secret_threshold` blocks with exit 1. Commit-msg sends the staged diff, complete message and first subject line together, asking the Noul breaking-change and Choice conventional-commit questions jointly. Breaking changes without `!` immediately before the subject's colon warn; subjects lacking a conventional `type(scope)!: message` form receive a suggested type. These advisory checks leave the message file untouched and return 0. Model uncertainty exits 3.

Project overrides use the existing content-hash preset trust boundary. Inspect them and grant `loupe presets trust NAME` before installation/execution; editing them revokes the grant. Checks require single-question presets with the expected types. Static context and permission-checked templates are supported. Scalar calibration pins are rejected because hook requests have a different joint view. Model-identity policies still require fresh verification.

`--diff PATCH` on `hook run` reads an existing patch instead of inspecting the Git index. Both patch and message files must be regular UTF-8 files with at most 1 MiB; final symlinks are rejected. Default Git inspection disables external diffs, textconv and optional locks and includes binary patch data. Empty pre-commit diffs succeed without credentials or HTTP. No split/truncation is implicit. The exact complete request must fit the token limits and budget before admission. Hooks require `clef-v1`; estimated hosted counts do not support the hard budget contract.

The overall deadline covers Git children, credential lookup, discovery, retries and inference. Every submitted retry is charged separately. Authentication and service/deadline failures warn and return 0 only with `fail_open=true`; `fail_open=false` retains exit 77/69. Bad trust/schema/configuration, oversized input, exhausted budget and uncertainty fail closed regardless. Runs are uncached and write no response files. Diagnostics exclude input bodies and credential output.

Generated scripts, exact plans, trust changes, actual credential failures and deadline/process cleanup have local filesystem/process coverage. The opt-in actual server test remains pending network access. These checks do not establish secret-detection quality. Git's hook invocation and bypass behavior are described in the [Git hook reference](https://git-scm.com/docs/githooks).
