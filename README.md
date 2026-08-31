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
- PostgreSQL 是控制面与运行时路由的事实来源；启动在一致性事务中加载 Source、Account、LogicalModel、ModelBinding、SourceModelCapability 和 Route，并按单调 `snapshot_revision` 原子发布不可变 snapshot。
- PostgreSQL-backed Virtual Key：创建、列表、撤销、模型白名单鉴权。
- 内置、版本化的 DeepSeek、MiniMax、Kimi Code ProviderPreset 和 ModelPreset；Source 创建时复制不可变快照，预设升级只展示差异。
- 按协议连接测试、模型发现、稳定 `added/changed/missing` 差异、待确认列表、用户编辑和批量确认 API；发现结果不会自动创建 LogicalModel、Binding 或 Route，失败信息和日志均不包含凭据或完整响应正文。
- Source、Account、LogicalModel、ModelBinding、Route 管理 API 支持创建、查询、更新、启停和删除；有效写入会在同一事务内完成校验与下一版 snapshot 构建，失败不会留下坏行。
- 管理接口还包括 `/admin/keys`、`/admin/keys/:id/revoke`、`/admin/provider-presets`、`/admin/sources/*`、基于当前 DB runtime snapshot 的有效能力矩阵 `/admin/capabilities`，以及 `/admin/usage/summary|timeseries|breakdown|events|export`；`/admin/usage/aggregate` 保留为一次获取三类聚合的组合入口。

工具链由 [Mise](https://mise.jdx.dev/) 管理（Rust 1.97.1 + Node 24，见 [`mise.toml`](./mise.toml)）：

```bash
mise install      # 安装 Rust 与 Node 工具链
mise run install  # 安装 web/ 前端锁定依赖
```

运行（`DATABASE_URL` 是 DB-first 运行时的必填项）：

```bash
export DATABASE_URL='postgres://gateway:gateway@127.0.0.1:5432/gateway'
cargo run
curl http://127.0.0.1:8787/healthz
```

空控制面首次启动时，可通过 `GATEWAY_CONFIG_JSON` 一次性初始化。控制面已有任意管理数据后，后续启动不会解析或覆盖该 JSON；此时运行时直接加载数据库 snapshot。需要显式替换现有开发控制面时，同时设置 `GATEWAY_CONFIG_IMPORT=true`，该操作会在事务中清理并重新导入 Source/Account/模型/Binding/Route，因此只应在明确需要导入时使用。监听地址独立使用 `GATEWAY_LISTEN_ADDR`。

常用任务：`mise run dev`（网关 + Vite 开发环境）、`mise run build`、`mise run test`、`mise run lint`、`mise run verify`（完整门禁）。

设置 `GATEWAY_ADMIN_KEY` 后可创建下游 Virtual Key，原始 Key 只在创建响应中返回：

```bash
curl -X POST http://127.0.0.1:8787/admin/keys \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"service-a","allowed_models":["MiniMax-M2.7"]}'
```

Usage API 返回显式的 `version: "v1"` 和 `timezone: "UTC"`。所有入口共享 `from`、`to`（RFC3339、半开区间 `[from,to)`）、`logical_model`、`upstream_model`、`provider`、`source_id`、`client_source`、`account`、`protocol_in`、`protocol_upstream`、`virtual_key`、`status=success|failure`、`status_code` 和 `usage_source` 组合筛选。`source_id` 是 DB-first Runtime Binding 最终实际选中的一等 Source；可选下游 `X-Client-Source` 只记录为独立 `client_source`，缺省为 `unknown`。Virtual Key 鉴权的请求会记录 Key ID，静态 `GATEWAY_API_KEY` 请求为 `null`。

ProviderPreset、连接测试、模型发现和确认接口的完整请求/响应契约见 [`docs/admin-api.md`](./docs/admin-api.md)。最小流程为：创建 Source 快照 → 选择关联且启用的 Account 按协议测试 → 执行 discovery → 查看 diff/待确认模型 → 编辑并批量确认。确认 SourceModel 仍不会自动创建 LogicalModel、Binding 或 Route。

```bash
curl 'http://127.0.0.1:8787/admin/usage/timeseries?from=2026-08-01T00:00:00Z&to=2026-09-01T00:00:00Z&granularity=day&logical_model=MiniMax-M2.7' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"

curl 'http://127.0.0.1:8787/admin/usage/breakdown?breakdown=source_id&usage_source=upstream' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"

curl 'http://127.0.0.1:8787/admin/usage/export?format=csv&status=failure' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" -o usage-events.csv
```

`events` 固定按 `(created_at DESC, request_id DESC)` 排序，`limit` 为 `1..500`；后续页应原样传回响应中的 `page.next_cursor`。逻辑事件的 `source_id` 对应成功 attempt，全部失败时对应最终实际 attempt；attempt 明细也独立携带 `source_id`。Summary、timeseries 和 breakdown 的 Token 只累计每个逻辑请求的最终 Usage，不会因 fallback 重复；`upstream_attempts` 单独统计关联的上游尝试。CSV/JSON 导出复用完全相同的筛选与排序，显式区分 `source_id`/`client_source`，且事件契约不包含 prompt/response 正文。

可通过 `GATEWAY_CONFIG_JSON` 初始化多个 Source、账号和固定路由（示例）：

完整的 MiniMax、DeepSeek、Kimi 三 Provider 示例见 [`config.example.json`](./config.example.json)。

```bash
export GATEWAY_CONFIG_JSON='{
  "listen_addr":"127.0.0.1:8787",
  "providers":[{"id":"minimax","name":"MiniMax","base_url":"https://your-minimax-endpoint","models":["MiniMax-M2.7"],"native_protocols":["openai_chat_completions","openai_responses","anthropic_messages"],"endpoints":{"openai_chat_completions":"/v1/chat/completions","openai_responses":"/v1/responses","anthropic_messages":"/v1/messages"},"capabilities":{"streaming":true,"tools":true,"thinking":true,"web_search":true,"usage":true}}],
  "accounts":[{"id":"minimax-01","provider_id":"minimax","display_name":"primary","credential_env":"MINIMAX_API_KEY","enabled":true},{"id":"minimax-02","provider_id":"minimax","display_name":"backup","credential_env":"MINIMAX_API_KEY_2","enabled":true}],
  "routes":[{"id":"minimax-all","model":"MiniMax-M2.7","provider_id":"minimax","protocols":["openai_chat_completions","openai_responses","anthropic_messages"],"primary_account_id":"minimax-01","fallback_accounts":["minimax-02"],"mode":"native"}]
}'
export DATABASE_URL='postgres://gateway:gateway@127.0.0.1:5432/gateway'
cargo run
```

该文件是初始化/显式导入输入，不是每次启动同步源。

控制面资源路径如下；单资源路径支持 `GET`、`PUT`、`DELETE`，集合路径支持 `GET`、`POST`，启停使用 `PUT .../{id}/enabled` 与 `{"enabled":true|false}`：

```text
/admin/sources
/admin/accounts
/admin/logical-models
/admin/model-bindings
/admin/routes
```

所有错误使用 `{"error":{"code":"...","message":"..."}}`；Account 响应只返回 `credential_env` 和 `credential_configured`，不会返回 `credential_ciphertext` 或明文凭据。启用 Route 时，每个协议必须已有 confirmed/available Binding；Binding 的 endpoint、Adapter 方向、单段转换、SourceModelCapability 和引用完整性会在保存事务内校验。写入响应与 `/healthz` 会返回单调 `snapshot_revision`，用于避免并发写入完成顺序与内存发布顺序不一致。

需要执行 PostgreSQL 集成测试时，显式设置专用的 `TEST_DATABASE_URL`；测试为每次运行创建并清理独立 schema，不会复用运行时 `DATABASE_URL`。控制面完整回归是显式 ignored 测试，必须实际运行，不能把缺少数据库导致的跳过作为通过：

```bash
TEST_DATABASE_URL='postgres://gateway:gateway@127.0.0.1:5432/gateway_test' \
  cargo test postgres_db_first_crud_rollback_snapshot_and_models_contract -- --ignored
```

设置 `GATEWAY_API_KEY` 后，三类协议入口会要求 `Authorization: Bearer ...` 或 `x-api-key`。Kimi Responses 路由只需配置 `"adapter":"kimi_responses_adapter"`，不需要启动额外服务；账号凭据通过 `credential_env` 注入。之后客户端仍然只需要调用网关：

```bash
curl http://127.0.0.1:8787/v1/responses \
  -H "Authorization: Bearer $GATEWAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"kimi-for-coding-highspeed","input":"hello","stream":true}'
```
