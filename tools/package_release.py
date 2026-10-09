"""Package a verified native binary with installation assets and checksums."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import subprocess
import tarfile
import tomllib
from pathlib import Path

PUBLIC_DOCS = (
    "baselines.md",
    "calibration.md",
    "cli-reference.md",
    "compatibility.md",
    "dedupe.md",
    "ensembles.md",
    "eval.md",
    "evaluation-metrics.md",
    "evaluation-suites.md",
    "explain.md",
    "extract.md",
    "failure-policy.md",
    "fallback.md",
    "features.md",
    "find.md",
    "hooks.md",
    "hosted-profiles.md",
    "install.md",
    "join-blocking.md",
    "join-evaluation.md",
    "join.md",
    "json-interfaces.md",
    "json-types.md",
    "mcp-access.md",
    "mcp.md",
    "on-state.md",
    "on.md",
    "optional-dependencies.md",
    "pick.md",
    "recovery.md",
    "release-notes.md",
    "review.md",
    "route-recovery.md",
    "run-summary.md",
    "rust-builds.md",
    "selection-candidates.md",
    "selection-evaluation.md",
    "selection-policy.md",
    "selection-seed-review.md",
    "selection-tournament.md",
    "server-identity.md",
    "signals.md",
    "streaming-dedupe.md",
    "subcommand-examples.md",
    "usage-guide.md",
)

PUBLIC_EXAMPLES = (
    "config.toml",
    "dedupe-ensemble.toml",
    "ensemble-factors.toml",
    "ensemble-features.toml",
    "h2o.toml",
    "incident-factors.toml",
    "join-ensemble.toml",
    "mcp.toml",
    "postmortem-features.toml",
    "selection-ensemble.toml",
)

TARGETS = (
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--target", choices=TARGETS, default=TARGETS[0])
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    content = binary.read_bytes()
    if args.target.endswith("-linux-musl"):
        machine = 183 if args.target.startswith("aarch64-") else 62
        if (
            content[:6] != b"\x7fELF\x02\x01"
            or int.from_bytes(content[18:20], "little") != machine
        ):
            raise SystemExit(f"Expected an ELF64 binary for {args.target}")
        headers = subprocess.run(
            ["readelf", "-l", str(binary)], capture_output=True, text=True, check=True
        ).stdout
        dynamic = subprocess.run(
            ["readelf", "-d", str(binary)], capture_output=True, text=True, check=True
        ).stdout
        if "INTERP" in headers or "(NEEDED)" in dynamic:
            raise SystemExit("Refusing to package a dynamically linked Linux binary")
    else:
        cpu = 0x0100000C if args.target.startswith("aarch64-") else 0x01000007
        if (
            content[:4] != b"\xcf\xfa\xed\xfe"
            or int.from_bytes(content[4:8], "little") != cpu
        ):
            raise SystemExit(f"Expected a Mach-O binary for {args.target}")
        dependencies = subprocess.run(
            ["otool", "-L", str(binary)], capture_output=True, text=True, check=True
        ).stdout
        for line in dependencies.splitlines()[1:]:
            dependency = line.strip().split(" (", 1)[0]
            if not dependency.startswith(("/usr/lib/", "/System/Library/")):
                raise SystemExit(f"Non-system macOS library dependency: {dependency}")
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"][
        "version"
    ]
    observed = subprocess.run(
        [str(binary), "--version"],
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    ).stdout.strip()
    if observed != f"loupe {version}":
        raise SystemExit(
            f"Binary version {observed!r} does not match manifest {version}"
        )
    prefix = f"loupe-{version}-{args.target}"
    files = {
        "bin/loupe": content,
        "README.md": (root / "README.md").read_bytes(),
        "LICENSE": (root / "LICENSE").read_bytes(),
        "THIRD_PARTY_NOTICES.md": (root / "THIRD_PARTY_NOTICES.md").read_bytes(),
        "licenses/Apache-2.0.txt": (root / "licenses/Apache-2.0.txt").read_bytes(),
        "INSTALL.md": (root / "docs/install.md").read_bytes(),
        "mcp.md": (root / "docs/mcp.md").read_bytes(),
    }
    for name in PUBLIC_EXAMPLES:
        files[f"examples/{name}"] = (root / "examples" / name).read_bytes()
    for name in PUBLIC_DOCS:
        files[f"docs/{name}"] = (root / "docs" / name).read_bytes()
    for shell in ["bash", "fish", "zsh"]:
        files[f"completions/loupe.{shell}"] = subprocess.run(
            [str(binary), "completions", shell],
            capture_output=True,
            check=True,
            timeout=30,
        ).stdout
    files["man/loupe.1"] = subprocess.run(
        [str(binary), "man"], capture_output=True, check=True, timeout=30
    ).stdout
    args.out.mkdir(parents=True, exist_ok=True)
    archive = args.out / f"{prefix}.tar.gz"
    with (
        archive.open("wb") as output,
        gzip.GzipFile(fileobj=output, mode="wb", filename="", mtime=0) as compressed,
        tarfile.open(fileobj=compressed, mode="w") as tar,
    ):
        for name, content in sorted(files.items()):
            info = tarfile.TarInfo(f"{prefix}/{name}")
            info.size = len(content)
            info.mode = 0o755 if name == "bin/loupe" else 0o644
            tar.addfile(info, io.BytesIO(content))
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    (args.out / f"{prefix}.tar.gz.sha256").write_text(f"{checksum}  {archive.name}\n")
    print(f"{archive}: {checksum}")


if __name__ == "__main__":
    main()
