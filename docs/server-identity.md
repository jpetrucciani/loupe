# Server identity integration

Each classification run makes a fresh, uncached `GET /v1/metadata?model=ALIAS`. The configured model alias must match `requested_model`. The service should return `Cache-Control: no-store`. A multi-model service needs compatible model-specific discovery before its profile can use this contract. The example below uses Clef's pinned model and tokenizer.

```json
{
  "schema_version": 1,
  "requested_model": "clef-flash",
  "model": "Cloudflare/clef-flash@17f0b0ad64efb65d273590632833508766b2aae6;compute=bf16;quantization=nf4;server=<64 lowercase SHA-256 hex characters>",
  "encoder_identity": "clef-v1@17f0b0ad64efb65d273590632833508766b2aae6;tokenizer=17f0b0ad64efb65d273590632833508766b2aae6;blake3=16066ad27076349236a282e9351f8bab5aa9b33803f12f7ab9dca401909d7273"
}
```

The model identity requires a 40-character lowercase immutable Hugging Face revision, a known compute dtype, an explicit quantization identity (`none` for unquantized weights), and a 64-character lowercase SHA-256 server build identity. The server build digest must bind code, dependencies and every inference setting that can change probabilities, including quantization parameters and kernels. If a server allows edited weights under an unchanged upstream revision, its build identity must bind their content too. Metadata must describe the actually loaded model, rather than merely the requested configuration. Parameter order is normalized to `compute`, `quantization`, `server`. The client refuses incomplete, duplicate or unknown model parameters.

Every single/batch response's `model` field must report the same complete identity. A mid-run change, alias-only response, or malformed identity after successful discovery fails the affected result with exit 69 and disables cache access for the rest of the run. `--no-cache` disables disk access while retaining this live identity check. Both focus phases and all split/window requests use it.

`encoder_identity` must identify the actual encoder version and tokenizer revision/content. The example shows the pinned client's full identity format, but the exact value comes from `doctor --json`. The server must compute its tokenizer digest from the loaded bytes and verify its encoder matches the reference; copying the client value into an unverified server configuration is insufficient. A mismatch disables local exact-encoder caching. Profiles with an unverified encoder use a complete wire-request key and still require authoritative server identity.

404/405/501, inaccessible metadata, invalid JSON, incompatible version, alias mismatch, or incomplete identity disables classification cache reads and writes. Discovery authentication failures retain exit 77. The client never falls back to health's alias/quantization, a trusted profile pin, or an identity saved by a previous run. Doctor reports identity eligibility without opening the disk store; disk locking/format availability is checked only when needed. Cache management itself remains offline.

The implementation and filesystem tests use mathematical identity fixtures and captured response data to verify hashing, validation, transactions and ordering. They do not prove that a live server reports the true identity. Integration qualification still requires actual metadata and full response identities, positive hits with zero submitted usage, and fresh misses after real changes to weights/precision/quantization/build/tokenizer. No HTTP substitute or fabricated live identity closes that gate.

