# Faster Rust CLI builds with Nix

The native package uses different release-profile settings from the portable
binary to reduce compilation work across test executables. Compile and test
times depend on the machine and available dependency cache.

## An inexpensive default for native packages

Loupe's `default.nix` keeps optimization and the full offline tests, but overrides
these two release settings inside the native derivation:

```nix
CARGO_PROFILE_RELEASE_LTO = "off";
CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16";
```

Cargo environment overrides take precedence over the workspace profile. Disabling
cross-crate LTO avoids whole-program optimization for every test executable;
multiple codegen units allow more compiler parallelism. Binary size and runtime
performance may change. Keep aggressive optimization for artifacts where measured
benefits justify it. Loupe's portable `loupe-dist` build still uses the original
thin-LTO release profile. [Cargo profile reference](https://doc.rust-lang.org/cargo/reference/profiles.html).

These profile settings apply inside the native package and do not drop tests,
disable ordinary optimization or share mutable directories across Nix builds.

## Cache compiled dependencies when source changes often

Loupe now uses Crane v0.24.0, pinned by commit and archive hash in `default.nix`.
It keeps the existing Nix environment and separate rust-overlay toolchain pin.
The native build has two derivations:

```text
Cargo manifests/lock + toolchain + features + profile
    -> cargoArtifacts: compiled dependencies and dev-dependencies

cargoArtifacts + actual Rust source + test assets
    -> package: CLI/MCP binary, full offline tests, completions and manual
```

`craneLib.buildDepsOnly packageArgs` creates a minimal workspace from the Cargo
manifests and caches dependency artifacts. `craneLib.buildPackage` consumes those
artifacts, compiles Loupe and executes the real tests. The dependency derivation
compiles dummy workspace crates only to produce the cache; it does not substitute
model responses or run application tests. See [Crane's API](https://crane.dev/API.html#cranelibbuilddepsonly).

Both steps share `packageArgs`: the exact toolchain, lockfile, features, native
inputs and profile settings. Their Cargo commands use `--locked --offline` and
the `mcp,parquet` features; default review remains enabled. The full package keeps
the pinned tokenizer fixture and install checks. Changing Rust application source
should change `package` while retaining `cargoArtifacts`. Dependency, toolchain,
feature and profile changes can invalidate the dependency cache.

```sh
# Compile/cache dependencies separately, or let the package build do it.
nix-build default.nix -A cargoArtifacts --no-out-link
nix-build default.nix -A package --no-out-link

# Inspect the dependency derivation identity without building it.
nix-instantiate default.nix -A cargoArtifacts
```

The first build pays the dependency compilation cost. Subsequent source edits
reuse that output instead of recompiling every crate. Tests still run for each
new application build. A repeated unchanged package build is a Nix cache hit,
not evidence of faster compilation. Local Cargo's `target/` remains independent.
An authorized binary cache can distribute these Nix outputs to other machines;
configure cache access separately from the package expression.

## Keep rebuild inputs small

Filter package source to actual build/test assets. Exclude logs, generated reports,
local caches and ordinary prose docs unless the build consumes them. Include
compile-time fixtures and runtime test assets explicitly; excluding these can
make a sandbox build fail even when local Cargo succeeds. Loupe's package fileset
already follows this approach.

Pin the Rust toolchain and Nix inputs. Changing compiler, target, features or flags
creates a different artifact set. Keep native and musl artifacts separate, and
avoid large feature matrices until they serve a real consumer. For daily local
edits, use the persistent development shell and normal debug Cargo builds.

## Measure before adding another layer

Retain the full Nix log and distinguish downloads, dependency compilation, final
linking, test compilation and test execution. Compare cold compilation with cold
compilation; repeated `nix-build` of an unchanged realized package mostly measures
cache lookup. Do not call that a compiler speedup.

If compilation is already short but tests take longer, inspect the tests. If
application edits rebuild a large unchanged dependency graph, dependency artifact
caching is the next investment. A fresh dependency/profile/toolchain change still
needs a cold build. [Nixpkgs Rust packaging](https://nixos.org/manual/nixpkgs/stable/#rust).
