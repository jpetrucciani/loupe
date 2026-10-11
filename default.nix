{ pkgs ? import
    (fetchTarball {
      # nixup: pin=jpetrucciani/nix;
      name = "jpetrucciani-2026-10-05";
      url = "https://github.com/jpetrucciani/nix/archive/4f94c9782a1f70934e979416cf751b00a53518b3.tar.gz";
      sha256 = "084ldz360yk9v5pcmx7hgfksysgz0kgbcf9l4pwhgmarjdwdwgxv";
    })
    { overlays = [ _rust ]; }
, _rust ? import
    (fetchTarball {
      name = "oxalica-2026-10-05";
      url = "https://github.com/oxalica/rust-overlay/archive/dbc715a4b7c0ace63b9769a032d1dd34cd89e5bd.tar.gz";
      sha256 = "0hffw791ixrgdm1wgvlkmkpnnyhd589lj8klsbcl9dvls3xqbwfb";
    })
, craneSrc ? fetchTarball {
    name = "crane-0.24.0";
    url = "https://github.com/ipetkov/crane/archive/692f7e9ef2ece8125b466f66f2af532b3edaed0d.tar.gz";
    sha256 = "06y3dvs9ms4c70r2ia50k77650yrihc09jibqbqrzly23dn42s4m";
  }
}:
let
  name = "loupe";
  target =
    if pkgs.stdenv.hostPlatform.isDarwin
    then pkgs.stdenv.hostPlatform.rust.rustcTarget
    else if pkgs.stdenv.hostPlatform.isAarch64
    then "aarch64-unknown-linux-musl"
    else "x86_64-unknown-linux-musl";
  rust = pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
    extensions = [ "rust-src" "rustc-dev" "rust-analyzer" ];
    targets = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ target ];
  });
  craneLib = (import craneSrc { inherit pkgs; }).overrideToolchain rust;
  version = (pkgs.lib.importTOML ./Cargo.toml).workspace.package.version;
  modelRevision = "17f0b0ad64efb65d273590632833508766b2aae6";
  tokenizer = pkgs.fetchurl {
    url = "https://huggingface.co/Cloudflare/clef-flash/resolve/${modelRevision}/tokenizer.json";
    hash = "sha256-BrlQk1LSr1A4GrIkfgg7gNMtXAq6kcJyyp/3Kbag5SM=";
  };
  referenceSource = pkgs.fetchurl {
    url = "https://huggingface.co/Cloudflare/clef-flash/resolve/${modelRevision}/joint_schema_model.py";
    hash = "sha256-DjBM98ZQDou1m+9+Kv0sY3P4JZbfs7V9Gqk8F14tw6M=";
  };
  pythonEnv = pkgs.uv-nix.mkEnv {
    name = "loupe-reference-tools";
    python = pkgs.python313;
    workspaceRoot = pkgs.lib.fileset.toSource {
      root = ./.;
      fileset = pkgs.lib.fileset.unions [ ./pyproject.toml ./uv.lock ];
    };
    gitignore = false;
  };
  packageArgs = {
    pname = name;
    inherit version;
    src = pkgs.lib.fileset.toSource {
      root = ./.;
      fileset = pkgs.lib.fileset.unions [
        ./Cargo.toml
        ./Cargo.lock
        ./LICENSE
        ./THIRD_PARTY_NOTICES.md
        ./licenses
        ./crates
        ./evals/golden
        ./evals/suites
        ./examples
      ];
    };
    cargoExtraArgs = "--locked --offline --features mcp,parquet";
    # Cross-crate LTO repeats for every test executable; favor native rebuilds.
    CARGO_PROFILE_RELEASE_LTO = "off";
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16";
    nativeBuildInputs = [ pkgs.cmake pkgs.installShellFiles pkgs.python3 ];
    LOUPE_TEST_TOKENIZER = tokenizer;
    LOUPE_TOKENIZER = tokenizer;
    RUST_TEST_THREADS = "1";
    SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  };
  cargoArtifacts = craneLib.buildDepsOnly packageArgs;
  package = craneLib.buildPackage (packageArgs // {
    inherit cargoArtifacts;
    postInstall = ''
      install -Dm644 LICENSE "$out/share/doc/loupe/LICENSE"
      install -Dm644 THIRD_PARTY_NOTICES.md "$out/share/doc/loupe/THIRD_PARTY_NOTICES.md"
      install -Dm644 licenses/Apache-2.0.txt "$out/share/doc/loupe/licenses/Apache-2.0.txt"
      installShellCompletion --cmd loupe \
        --bash <("$out/bin/loupe" completions bash) \
        --zsh <("$out/bin/loupe" completions zsh) \
        --fish <("$out/bin/loupe" completions fish)
      "$out/bin/loupe" man >loupe.1
      installManPage loupe.1
    '';
    doInstallCheck = true;
    installCheckPhase = ''
      runHook preInstallCheck
      "$out/bin/loupe" --version | grep -Fx 'loupe ${version}'
      "$out/bin/loupe" --help >/dev/null
      test -s "$out/share/bash-completion/completions/loupe.bash"
      test -s "$out/share/man/man1/loupe.1.gz"
      runHook postInstallCheck
    '';
    meta = {
      description = "Semantic Unix filters for System One decision models";
      homepage = "https://github.com/jpetrucciani/loupe";
      license = [ pkgs.lib.licenses.mit pkgs.lib.licenses.asl20 ];
      mainProgram = name;
      platforms = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    };
  });

  envVars = {
    NIXUP = "0.0.15";
  };
  tools = with pkgs; {
    cli = [
      jfmt
      nixup
    ];
    rust = [
      cargo-zigbuild
      rust
      stdenv.cc
      pkg-config
      cmake
      openssl
    ];
    python = [ uv pythonEnv ];
    scripts = pkgs.lib.attrsets.attrValues scripts;
  };

  scripts = {
    loupe-dist = pkgs.pog {
      name = "loupe-dist";
      description = "Build and package the CLI and MCP server for the current platform";
      runtimeInputs = [ rust pkgs.cmake pkgs.python3 pkgs.stdenv.cc ]
        ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.cargo-zigbuild pkgs.zig pkgs.binutils ]
        ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.darwin.cctools ];
      script = ''
        set -euo pipefail
        export RUSTFLAGS="''${RUSTFLAGS-} --remap-path-prefix=$HOME=/build/user --remap-path-prefix=$PWD=/build/loupe"
        cargo ${if pkgs.stdenv.hostPlatform.isDarwin then "build" else "zigbuild"} --locked --release --target ${target} --features mcp,parquet
        python tools/package_release.py --target ${target} --binary target/${target}/release/loupe --out dist
      '';
    };
    loupe-check = pkgs.pog {
      name = "loupe-check";
      description = "Format, lint, and test Loupe using the pinned Rust toolchain";
      runtimeInputs = [ rust pkgs.stdenv.cc pkgs.pkg-config pkgs.cmake pkgs.python3 ];
      script = ''
        set -euo pipefail
        export LOUPE_TEST_TOKENIZER=${tokenizer}
        export LOUPE_TOKENIZER=${tokenizer}
        export RUST_TEST_THREADS=1
        cargo fmt --all --check
        cargo clippy --locked --all --benches --tests --examples --all-features
        python -m unittest tools.test_package_release
        cargo test --locked "$@"
      '';
    };
    loupe-golden = pkgs.pog {
      name = "loupe-golden";
      description = "Regenerate full encoder fixtures from pinned CPU-only Python tooling";
      runtimeInputs = [ pythonEnv ];
      script = ''
        set -euo pipefail
        python tools/golden.py --source ${referenceSource} --tokenizer ${tokenizer} "$@"
      '';
    };
    loupe-live = pkgs.pog {
      name = "loupe-live";
      description = "Run classification contracts against an explicitly selected live server";
      runtimeInputs = [ rust pkgs.stdenv.cc pkgs.pkg-config ];
      script = ''
        set -euo pipefail
        : "''${LOUPE_TEST_URL:?Set LOUPE_TEST_URL to the actual server base URL}"
        export LOUPE_TEST_TOKENIZER=${tokenizer}
        export LOUPE_TOKENIZER=${tokenizer}
        cargo test --workspace -- --ignored --test-threads=1 "$@"
      '';
    };
  };
  checkShell = pkgs.mkShell {
    name = "loupe-check-shell";
    packages = [ scripts.loupe-check ];
  };
  distShell = pkgs.mkShell {
    name = "loupe-dist-shell";
    packages = [ scripts.loupe-dist ];
  };
  paths = pkgs.lib.flatten [ (builtins.attrValues tools) ];
  env = pkgs.buildEnv {
    inherit name paths; buildInputs = paths;
  };
in
(env.overrideAttrs (old: {
  inherit name;
  env = (old.env or { }) // envVars;
})) // { inherit scripts package cargoArtifacts tokenizer referenceSource pythonEnv checkShell distShell; }
