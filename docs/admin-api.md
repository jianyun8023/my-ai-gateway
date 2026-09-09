# ProviderPreset 与模型发现 Admin API

本页记录 Issue #13 提供的管理契约。所有接口都要求现有 Admin Key 鉴权（`Authorization: Bearer $GATEWAY_ADMIN_KEY`），并要求配置 `DATABASE_URL`。

模型发现只管理 ProviderPreset、Source 与 SourceModel。它不会自动创建 LogicalModel、ModelBinding 或 Route，也不会修改 `/v1/models` 或运行时路由。

## Virtual Key 查看与复制

`POST /admin/keys` 和 `POST /admin/keys/:id/rotate` 创建的 Key 使用 SHA-256 哈希完成数据面鉴权，同时用 `GATEWAY_CREDENTIAL_MASTER_KEY` 加密保存恢复副本。列表及普通详情只返回 `key_prefix` 和 `key_recoverable`，不会返回 Key、hash 或 ciphertext。

`GET /admin/keys/:id/value` 只接受独立 Admin Key，并返回 `{"data":{"id":1,"key":"..."}}`。旧的 hash-only 行返回 `409 key_not_recoverable`，需轮换后查看。未配置 master key 时创建、轮换或查看均 fail closed；管理端通过该接口提供显式查看和复制。

每次查看都会写入 `virtual_key.reveal` 元数据审计事件；审计记录、日志和控制面导出均不会包含 Key 明文或加密恢复副本。

### 上游账号凭据轮换

`POST /admin/accounts/:id/credentials/rotate` 使用独立 Admin Key，将账号已有密文重新加密到当前主密钥版本。服务在控制面事务中锁定账号、写入密文、校验并构建候选快照，提交成功后发布运行时快照。成功响应为 `{"data":{"account_id":"...","key_version":"..."}}`，不返回凭据。

账号不存在时返回 `404 not_found`；账号没有密文时返回 `422 no_ciphertext`；解密或快照校验失败返回相应 422 错误；数据库写入失败返回错误，不会被忽略为成功。写入或候选快照失败时保留原密文与已发布快照。

### 控制台轮换

系统设置中的轮换操作调用 `POST /admin/keys/:id/rotate`，提交 `overlap_secs` 和 `allowed_models`。重叠期为 0–86400 的整数秒，控制台默认 3600 秒；0 表示旧密钥立即失效。模型白名单预填旧密钥的值，修改只影响新密钥，空数组表示不限制模型。

接口返回 `old_id`、`new_id`、`key_prefix`、`key` 和 `overlap_until`，控制台展示新密钥供显式复制，并刷新列表。重叠期不会延长旧密钥原有的 `expires_at`；旧密钥仍按原模型权限工作。未提交的名称、权限范围和到期时间沿用已有轮换规则。

列表中的 `replaced_by_id`、`overlap_until`、`expires_at` 用于展示重叠期和失效状态；已轮换、已过期、禁用或撤销的密钥不可再次轮换。状态显示不替代服务端鉴权，轮换错误保留在表单内供处理后重试。

## 客户端归因（Client Source）

数据面请求可通过 `X-Client-Source` header 上报客户端来源；这是网关与下游 SDK 之间的协议约定，客户端 SDK 默认应带上该 header，以便用量统计和 Request Events 能准确归因。

当客户端未发送 `X-Client-Source` 时，网关按以下优先级推导 `usage_events.client_source`：

1. 显式 `X-Client-Source` header（非空）；
2. 鉴权身份默认值；
3. `User-Agent` 推导（已知 SDK/CLI 产品映射）；
4. `"unknown"`。

Virtual Key 鉴权时，默认使用 key 的 `name`；若 `name` 为空则回退到 `key_prefix`。静态 `GATEWAY_API_KEY` 鉴权时默认为 `"static_api_key"`。显式 header 始终优先于鉴权身份和 User-Agent 推导。

## 内置预设

每个内置 ProviderPreset 以不可变 `(id, version)` 保存。`deepseek@3` 与
`minimax@3` 在 Responses 协议声明已验证的 `web_search`，并显式声明
`web_search_citations` 与 `web_search_sources` 不支持；`kimi_code@3` 在 Responses Adapter
声明 `tool_streaming`、`web_search_citations` 与 `web_search_sources` 支持。`@1` 和 `@2`
记录继续可读取，已有 Source 快照不会被启动时注册新版本改写。创建 Source 时省略版本会选择
该 Provider 的最新版本。

`GET /admin/provider-presets` 返回数据库中的不可变版本列表。启动时注册以下首批版本：

| Preset | 默认 Base URL | Chat | Responses | Messages | Discovery |
| --- | --- | --- | --- | --- | --- |
| `deepseek@1` | `https://api.deepseek.com` | `/chat/completions` native | `/responses` native | `/anthropic/v1/messages` native | `GET /models` |
| `minimax@1` | `https://api.minimax.io` | `/v1/chat/completions` native | `/v1/responses` native | `/anthropic/v1/messages` native | `GET /v1/models` |
| `kimi_code@1`-`@3` | `https://api.kimi.com/coding` | `/v1/chat/completions` native | `/v1/messages` via `kimi_responses_adapter`（历史快照，#157 已退役） | `/v1/messages` native | 显式 `unsupported` |
| `kimi_code@4` | `https://api.kimi.com/coding` | `/v1/chat/completions` native | `/v1/responses` native | `/v1/messages` native | 显式 `unsupported` |

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

当前 `kimi_code@4` 预设的 Kimi Responses 返回 `protocol=openai_responses`、`upstream_protocol=openai_responses`、`mode=native`。上游非 2xx、超时或连接失败也返回持久化后的结构化结果，`status=failed`，错误消息为固定脱敏文本；不会读取或保存完整失败正文。

连接测试会产生一个最小的真实模型请求，可能消耗少量上游 Token。上游请求超时为 120 秒；账号健康探测复用该连接测试。此时限独立于数据面的 `GATEWAY_SSE_*` 配置。

## 账号健康与主动探测

账号健康的当前状态以 PostgreSQL `accounts` 行为事实来源。每次状态变化同时写入
`account_health_events`，但路由只读取账号当前行，不从事件历史推断状态。状态机如下：

| 状态 | 触发 | 路由资格 |
| --- | --- | --- |
| `unknown` | 没有成功或失败观测，或人工重新启用 | 可用（还没有失败冷却） |
| `healthy` | 路由请求或连接探测成功（2xx） | 可用 |
| `cooling_down` | 失败窗口达到阈值后进入指数退避 | 在 `cooldown_until` 前不可用 |
| `unhealthy` | 窗口内尚未达到阈值，或冷却已到期但还没有成功恢复 | 可用，可再次作为候选 |
| `stale` | `health_updated_at` 达到 `stale_after` | 可用；旧 cooldown 不会永久屏蔽账号 |
| `disabled` | Account 或 Source 被人工停用 | 不可用 |

408、429、5xx 和传输错误默认需在 60 秒窗口内累计 3 次才进入冷却；窗口外重新计数，
首次冷却采用 base，后续冷却指数增长并受最大退避上限约束。阈值和窗口分别由
`GATEWAY_HEALTH_FAILURE_THRESHOLD`、`GATEWAY_HEALTH_FAILURE_WINDOW_SECS` 配置；一次成功
会清零失败窗口、连续失败计数和 cooldown。状态行还保存 `health_source`（`passive`、`probe`、
`manual`、`startup` 或 `unknown`）、`health_updated_at`、最近探测结果和脱敏错误摘要。
所有时间均为 UTC。进程重启后直接读取这些绝对时间戳，不依赖进程内的 `Instant`。

`GET /admin/health` 返回全部账号的 `source`、`updated_at`、`stale`、状态、cooldown
剩余时间和最近探测信息；`GET /admin/health/:account_id` 返回单个账号的相同信息。
两个接口都要求独立的 `GATEWAY_ADMIN_KEY`，不会返回凭据或请求/响应正文。

主动探测复用同一 Source 的 ProviderPreset 最小连接测试，不调用模型 discovery：

```http
POST /admin/accounts/deepseek-main/probe
Content-Type: application/json

{
  "protocol": "openai_responses",
  "model": "deepseek-v4-flash",
  "requested_by": "admin-ui"
}
```

也可以使用 `POST /admin/health/probe` 并在正文中提供 `account_id`，或使用
`POST /admin/health/probes` 批量探测（省略 `account_ids` 时探测所有启用且有有效预设的账号）。
成功或失败的连接测试都会更新账号健康；模型 discovery 的失败只写入
`source_discovery_runs`，不会改变账号路由健康。探测请求只使用数据库中绑定的 Source
endpoint，经过同一 URL allowlist、DNS、重定向和凭据策略，客户端不能提交任意上游 URL。

后台探测默认启用，每个账号按 `GATEWAY_HEALTH_PROBE_INTERVAL_SECS` 周期执行；可用
`GATEWAY_HEALTH_PROBE_ENABLED=false` 关闭，或用 `GATEWAY_HEALTH_PROBE_ON_STARTUP=true`
在启动时立即执行一次。冷却期间仍按该周期执行半开探测，成功可提前关闭 cooldown；失败不会
在同一 cooldown 内继续放大退避。无可用 fallback 且 429 携带不超过 2 秒的 `Retry-After`
时，同一账号最多重试一次；更长或非法的值不会让请求阻塞等待。固定首选仍保持优先。

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

### SourceModel 协议能力

`GET /admin/sources/:source_id/models/:upstream_model_id/capabilities` 返回该来源模型逐协议的 `source_model_capabilities` 记录；未声明的协议不出现在列表中。

```http
PUT /admin/sources/deepseek-primary/models/deepseek-v4-flash/capabilities/openai_chat_completions
Content-Type: application/json

{
  "status": "confirmed",
  "mode": "native"
}
```

`PUT .../capabilities/:protocol` 按 `(source_id, upstream_model_id, protocol)` upsert 能力声明，字段为 `status`（`pending/confirmed/unavailable`）、`mode`（`unknown/native/adapter/unsupported`），`mode=adapter` 时必须附带 `source_protocol` 和 `adapter`，且来源协议已确认为 `native`。`feature_capabilities` 缺省时从 SourceModel metadata 推导，缺失或非法值的 feature 保持 unknown，不会被猜测为支持。写入与 runtime snapshot 发布在同一事务完成，返回标准 mutation envelope（含 `snapshot_revision`）。`unknown` 模式不能被确认，ModelBinding 只接受 `confirmed` 且 `native/adapter` 的能力。

## DB runtime 有效能力矩阵

`GET /admin/capabilities` 返回当前已原子发布的 PostgreSQL runtime snapshot 有效能力矩阵，并要求 Admin Key 鉴权。它与 proxy、`/admin/routes/{protocol}/{model}` 和 `/v1/models` 读取同一个不可变 snapshot；不会读取 `GatewayConfig.routes` 作为回退，也不会让初始化 JSON 覆盖运行时事实。

响应使用稳定的 `v1` 类型化契约，并通过 `fact_source=runtime_snapshot`、`snapshot_revision` 和 `snapshot_generated_at` 明确事实来源。每行按 Route、Source、Account、logical model 和 upstream model 聚合；primary 与 fallback Binding 都会输出。每行固定包含 `openai_chat_completions`、`openai_responses`、`anthropic_messages` 三个协议单元。以下示例为节省篇幅只展开一个协议单元，实际响应固定返回三项。

```json
{
  "version": "v1",
  "fact_source": "runtime_snapshot",
  "snapshot_revision": 42,
  "snapshot_generated_at": "2026-08-31T08:00:00Z",
  "data": [
    {
      "route_id": "kimi-responses-native",
      "source": {
        "source_id": "kimi-code-primary",
        "display_name": "Kimi Code"
      },
      "account": {
        "account_id": "kimi-main",
        "display_name": "Kimi 主账号",
        "enabled": true
      },
      "model": "kimi-for-coding-highspeed",
      "model_display_name": "Kimi for Coding Highspeed",
      "upstream_model_id": "kimi-for-coding",
      "protocols": [
        {
          "protocol_in": "openai_responses",
          "status": "routable",
          "binding_id": 17,
          "selection": "primary",
          "selection_rank": 0,
          "protocol_upstream": "openai_responses",
          "endpoint": "https://api.kimi.com/coding/v1/responses",
          "mode": "native",
          "adapter": null,
          "conversion_chain": [
            {
              "protocol_from": "openai_responses",
              "protocol_to": "openai_responses",
              "mode": "native",
              "adapter": null
            }
          ],
          "effective_capabilities": {
            "streaming": "native",
            "tools": "native",
            "tool_streaming": "native",
            "thinking": "native",
            "web_search": "native",
            "file_search": "unknown",
            "vision": "unknown",
            "usage": "native"
          },
          "degraded": false,
          "degraded_features": [],
          "allow_lossy_conversion": false,
          "error": null
        }
      ]
    }
  ]
}
```

`routable` 单元的 `conversion_chain` 完整描述入口协议到上游协议的一次直接步骤；native 路径也显式包含一个同协议步骤。`selection=primary|fallback` 与 `selection_rank` 来自 RouteResolver 的真实 Binding 顺序。`endpoint` 只由受控 Source Base URL 与协议 endpoint 组合，不读取账号凭据。

无法路由的单元使用 `status=unroutable`，将未知的 `binding_id`、`selection`、`protocol_upstream`、`endpoint`、`mode`、`adapter` 和 `allow_lossy_conversion` 设为 `null`，转换链设为空，并返回 `{code,message,route_id}` 结构化 `error`。`route_not_found` 表示该 logical model + 协议没有已发布 Route；`runtime_binding_not_available` 表示该 Route 可由其他 Binding 解析，但本行 Source/Account/upstream model 在此协议没有 confirmed、available Binding。功能能力在无法确认时全部显式为 `unsupported`。

`SourceModelCapability` 的 `pending`、`unknown`、`unsupported`、不可用或未确认状态不会进入 runtime snapshot。接口不会把这些缺失事实猜成 `native`、`adapter` 或 `unsupported`，而是保留 `mode=null` 的不可路由单元。缺 endpoint、未知 Adapter、非直接转换链或未经允许的 lossy 能力会在控制面事务构建候选 snapshot 时返回结构化校验错误；失败候选不会替换当前有效 snapshot。

## 运行事件统一查询（Issue #110）

`GET /admin/events` 返回 PostgreSQL 统一读模型。它不会把既有事实复制到 `system_events`：系统生命周期、配置/snapshot、数据库与凭据异常来自 `system_events`；Admin/运维、健康、发现分别投影 `audit_logs`、`account_health_events`、`source_discovery_runs`；请求只投影 `usage_events` 中失败、fallback 或 degraded 的行。普通成功请求仍只在 `/admin/usage/events`（Request Events）中查询。

支持以下可组合参数：

- `from` 或 `since`：RFC3339 下界，二者互斥；`from` 为包含，`since` 为不包含，后者适合轮询；
- `to`：RFC3339 不包含上界；
- `category=lifecycle|configuration|database|security|request|health|operation|admin|discovery`；
- `level=info|warning|error`、精确 `event_type`、`subject_type`、`subject_id`；
- `correlation_id`，或等价查询别名 `operation_id`；
- `source=system_events|usage_events|account_health_events|audit_logs|source_discovery_runs`；
- `limit=1..500`（默认 100）和服务端返回的不透明 `cursor`。

响应按 `(occurred_at DESC,event_id DESC)` 稳定分页；`event_id` 带事实来源命名空间。示例：

```json
{
  "version": "v1",
  "timezone": "UTC",
  "fact_source": "postgresql_unified_read_model",
  "range": {"from": null, "since": "2026-09-09T00:00:00+00:00", "to": null, "boundary": "(since,to)"},
  "data": [{
    "event_id": "system:42",
    "occurred_at": "2026-09-09T00:01:00Z",
    "category": "configuration",
    "event_type": "runtime.snapshot_switched",
    "level": "info",
    "subject_type": "runtime_snapshot",
    "subject_id": "18",
    "correlation_id": "admin-request-id",
    "message": "Runtime snapshot switched",
    "details": {"previous_revision": 17, "candidate_revision": 18},
    "source": "system_events"
  }],
  "page": {"limit": 100, "has_more": false, "next_cursor": null}
}
```

所有 `details` 都是 metadata-only，禁止包含 prompt/response 正文、Authorization、API Key、credential 或可逆正文编码。返回边界会过滤所有来源的敏感文本、主体、关联字段及嵌套 metadata，不能依赖历史写入器已经完成脱敏；任意调用方可控且非必要的 audit actor 与 discovery requested_by 无论格式如何都固定返回 `[REDACTED]`，不靠已知 Key 前缀猜测。请求事件 ID 使用请求标识的摘要，分页游标不携带原始请求标识。统一读模型用于时间线与关联检索；当前账号状态、请求详情和运维任务控制仍以各自专用接口为准。后台任务可用 `operation_id` + `since` 增量轮询，但完成、取消和 retry 不引入推送回调或 DB signal。

查询缺表、权限或解码错误仍返回 `events_query_failed`，不会生成 `database.connection_failed`。连接事件仅记录共享 SQLx 分类确认的连接/连接池故障；incident 按组件独立去重和恢复，对应组件后续操作成功才关闭自己的 incident，其他组件的失败不会被吞掉，成功也不代表该组件恢复。连接 incident 的失败登记不会为了写诊断再次同步等待已耗尽的连接池；成功恢复后才补写配对事件。

## 数据保留与恢复运维（Issue #53）

以下接口使用与其他 Admin API 相同的 `Authorization: Bearer $GATEWAY_ADMIN_KEY` 鉴权，所有时间和 cut-off 都是 UTC。它们不读取或返回 prompt/response 正文、Authorization、API Key 或凭据值。

### 保留策略

`GET /admin/retention/policies` 返回五个独立策略：`usage_events`（logical UsageEvent）、`usage_attempts`（UsageAttempt）、`audit`（`audit_logs`、连接测试和 `account_health_events` 历史）、`discovery`（`source_discovery_runs`）和 `system_events`（窄系统事件）。每项包含 `retention_days`、`enabled` 和 `updated_at`。

`PUT /admin/retention/policies` 接受以下任一形式：

```json
{
  "policies": [
    {"policy_key":"usage_events","retention_days":90,"enabled":true},
    {"policy_key":"usage_attempts","retention_days":90,"enabled":true},
    {"policy_key":"audit","retention_days":365,"enabled":true},
    {"policy_key":"discovery","retention_days":365,"enabled":true},
    {"policy_key":"system_events","retention_days":365,"enabled":true}
  ],
  "requested_by":"admin-ui"
}
```

也支持单个 `{ "policy_key": "audit", "retention_days": 365 }` 或按 key 的对象映射。策略更新会写入 `audit_logs`。

### 清理运行

`POST /admin/retention/cleanup` 启动或继续一个清理运行。请求字段：

- `dry_run`（默认 `false`）；
- `batch_size`（`1..10000`，默认 `500`）；
- `max_batches`（`1..100000`，默认 `1000`）；
- `operation_id`（可选但建议由调度器提供，重复提交保持幂等）；
- `policy_keys`（可选，只运行指定策略）；
- `requested_by`（可选审计主体）。

dry-run 只统计候选，不删除数据。正式清理按 attempt → logical event → audit → discovery → system event 的顺序分批提交；logical event 若仍有未到期 attempt 会延后删除。响应的 `data` 包含每类 scanned/deleted（包括 `scanned_system_events` / `deleted_system_events`）、`batches_completed`、固定 `cutoff_snapshot` 和 `progress`。`status=running` 表示本次达到 `max_batches`，可用相同 `operation_id` 继续。

`GET /admin/retention/cleanup/{operation_id}` 查询进度；`POST /admin/retention/cleanup/{operation_id}/cancel` 设置取消标志并在当前批次结束后转为 `cancelled`；`POST /admin/retention/cleanup/{operation_id}/retry` 可恢复失败、取消或进程中断的运行。每个生命周期事件都写入 `audit_logs`，重试不会重复删除已提交的行。

`GET /admin/retention/cleanup?limit=100`（`/admin/retention/runs` 同义入口）按创建时间倒序列出最近运行，便于调度器发现仍为 `running` 的任务。

`GET /admin/audit?operation_id=...&limit=100` 返回脱敏运维审计；`GET /admin/backups?limit=100` 列出最近备份/恢复运行，`GET /admin/backups/{backup_id}` 返回单次操作状态、checksum、`schema_version` 和 `migration_version`。

### 控制面导出与恢复

`GET /admin/control-plane/export` 返回可保存为 JSON 的脱敏控制面快照，响应带 `Content-Disposition: attachment` 和对 `data` 载荷计算的 `checksum`。内容包括 Provider/Model preset、Source、Account、SourceModel、能力、LogicalModel、Binding、Route、schema/migration 版本和 runtime snapshot fingerprint，不包括 usage 历史。账号凭据只保留：

- `credential_env` Secret 名称和 `credential: {"kind":"secret_ref","name":"..."}`；
- 无 Secret 引用但存在加密字段时的 `{"kind":"redacted"}` 占位；
- Virtual Key 元数据（不含 `key_hash` 或 `key_ciphertext`，恢复时报告 `skipped_virtual_keys`）。

`POST /admin/control-plane/import` 接受导出 JSON，或 `{ "data": <export>, "replace": true, "requested_by": "..." }` 包装。非空目标必须显式 `replace=true`。导入按 FK 顺序恢复并重置 serial sequence；提交后重新构建 snapshot，只有 fingerprint 与导出一致才返回 `verified=true` 和新的 `snapshot_revision`。目标环境必须自行注入导出中列出的 Secret。

`GET /admin/ops/schema`（`/admin/schema` 为同义入口）返回当前 `schema_version`、`migration_version`、应用版本和 UTC 更新时间。网关启动时会顺序应用仓库中的迁移；当前版本为 24，`migrations/0024_system_events.sql` 增加窄系统事件表、第五类 retention policy 及清理计数。

完整的 PostgreSQL `pg_dump`、新库恢复、Docker Compose 和本地 CLI 步骤见 [`docs/operations.md`](./operations.md)。物理 dump 可能包含数据库内的加密凭据和全部历史，必须按高敏感备份保护；脱敏迁移请使用控制面 JSON 导出。

矩阵聚合直接复用真实 `RouteResolver`，不复制选择算法。Adapter 翻译或显式允许的能力损失会列入 `degraded_features`，`degraded` 只在实际发生翻译或损失时为 `true`。响应不包含 `credential_env`、加密/明文凭据、Authorization、API Key 或请求/响应正文。控制台的“能力矩阵”页面展示该响应。

## 错误与安全

HTTP 错误统一使用：

```json
{"error":{"code":"invalid_catalog_state","type":"invalid_catalog_state","message":"..."}}
```

常见状态：`401 unauthorized`、`404 not_found`、`409 catalog_conflict`、`422 invalid_*`、`503 database_unavailable`。Admin 请求正文限制为 1 MiB，上游模型列表读取限制为 2 MiB。

日志和审计禁止记录 Authorization、API Key 和完整响应正文。上游失败只记录错误类别、HTTP 状态、延迟和不含 URL/响应内容的固定消息。

本流程只借鉴 New API 的接入向导、模型同步差异和确认交互语义；未复制其 AGPL 代码、页面或运行时结构。
