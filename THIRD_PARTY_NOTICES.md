# Third-party notices

Loupe's project code is MIT licensed. The Clef text encoder in
`crates/loupe-core/src/encoder.rs` ports the prompt constants and the `render`,
`question_options`, and `encode_record` behavior from Cloudflare's Clef release,
pinned to revision `17f0b0ad64efb65d273590632833508766b2aae6`.

The reference release is Apache-2.0 licensed:
[upstream joint_schema_model.py](https://huggingface.co/Cloudflare/clef-flash/blob/17f0b0ad64efb65d273590632833508766b2aae6/joint_schema_model.py).
Apache-2.0 terms apply to those derived encoder portions. The upstream license
text is included in [licenses/Apache-2.0.txt](licenses/Apache-2.0.txt).
The Rust port adds complete-state counting, checked preflight, and
Python-compatible numeric rendering.

The CPU golden requests are handwritten cases. Expected token IDs, spans, and
numeric strings are generated from that pinned source and tokenizer. Revision,
license, and source hashes are recorded in
[fixture provenance](evals/golden/clef-v1/provenance.json).

Cargo.lock and uv.lock identify the Rust and Python dependencies. Model weights
are not included in Loupe's binary distributions; model/tokenizer licenses and
provider terms remain separate from Loupe's license.
