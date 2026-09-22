# 数据保留、备份与恢复 Runbook

本文对应 Issue #53，适用于本地部署和 `docker compose` 部署。Compose 的启动、镜像更新和基础生命周期见 [`docs/deployment.md`](deployment.md)。所有控制面运维接口都要求独立的 `GATEWAY_ADMIN_KEY`；数据面 Virtual Key 和过渡静态 Key 都不能调用这些接口。

## 流式用量结算与停机

带数据库的数据面请求在首次上游请求前预留流式结算名额，最多 64 个并发请求持有名额。非 SSE 响应结束时立即释放；SSE 响应从开始传输到用量与健康结算任务结束才释放。流式数据库写入最多 8 个并发，其余任务占用上述 64 个名额等待，不会生成无界队列。名额耗尽时网关在调用上游前返回 `503 settlement_capacity_exhausted` 并记录警告；调用方可稍后重试。流式用量写入失败最多重试两次，仍失败会记录带 `request_id` 的错误日志。用量事件和 attempts 沿用同一事务及 `request_id` 幂等约束。

收到 SIGINT/SIGTERM 后，HTTP 服务先停止接入并等待现有响应结束，然后等待流式结算名额全部归还，才记录正常停止。长时间未结束的 SSE 或数据库写入会延长停机；强制杀进程、容器终止期限耗尽或数据库持续不可用仍可能造成最后一批结算缺失。应为部署终止宽限期预留响应与结算时间，并检查 `failed to persist streaming usage after retries` 日志。进程内结算任务并非持久化队列，无法跨强杀恢复。

## 数据边界

网关默认只保存结构化用量和状态，不保存 prompt/response 正文。五类策略分别保留：

| policy_key | 数据表 | 时间列 | 默认保留 |
| --- | --- | --- | --- |
| `usage_events` | `usage_events` | `created_at` | 90 天 |
| `usage_attempts` | `usage_event_attempts` | `created_at` | 90 天 |
| `audit` | `audit_logs`、`source_connection_tests`、`account_health_events` | `created_at`/`tested_at` | 365 天 |
| `discovery` | `source_discovery_runs` | `completed_at` | 365 天 |
| `system_events` | `system_events` | `occurred_at` | 365 天 |

策略使用 UTC 的 `retention_days`。`enabled=false` 表示该类不自动删除；`retention_days=0` 只适合明确的测试或紧急清理。清理先删过期 attempt，再删过期 logical event。若 logical event 仍有未到期 attempt，事件会延后删除，避免 `ON DELETE CASCADE` 破坏引用完整性。删除 logical event 时数据库级 cascade 会同时移除已经符合 attempt 策略的子记录。

## 查看和修改策略

```bash
export ADMIN_URL='http://127.0.0.1:8787'
export GATEWAY_ADMIN_KEY='replace-with-admin-secret'

curl "$ADMIN_URL/admin/retention/policies" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"

curl -X PUT "$ADMIN_URL/admin/retention/policies" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"policies":[
        {"policy_key":"usage_events","retention_days":90},
        {"policy_key":"usage_attempts","retention_days":90},
        {"policy_key":"audit","retention_days":365},
        {"policy_key":"discovery","retention_days":365},
        {"policy_key":"system_events","retention_days":365}
      ],"requested_by":"operator"}'
```

策略更新和清理操作都会写入 `audit_logs`。响应固定带 `version: "v1"` 和 `timezone: "UTC"`。

## Dry-run 与分批清理

先执行 dry-run，确认候选数量和固定 cut-off：

```bash
curl -X POST "$ADMIN_URL/admin/retention/cleanup" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"dry_run":true,"batch_size":500,"operation_id":"audit-2026-08-31"}'
```

正式清理使用稳定的 `operation_id`。每次调用最多执行 `max_batches` 个批次；返回 `status=running` 时可重复提交相同请求，cut-off 和策略快照不会改变：

```bash
curl -X POST "$ADMIN_URL/admin/retention/cleanup" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"batch_size":500,"max_batches":20,"operation_id":"cleanup-2026-08-31","requested_by":"operator"}'

curl "$ADMIN_URL/admin/retention/cleanup/cleanup-2026-08-31" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"
```

中断时请求已提交的批次不会回滚到更早批次；每批单独提交，重复执行只会处理剩余行。可以请求取消，当前批次结束后变为 `cancelled`；确认原因后用 retry 继续：

```bash
curl -X POST "$ADMIN_URL/admin/retention/cleanup/cleanup-2026-08-31/cancel" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"
curl -X POST "$ADMIN_URL/admin/retention/cleanup/cleanup-2026-08-31/retry" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"
```

`GET /admin/audit?operation_id=...` 可查看 started、progress、cancelled、succeeded 或 failed 事件；`GET /admin/events?operation_id=...&since=...` 可把同一操作与系统、健康等事实放在统一时间线增量轮询；`GET /admin/backups/:id` 查看导出/恢复状态、校验和及 schema 版本。清理错误会固定为脱敏错误码和消息，不保存 Authorization、API Key 或正文。

## 控制面脱敏导出

控制面 JSON 导出只包含恢复路由 snapshot 所需的配置和目录表，不包含 usage 历史。账号的凭据值和 `credential_ciphertext` 永远不会输出：

- `credential_env` 只作为 Secret 名称保留，并附带 `{"kind":"secret_ref","name":"..."}`；
- 只有加密字段而没有 Secret 名称时输出不可解密的 `redacted` 占位；
- Virtual Key 的 `key_hash` 和恢复用 `key_ciphertext` 都不导出，恢复时会计数为 `skipped_virtual_keys`，请在目标库重新签发 Key；
- Source `auth_config`、预设 snapshot 和所有嵌套敏感字段递归脱敏。

```bash
curl "$ADMIN_URL/admin/control-plane/export" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -o control-plane.json
chmod 600 control-plane.json
```

导出内容带 `schema_version`、`migration_version`、UTC 时间、源 runtime snapshot `revision/generated_at` 和不含监听地址的 fingerprint。不要把导出文件提交 Git 或发送到不受控位置。

## 恢复到新数据库

1. 创建空 PostgreSQL 16 数据库并让网关启动一次，确保所有 migration 已应用。
2. 复制目标环境需要的 Secret（导出文件只包含 `credential_env` 名称）。
3. 在确认目标库为空后导入；非空目标必须显式传 `replace=true`。

```bash
curl -X POST "$ADMIN_URL/admin/control-plane/import" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H 'Content-Type: application/json' \
  --data-binary @<(jq '. + {replace:true, requested_by:"restore-drill"}' control-plane.json)
```

导入按 FK 顺序恢复 Provider/Model preset、Source、Account、SourceModel、能力、LogicalModel、Binding 和 Route，重置 serial sequence，并在提交后重新构建 runtime snapshot。只有 fingerprint 与导出值一致时才返回 `verified=true`；否则记录失败并不发布新的内存 snapshot。可用以下接口确认版本：

```bash
curl "$ADMIN_URL/admin/ops/schema" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"
```

恢复不会带回原始 Virtual Key 值，也不会把任何凭据值写回数据库；目标库中已有的 Virtual Key 会保留。新库恢复后应重新创建下游 Virtual Key 并轮换必要 Secret。

## PostgreSQL 物理备份

控制面 JSON 适合脱敏迁移；完整 PostgreSQL 备份适合灾难恢复，可能包含账号凭据和 Virtual Key 的加密字段及全部历史，因此必须当作高敏感文件保护。建议使用 `.pgpass` 或 Secret 注入，避免把密码写进命令历史：

```bash
umask 077
mkdir -p backups
pg_dump --format=custom --no-owner --no-acl "$DATABASE_URL" \
  > "backups/gateway-$(date -u +%Y%m%dT%H%M%SZ).dump"
sha256sum backups/*.dump > backups/SHA256SUMS
```

恢复演练优先恢复到新库，不要覆盖生产库：

```bash
createdb gateway_restore
pg_restore --no-owner --exit-on-error \
  --dbname "$RESTORE_DATABASE_URL" backups/gateway-<timestamp>.dump
```

恢复后检查 `gateway_schema_metadata`、`gateway_schema_migrations`，调用 `/admin/ops/schema` 和 `/admin/control-plane/export`，比较 runtime fingerprint，再执行 `/healthz` 与一条不带真实凭据的路由检查。物理 restore 的破坏性命令必须由操作者明确确认，并保留源 dump 不变。

## Docker Compose 与本地 CLI

Compose 只负责 Gateway + PostgreSQL；完整启动配置见 [`docs/deployment.md`](deployment.md)。备份文件应写入宿主机受限目录，不写入镜像层：

```bash
umask 077
mkdir -p backups
docker compose --env-file .env.compose up -d --build
docker compose --env-file .env.compose exec -T postgres \
  sh -c 'pg_dump --format=custom --no-owner --no-acl -U "$POSTGRES_USER" -d "$POSTGRES_DB"' \
  > "backups/gateway-$(date -u +%Y%m%dT%H%M%SZ).dump"
```

网关二进制也提供不依赖 HTTP 的 Admin CLI（仍使用 `DATABASE_URL`，输出只含脱敏数据）：

```bash
cargo run -- ops retention-policies
cargo run -- ops retention-cleanup --dry-run --batch-size 500
cargo run -- ops control-plane-export --output control-plane.json
cargo run -- ops control-plane-import --input control-plane.json --replace
```

生产环境可将 `cargo run` 换成构建后的 `my-ai-gateway`。CLI 与 HTTP API 使用相同的数据库契约和审计记录。

仓库还提供了不回显 Secret 的辅助脚本：`scripts/retention-cleanup.sh`、`scripts/control-plane-export.sh`、`scripts/postgres-backup.sh` 和带显式 `--confirm` 的 `scripts/postgres-restore.sh`。脚本不会自动删除或覆盖宽泛路径。

## 检查清单与剩余风险

- 所有时间比较使用 UTC，清理 cut-off 在运行开始时固定；
- 清理批次可中断、重复执行和恢复，`request_id`/attempt 幂等语义不变；
- 控制面导出不包含 prompt/response、Authorization、API Key 或可解密凭据；
- 物理 dump 必须加密存储、限制权限并设置独立保留期；
- 恢复演练不能把测试数据库当作运行时 `DATABASE_URL`；
- 当前凭据信封/统一 Secret Resolver 仍由 Issue #47 负责，JSON 导出只保留 Secret 引用，不能替代 Secret 管理系统。

## Usage 事件 Token 计数语义

`usage_events` 的 token 列遵循以下约定，避免下游计费面板把缓存命中静默低估（Issue #100）：

| 列 | 含义 |
| --- | --- |
| `input_tokens` | 上游报告的新增（未缓存）输入 token。Anthropic `cache_read_input_tokens` 与 `cache_creation_input_tokens` 不计入此列。 |
| `output_tokens` | 上游报告的生成 token。独立计费的 reasoning / thinking token 不计入此列（参见 `reasoning_tokens`）。 |
| `reasoning_tokens` | 思考/推理 token，仅当上游在 usage 中独立报告（OpenAI `output_tokens_details.reasoning_tokens` / Anthropic 风格 `thinking_tokens`）时非零。 |
| `cached_tokens` | 提示缓存命中 token：Anthropic `cache_read_input_tokens` + `cache_creation_input_tokens` 之和。 |
| `total_tokens` | `input_tokens + output_tokens`，**不包含** `cached_tokens` 也不包含 `reasoning_tokens`。 |

**计费口径**：跨厂商聚合需要 `input_tokens + output_tokens + cached_tokens`，把 `reasoning_tokens` 按厂商账单规则单算。下游报表若只读 `total_tokens`，对有缓存命中的长会话会大幅低估。CSV 导出（`/admin/usage/export`）与 JSON（`/admin/usage/events`、`/admin/usage/summary`）的 `total_tokens` 字段都按本约定。

`usage_source` 标记 token 数来源：`upstream` 表示上游 usage 字段直接解析；`parsed` 表示按 SSE 事件顺序合并明确报告的 usage 字段（缺失字段保留，显式零值覆盖）；`estimated` 表示上游未报告，由 tiktoken 对请求体/响应体估算；`missing` 表示请求失败且无可用 usage，或成功 SSE 的估算样本已超限且没有上游 usage。`estimated` 与 `missing` 行的 `total_tokens` 含义同上，但数值仅为粗估，**不可作为计费值**（Issue #98）。

## Usage 事件回退原因

`usage_events.fallback_reason`（migration 0019，Issue #103）记录请求为何由 fallback 账号完成而不是请求的逻辑模型对应的主账号；`NULL` 表示未发生回退（主账号完成或单次尝试）。取值是数据面生成的白名单原因码，用于在 Request Events 中解释“响应模型为什么不是请求的模型”：

| 值 | 含义 |
| --- | --- |
| `account_disabled` | 主账号或 Source 被停用，请求未发出即回退。 |
| `account_cooling_down` | 主账号处于冷却中（如上游 429 用量熔断触发指数退避），请求未发出即回退。 |
| `account_unhealthy` | 主账号冷却过期但连续失败尚未恢复，请求未发出即回退。 |
| `account_unavailable` | 无健康记录或健康读取失败等，请求未发出即回退。 |
| `upstream_http_<status>` | 主账号已尝试但返回 retryable 状态（408/425/429/5xx），如 `upstream_http_429`。 |
| `upstream_transport_error` | 主账号已尝试但连接层失败（超时/连接错误）。 |

查询示例如下：

```sql
SELECT created_at, logical_model, source_id, account_id, upstream_model_id,
       status_code, success, fallback_reason
FROM usage_events
WHERE logical_model = 'MiniMax-M3' AND fallback_reason IS NOT NULL
ORDER BY created_at DESC LIMIT 20;
```


### SSE 观察资源边界与诊断

SSE 按完整事件增量合并 usage，支持跨网络 chunk 和多行 `data:`。每个事件只保留理解的数值计数器，长流末尾的 usage 仍会更新 input/output/cache/reasoning，缺失字段保留、显式零覆盖。

资源上限为代码安全常量，不是环境配置，`GATEWAY_SSE_*` 时间限制设为 `0` 不会禁用它们：

- 单帧原始字节（含行分隔）最多 1 MiB，未完成的 Chat choice 最多 1024 个；超过后返回 `gateway_stream_buffer_limit` 并关闭流。超限帧可能包含末尾 usage，网关不会跳过它并伪报完成；只保留此前完整解析的上游计数。
- 用于缺失 usage 时估算的响应样本最多 256 KiB。超限后仍继续增量提取 usage，但无法取得上游计数时返回 `usage_source=missing` 与零 Token，不用前缀估算整个响应。失败、取消和超时也不凭内容估算 Token。
- 既有 `<think>` reasoning 补充估算改为逐个闭合块处理，单流累计最多处理 256 KiB 的标签内原始字节（含闭合标签），单块缓存不超过 256 KiB，可跨 chunk；超限或未闭合时放弃此补充并记录 `reasoning_estimate_unavailable=true`，已报告的上游 reasoning 不受影响。该启发式仍沿用既有原始流标签语义，不是 Provider 计费数据。

生命周期日志带 `request_id`，只记录终止代码、字节数、样本是否截断、数据事件数、JSON 解析失败数、缺失 usage 事件数和 TTFT。错误分类区分 `gateway_transport_error`（响应体读取/解压失败）、`gateway_incomplete_stream`（EOF 缺少终态）、`gateway_upstream_error`（Provider 错误事件）、`gateway_stream_buffer_limit`（观察资源超限）；空流、取消、连接/首事件/空闲/总超时保留各自代码。错误摘要同步写入请求事件；不记录正文、正文片段、编码预览、指纹或上游错误文本。

资源边界由本地 Mock/单元/Contract 测试覆盖，不代表生产负载、真实 Provider 或生产历史问题复验通过。
