# my-ai-gateway

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./assets/keeper-logo-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="./assets/keeper-logo-light.svg" />
    <img src="./assets/keeper-logo-light.svg" alt="Keeper" width="560" />
  </picture>
</p>

Rust AI 网关 MVP，目标是将多个上游账号统一为一个入口，并提供 OpenAI Chat Completions、OpenAI Responses、Anthropic Messages（Claude/Coze 客户端兼容面）入口。

当前版本完成：

- Axum HTTP 服务与 `/healthz`、`/v1/models`。
- 三类协议入口，按 `protocol + model` 解析路由。
- Provider 原生协议直接透传，支持自定义 endpoint、能力矩阵和凭据引用。
- 上游返回的 JSON/SSE 响应头和响应体流式转发；429/5xx 可进入 fallback 账号。
- Provider、Account、Route 配置抽象。
- 精确路由优先：为协议+模型绑定的账号优先于默认启用账号。
- Kimi Responses 适配器已作为 workspace crate 内置，路由使用 `kimi_responses_adapter` 时直接在进程内转换。
- 设置 `DATABASE_URL` 后自动初始化 PostgreSQL 的用量、控制面、模型目录表和 Usage 查询索引；模型目录目前是仓储基线，尚未替换现有运行时 Route。
- PostgreSQL-backed Virtual Key：创建、列表、撤销、模型白名单鉴权。
- 管理接口：`/admin/keys`、`/admin/keys/:id/revoke`，以及 `/admin/usage/summary|timeseries|breakdown|events|export`；`/admin/usage/aggregate` 保留为一次获取三类聚合的组合入口。

工具链由 [Mise](https://mise.jdx.dev/) 管理（Rust 1.97.1 + Node 24，见 [`mise.toml`](./mise.toml)）：

```bash
mise install      # 安装 Rust 与 Node 工具链
mise run install  # 安装 web/ 前端锁定依赖
```

运行：

```bash
cargo run
curl http://127.0.0.1:8787/healthz
```

常用任务：`mise run dev`（网关 + Vite 开发环境）、`mise run build`、`mise run test`、`mise run lint`、`mise run verify`（完整门禁）。

设置 `GATEWAY_ADMIN_KEY` 后可创建下游 Virtual Key，原始 Key 只在创建响应中返回：

```bash
curl -X POST http://127.0.0.1:8787/admin/keys \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"service-a","allowed_models":["MiniMax-M2.7"]}'
```

Usage API 返回显式的 `version: "v1"` 和 `timezone: "UTC"`。所有入口共享 `from`、`to`（RFC3339、半开区间 `[from,to)`）、`logical_model`、`upstream_model`、`provider`、`source`、`account`、`protocol_in`、`protocol_upstream`、`virtual_key`、`status=success|failure`、`status_code` 和 `usage_source` 组合筛选。`source` 当前对应下游 `X-Client-Source`；Virtual Key 鉴权的请求会记录 Key ID，静态 `GATEWAY_API_KEY` 请求为 `null`。

```bash
curl 'http://127.0.0.1:8787/admin/usage/timeseries?from=2026-08-01T00:00:00Z&to=2026-09-01T00:00:00Z&granularity=day&logical_model=MiniMax-M2.7' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"

curl 'http://127.0.0.1:8787/admin/usage/breakdown?breakdown=provider&usage_source=upstream' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"

curl 'http://127.0.0.1:8787/admin/usage/export?format=csv&status=failure' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" -o usage-events.csv
```

`events` 固定按 `(created_at DESC, request_id DESC)` 排序，`limit` 为 `1..500`；后续页应原样传回响应中的 `page.next_cursor`。Summary、timeseries 和 breakdown 的 Token 只累计每个逻辑请求的最终 Usage，不会因 fallback 重复；`upstream_attempts` 单独统计关联的上游尝试。CSV/JSON 导出复用完全相同的筛选与排序，且事件契约不包含 prompt/response 正文。

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

需要执行模型目录 PostgreSQL 集成测试时，显式设置专用的 `TEST_DATABASE_URL`；测试不会复用运行时 `DATABASE_URL`。

设置 `GATEWAY_API_KEY` 后，三类协议入口会要求 `Authorization: Bearer ...` 或 `x-api-key`。Kimi Responses 路由只需配置 `"adapter":"kimi_responses_adapter"`，不需要启动额外服务；账号凭据通过 `credential_env` 注入。之后客户端仍然只需要调用网关：

```bash
curl http://127.0.0.1:8787/v1/responses \
  -H "Authorization: Bearer $GATEWAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"kimi-for-coding-highspeed","input":"hello","stream":true}'
```
