# ProviderPreset 与模型发现 Admin API

本页记录 Issue #13 提供的管理契约。所有接口都要求现有 Admin Key 鉴权（`Authorization: Bearer $GATEWAY_ADMIN_KEY`），并要求配置 `DATABASE_URL`。

模型发现只管理 ProviderPreset、Source 与 SourceModel。它不会自动创建 LogicalModel、ModelBinding 或 Route，也不会修改 `/v1/models` 或运行时路由。

## 内置预设

`GET /admin/provider-presets` 返回数据库中的不可变版本列表。启动时注册以下首批版本：

| Preset | 默认 Base URL | Chat | Responses | Messages | Discovery |
| --- | --- | --- | --- | --- | --- |
| `deepseek@1` | `https://api.deepseek.com` | `/chat/completions` native | `/responses` native | `/anthropic/v1/messages` native | `GET /models` |
| `minimax@1` | `https://api.minimax.io` | `/v1/chat/completions` native | `/v1/responses` native | `/anthropic/v1/messages` native | `GET /v1/models` |
| `kimi_code@1` | `https://api.kimi.com/coding` | `/v1/chat/completions` native | `/v1/messages` via `kimi_responses_adapter` | `/v1/messages` native | 显式 `unsupported` |

预设同时包含 Bearer 认证模板、Anthropic 版本 Header、各协议最小测试请求、默认能力和发现解析规则。Kimi Code 未猜测不存在的模型列表 endpoint；模型元数据可继续使用内置 ModelPreset 和用户编辑。

## 创建 Source 快照

`POST /admin/sources`

```json
{
  "id": "deepseek-primary",
  "display_name": "DeepSeek Primary",
  "provider_preset_id": "deepseek",
  "provider_preset_version": 1,
  "base_url": "https://api.deepseek.com",
  "endpoint_overrides": {
    "openai_chat_completions": "/chat/completions"
  }
}
```

`provider_preset_version`、`base_url` 和 `endpoint_overrides` 可省略；省略版本时使用当前最新版本。响应中的 `provider_preset_snapshot` 是创建时的完整副本。以后注册新版本不会更新此字段或 Source 的 Base URL、endpoint、认证和协议能力。网关应用中的该写入由 DB-first 控制面执行，成功响应同时包含单调 `snapshot_revision` 与 `snapshot_generated_at`；校验或 snapshot 构建失败时整个事务回滚。

`GET /admin/sources` 返回 Source 列表。`GET /admin/sources/:source_id/preset-diff` 将创建时 snapshot 与同 ID 的最新预设比较，按 JSON path 稳定返回 `added/changed/missing` 类型；该操作只读。

Source 生命周期还提供 `GET/PUT/DELETE /admin/sources/:source_id` 和 `PUT /admin/sources/:source_id/enabled`。Account、LogicalModel、ModelBinding 与 Route 使用相同的集合 `GET/POST`、单资源 `GET/PUT/DELETE` 和独立 enabled 路径约定；发现确认只更新 SourceModel，仍不会隐式创建这些运行时资源。

连接测试和发现必须选择一个已经关联到该 Source、处于 enabled 状态且配置了 `credential_env` 的 Account。凭据只在进程内从环境变量读取，不在请求响应、审计表或日志中回显。Account/Source 的完整生命周期由 PostgreSQL 控制面 API 管理。

## 按协议连接测试

`POST /admin/sources/:source_id/connection-tests`

```json
{
  "account_id": "deepseek-main",
  "protocol": "openai_responses",
  "model": "deepseek-v4-flash",
  "requested_by": "admin-ui"
}
```

`model` 可省略并使用预设测试模型。响应示例：

```json
{
  "data": {
    "source_id": "deepseek-primary",
    "account_id": "deepseek-main",
    "protocol": "openai_responses",
    "upstream_protocol": "openai_responses",
    "mode": "native",
    "status": "succeeded",
    "http_status": 200,
    "latency_ms": 143,
    "error_code": null,
    "error_message": null,
    "requested_by": "admin-ui",
    "tested_at": "2026-08-31T12:00:00Z"
  }
}
```

Kimi Responses 返回 `protocol=openai_responses`、`upstream_protocol=anthropic_messages`、`mode=adapter`。上游非 2xx、超时或连接失败也返回持久化后的结构化结果，`status=failed`，错误消息为固定脱敏文本；不会读取或保存完整失败正文。

连接测试会产生一个最小的真实模型请求，可能消耗少量上游 Token。

## 模型发现与差异

`POST /admin/sources/:source_id/discoveries`

```json
{
  "account_id": "deepseek-main",
  "requested_by": "admin-ui"
}
```

成功响应的 `data` 包含：

- `run`：Source、Account、ProviderPreset 版本、HTTP 状态、耗时、UTC 起止时间、原始模型列表 snapshot 和模型数；
- `diff.added|changed|missing`：按 `upstream_model_id` 排序，每项列出稳定 `changed_fields`；
- `models`：本次新增、变化或 availability 变化后的 SourceModel。

相同 snapshot 的重复刷新返回空 diff。模型消失时只将 SourceModel 标记为 `unavailable`，不删除记录。发现失败只新增一条 `failed` run，不更新任何 SourceModel；Kimi Code 返回 `unsupported` run。

`GET /admin/sources/:source_id/discoveries/latest` 返回最近 run、可复现 diff 和 `last_discovered_at`。成功 run 的 `raw_snapshot` 是上游模型目录，不包含请求凭据；失败 run 的 snapshot 为 `null`，只包含脱敏错误码和消息。

## 待确认、编辑与批量确认

```text
GET /admin/sources/:source_id/models?confirmation_status=pending&availability_status=available
```

可选查询值：

- `confirmation_status=pending|confirmed`；
- `availability_status=unknown|available|unavailable`。

每个 SourceModel 返回 `metadata` 与逐字段 `field_sources`。来源固定为 `upstream`、`preset`、`user` 或 `unknown`，优先级为 `user > preset > upstream > unknown`。

用户编辑但暂不确认：

```http
PATCH /admin/sources/deepseek-primary/models
Content-Type: application/json

{
  "upstream_model_id": "deepseek-v4-flash",
  "metadata": {
    "logical_model_name": "deepseek-fast",
    "context_window": 1000000,
    "web_search": "unknown"
  }
}
```

事务化批量确认：

```http
POST /admin/sources/deepseek-primary/models/confirm
Content-Type: application/json

{
  "models": [
    {"upstream_model_id": "deepseek-v4-flash", "metadata": {}},
    {"upstream_model_id": "deepseek-v4-pro", "metadata": {"logical_model_name": "deepseek-pro"}}
  ]
}
```

任一模型不存在、不可用或元数据非法时整批回滚。confirmed 模型刷新时保持已确认元数据和匹配预设；pending 模型刷新会重算 upstream/preset 字段，但保留所有 `user` 字段。

## 错误与安全

HTTP 错误统一使用：

```json
{"error":{"code":"invalid_catalog_state","type":"invalid_catalog_state","message":"..."}}
```

常见状态：`401 unauthorized`、`404 not_found`、`409 catalog_conflict`、`422 invalid_*`、`503 database_unavailable`。Admin 请求正文限制为 1 MiB，上游模型列表读取限制为 2 MiB。

日志和审计禁止记录 Authorization、API Key 和完整响应正文。上游失败只记录错误类别、HTTP 状态、延迟和不含 URL/响应内容的固定消息。

本流程只借鉴 New API 的接入向导、模型同步差异和确认交互语义；未复制其 AGPL 代码、页面或运行时结构。
