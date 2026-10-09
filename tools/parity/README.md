# Actual reference inference parity

This harness loads the pinned upstream reference and existing local weights on an explicitly selected device. It compares all option probabilities and deterministic argmax decisions with real HTTP responses. It does not start or reconfigure the server, fetch model weights, or select a GPU automatically.

The separate `pyproject.toml` pins the reference environment to the server-matched torch/transformers versions. It is not part of ordinary encoder tests or the default Nix environment. Lock it with `uv lock --project tools/parity`, then run with `uv run --project tools/parity tools/parity/parity.py --help`.

Supply the actual `/v1/systemone` URL, pinned `joint_schema_model.py`, existing release weight directory, reference device, server precision, report path and explicitly accepted probability/argmax bounds. Use a separately available GPU or an environment where allocating reference weights is authorized.

BF16-to-BF16 comparison uses a maximum difference of 0.02 and minimum agreement of 0.995. Comparisons against a quantized server need separately accepted bounds and task-quality baselines. A passed harness comparison alone does not verify full server or local weight identity. The report records those limits explicitly.

The full inference run is not yet qualified in this checkout. CPU encoder fixtures and live HTTP count parity are separate evidence.
