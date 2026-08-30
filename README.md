# my-ai-gateway

Rust AI 网关 MVP，目标是将多个上游账号统一为一个入口，并提供 OpenAI Chat Completions、OpenAI Responses、Anthropic Messages（Claude/Coze 客户端兼容面）入口。

当前版本完成：

- Axum HTTP 服务与 `/healthz`、`/v1/models`。
- 三类协议入口，按 `protocol + model` 解析路由。
- Provider 原生协议直接透传，支持自定义 endpoint、能力矩阵和凭据引用。
- 上游返回的 JSON/SSE 响应头和响应体流式转发；429/5xx 可进入 fallback 账号。
- Provider、Account、Route 配置抽象。
- 精确路由优先：为协议+模型绑定的账号优先于默认启用账号。
- Kimi Responses 适配器已作为 workspace crate 内置，路由使用 `kimi_responses_adapter` 时直接在进程内转换。
- 设置 `DATABASE_URL` 后自动初始化 PostgreSQL 的 `usage_events` 表。

运行：

```bash
cargo run
curl http://127.0.0.1:8787/healthz
```

可通过 `GATEWAY_CONFIG_JSON` 配置多个 provider、账号和固定路由（示例）：

完整的 MiniMax、DeepSeek、Kimi 三 Provider 示例见 [`config.example.json`](./config.example.json)。

```bash
export GATEWAY_CONFIG_JSON='{
  "listen_addr":"127.0.0.1:8787",
  "providers":[{"id":"minimax","name":"MiniMax","base_url":"https://your-minimax-endpoint","models":["MiniMax-M2.7"],"native_protocols":["openai_chat_completions","openai_responses","anthropic_messages"],"endpoints":{"openai_chat_completions":"/v1/chat/completions","openai_responses":"/v1/responses","anthropic_messages":"/v1/messages"},"capabilities":{"streaming":true,"tools":true,"thinking":true,"web_search":true,"usage":true}}],
  "accounts":[{"id":"minimax-01","provider_id":"minimax","display_name":"primary","credential_env":"MINIMAX_API_KEY","enabled":true},{"id":"minimax-02","provider_id":"minimax","display_name":"backup","credential_env":"MINIMAX_API_KEY_2","enabled":true}],
  "routes":[{"id":"minimax-all","model":"MiniMax-M2.7","provider_id":"minimax","protocols":["open_ai_chat_completions","open_ai_responses","anthropic_messages"],"primary_account_id":"minimax-01","fallback_accounts":["minimax-02"],"mode":"native"}]
}'
cargo run
```

设置 `GATEWAY_API_KEY` 后，三类协议入口会要求 `Authorization: Bearer ...` 或 `x-api-key`。Kimi Responses 路由只需配置 `"adapter":"kimi_responses_adapter"`，不需要启动额外服务；账号凭据通过 `credential_env` 注入。之后客户端仍然只需要调用网关：

```bash
curl http://127.0.0.1:8787/v1/responses \
  -H "Authorization: Bearer $GATEWAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"kimi-for-coding-highspeed","input":"hello","stream":true}'
```
