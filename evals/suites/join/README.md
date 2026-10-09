# Catalog join seed suite

Eight fictional, handwritten catalogs contain 48 fully labeled pairs. Each catalog is one fixed split unit. Four repeated label-graph strata put one duplicate, abbreviation, candidate-cap and negative catalog into each partition. Records use only `name` and `city`; labels remain outside requests. The records describe fictional organizations, with MIT provenance, and have not been human reviewed or ratified.

The written identity rule is name plus city, allowing the formatting and abbreviations specified in each case's generic context. Different business types or different cities are different entities. Context contains normalization rules, never labels. Deliberate blocked positives have no shared normalized field tokens or lose the two-candidate cap to negative records. Duplicate records refer to the same organization and exercise right claims and reuse.

```bash
loupe --config examples/config.toml --profile server eval \
  --suite join --strict --no-cache --warmup 0 --json --report /tmp/loupe-join-report
```

The current server server needs explicit `--allow-unverified` for provisional diagnostics. Offline planning uses `--dry-run` instead. The committed split artifact is validated on every run. This seed suite exercises native stage measurements, not the 400-pair target, a production quality floor, or human baseline acceptance. See [join evaluation](../../../docs/join-evaluation.md).
