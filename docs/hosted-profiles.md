# Jev and LiteLLM profiles

Local Clef remains the default. Select `backend = "jev"` explicitly for TypeSafe's schema rules; this defaults the model to `jev-latest` and encoder to `estimate`. Jev cannot claim `clef-v1` exact counts. Choice has at most 255 options and Score has 2–10 levels. State and instructions must be strings, objects or arrays. Score/Noul descriptions use those types; Choice also permits null descriptions. These checks happen before credential lookup and submission, matching the [TypeSafe API reference](https://docs.typesafe.ai/api).

```toml
[profile.jev]
backend = "jev"
base_url = "https://api.typesafe.ai"
path = "/v1/systemone"
model = "jev-latest"
encoder = "estimate"
api_key_env = "TYPESAFE_API_KEY"
max_length = 32768
max_state_tokens = 32768
max_options = 255
max_batch = 1

[profile.litellm]
backend = "jev"
base_url = "https://llm.example.com/typesafe"
path = "/v1/systemone"
model = "jev-latest"
encoder = "estimate"
api_key_env = "LITELLM_API_KEY"
max_length = 32768
max_state_tokens = 32768
max_options = 255
max_batch = 1
```

The listed context limits are configurable ceilings, with estimated counts. They do not reproduce the hosted tokenizer. Advertised limits can lower ceilings, and Jev's 255-option cap cannot be raised by configuration. Unsupported batch discovery falls back to single requests. Use a pinned model name when reproducibility matters, and explicit eval `--compare` for model/quantization changes.

For LiteLLM, keep `/typesafe` on `base_url`. Classification then reaches `/typesafe/v1/systemone`; model discovery reaches `/typesafe/v1/models`. A prefix only on `path` would leave optional discovery at the gateway root. Client authentication uses the LiteLLM virtual key; the gateway holds its TypeSafe upstream key, as documented by [LiteLLM's native pass-through](https://docs.litellm.ai/docs/pass_through/typesafe). This uses the native typed API.

Trusted profile `api_key_cmd = "pass show litellm/loupe"` can replace `api_key_env`. An explicitly supplied key takes precedence. Commands receive EOF on stdin, have a five-second timeout and retain at most 8 KiB per output stream; oversized stdout, invalid UTF-8, empty keys, embedded header newlines and unsuccessful exits fail with exit 77. Owned process groups stop background descendants after success, timeout or cancellation. Credential output is never included in diagnostics. Overall classification deadlines also bound lookup. Project presets cannot supply credential commands.

429/529 and supported transient server errors retry within the five-attempt limit with jitter and reduced admission. Numeric and date `Retry-After` values are honored, with an overall deadline limiting the wait. Unrepresentable clock delays return a service error. Authorization rejection and malformed input are not retried. Redirects are disabled so bearer credentials stay on the configured endpoint.

Estimated profiles reject the hard `--budget` guarantee. `--summary` reports counted versus actual usage and best-effort costs only when trusted `[profile.NAME.pricing]` declares rates. Unreturned usage/output remains explicit. No provider prices are assumed. Cache reads require fresh full model/encoder discovery and remain disabled when the provider cannot supply it. Full encoded-request keys remain conservative across backends; vendor claims about independent Jev fields do not establish Clef independence or justify reusing individual answers from joint requests.

Offline plans need no credentials:

```sh
printf 'Checkout is down.\n' | loupe --profile jev tokens 'Does this describe an outage?'
printf 'Checkout is down.\n' | loupe --profile litellm keep 'Does this describe an outage?' --summary
```

An opt-in real compatibility check covers Noul, Choice and Score, without a replacement server:

```sh
LOUPE_HOSTED_CONFIG=/absolute/path/config.toml LOUPE_HOSTED_PROFILE=jev \
  cargo test -p loupe-cli --test hosted_contracts \
  actual_hosted_endpoint_returns_valid_noul_choice_and_score -- --ignored --exact
```

That check sends a bounded public synthetic example using explicitly configured real credentials. Fresh hosted compatibility and cross-model qualification remain opt-in. After network access returned, server 11435 resolves but refuses TCP connections; its inference compatibility remains unverified. server 8015 accepts fresh real requests.
