# kimi-responses-adapter

English | [中文](README_CN.md)

OpenAI Responses API compatibility layer for the Kimi Code Anthropic API.

A deliberately thin, stateless, single-protocol adapter:

```text
Codex
  │ Responses API
  ▼
kimi-responses-adapter       (protocol adaptation only)
  │ Anthropic Messages API
  ▼
api.kimi.com/coding
```

This adapter translates `POST /v1/responses` into Kimi's Anthropic Messages
API and back, without the information loss of a generic Responses↔Anthropic
bridge:

- **Thinking round-trip**: Kimi `thinking` + `signature` blocks are carried in
  Responses `reasoning.encrypted_content` (self-contained payload), so the
  next turn restores them byte-for-byte. Bare-signature
  `encrypted_content` + summary text is also accepted on the way back in.
- **Web search**: Kimi's `web_search_20250305` server tool becomes a Responses
  `web_search_call` item (query + sources), and Kimi's
  `Search results for query: ...` status text blocks are suppressed instead of
  leaking into `output_text`.
- **Function calls**: `function_call` / `function_call_output` ↔ `tool_use` /
  `tool_result`, including incremental `function_call_arguments.delta`.
- **Codex model catalog**: `/v1/models?client_version=...` requests from Codex
  are translated into Codex-specific model metadata; ordinary `/v1/models`
  requests remain raw passthroughs.
- **Everything else is proxied unchanged**: other non-Responses paths
  (`/v1/messages`, `/v1/chat/completions`, ...) are forwarded to the Kimi
  upstream byte-for-byte, including streaming.

The adapter keeps no state and holds no credentials: conversation history
arrives with every request and is deterministically rebuilt into Anthropic
messages, and the client's API key (`Authorization: Bearer ...` or
`x-api-key`) is forwarded to Kimi unchanged on every request. There is
nothing to configure for authentication.

## Endpoints

| Endpoint            | Behavior                                    |
| ------------------- | ------------------------------------------- |
| `POST /v1/responses` | Responses ↔ Anthropic protocol adaptation  |
| `GET /v1/models?client_version=...` | Codex model metadata adaptation |
| `GET /healthz`       | health check                               |
| any other path       | raw passthrough to the Kimi upstream       |

## Supported scope

- `stream=true` and `stream=false`
- `input_text`, `input_image` (data URI or URL)
- function tools, `function_call`, `function_call_output`
- `reasoning` effort → thinking budget; `encrypted_content` ↔ Kimi signature
- `web_search` / `web_search_preview` → `web_search_20250305`
- usage mapping (`input_tokens`, `output_tokens`, cached tokens)
- `max_tokens` stop → `response.incomplete`

Not supported (by design): Chat Completions adaptation, Anthropic-facing
protocol, Gemini, OAuth, multi-provider, users/billing. Replay of historical
Completed `web_search_call` items are replayed into upstream context so
agentic follow-up turns do not repeat the same server-side search.

## Configuration

All configuration is via environment variables:

| Variable                   | Default                                  | Description |
| -------------------------- | ---------------------------------------- | ----------- |
| `LISTEN_ADDR`              | `:8787`                                  | bind address |
| `KIMI_BASE_URL`            | `https://api.kimi.com/coding`            | upstream base URL |
| `KIMI_ANTHROPIC_BETA`      | (empty)                                  | optional `anthropic-beta` header upstream |
| `KIMI_MODEL_MAP`           | `{"codex-auto-review":"kimi-for-coding-highspeed"}` | JSON mappings merged over the defaults; matching keys override defaults |
| `KIMI_CLIENT_SOURCE`       | (empty)                                  | upstream User-Agent override; empty passes through the client User-Agent |
| `KIMI_MAX_TOKENS`          | `32768`                                  | fallback `max_tokens` when neither the client nor model metadata provides one |
| `KIMI_THINKING_BUDGETS`    | `{"low":4096,"medium":16384,"high":32768}` | effort → budget; `minimal`/`none` disables thinking |
| `KIMI_SEARCH_STATUS_PREFIX`| `Search results for query:`              | status-text marker to suppress |
| `KIMI_DEBUG_SSE_FILE`      | (empty)                                  | when set, tee raw upstream SSE to this file for debugging |

Thinking budgets are clamped below `max_tokens`. When thinking is enabled,
sampling overrides (`temperature`, `top_p`) are dropped, as Anthropic
requires.

## Model metadata

Per-model limits (context window, max output tokens) are collected from the
Kimi Code upstream instead of being hardcoded: the adapter lazily calls
`GET {KIMI_BASE_URL}/v1/models` (forwarding the current request's
credential), tolerates both `{"data": [...]}` and bare-array shapes, and
caches the result for 10 minutes. Known Kimi Code models are seeded with
built-in defaults so a failed or metadata-less fetch is harmless.

`max_tokens` precedence: client `max_output_tokens` → upstream model
metadata → `KIMI_MAX_TOKENS`. The effective value is logged per request.

## Run

```sh
cargo build --release
./target/release/kimi-responses-adapter
```

Or use the published container image (multi-arch: `linux/amd64`,
`linux/arm64`):

```sh
docker run --rm -p 8787:8787 ghcr.io/jianyun8023/kimi-responses-adapter:latest
```

The adapter is a pure protocol converter and proxy: it does not terminate
authentication. Deploy it on a trusted network segment, not directly on the
public internet.

## Release

Releases are cut by pushing a semver tag. [dist](https://opensource.axo.dev/cargo-dist/)
(see [dist-workspace.toml](dist-workspace.toml)) then builds binaries for
Linux/macOS (amd64 + arm64, Linux statically linked against musl) and Windows
(amd64), publishes archives, shell/PowerShell installers and checksums to
GitHub Releases, and a separate workflow pushes a multi-arch image to GHCR:

```sh
mise run release 0.1.0
```

This task bumps `version` in `Cargo.toml`, commits it, tags `v0.1.0` and
pushes both — dist requires the tag to match the package version, so cutting
a release by hand (`git tag` without the bump) will fail the Release
workflow.

## Development

Toolchain and tasks are managed by [mise](https://mise.jdx.dev)
(`mise install` once, then):

```sh
mise run build   # cargo build
mise run test    # cargo test
mise run lint    # cargo clippy --all-targets -- -D warnings
mise run fmt     # cargo fmt
mise run ci      # fmt --check + clippy + test (what CI runs)
```
