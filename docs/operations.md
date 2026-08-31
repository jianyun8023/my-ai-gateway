# 数据保留、备份与恢复 Runbook

本文对应 Issue #53，适用于本地部署和 `docker compose` 部署。所有控制面运维接口都要求独立的 `GATEWAY_ADMIN_KEY`；数据面 `GATEWAY_API_KEY` 不能调用这些接口。

## 数据边界

网关默认只保存结构化用量和状态，不保存 prompt/response 正文。四类历史分别保留：

| policy_key | 数据表 | 时间列 | 默认保留 |
| --- | --- | --- | --- |
| `usage_events` | `usage_events` | `created_at` | 90 天 |
| `usage_attempts` | `usage_event_attempts` | `created_at` | 90 天 |
| `audit` | `audit_logs`、`source_connection_tests` | `created_at`/`tested_at` | 365 天 |
| `discovery` | `source_discovery_runs` | `completed_at` | 365 天 |

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
        {"policy_key":"discovery","retention_days":365}
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

`GET /admin/audit?operation_id=...` 可查看 started、progress、cancelled、succeeded 或 failed 事件；`GET /admin/backups/:id` 查看导出/恢复状态、校验和及 schema 版本。清理错误会固定为脱敏错误码和消息，不保存 Authorization、API Key 或正文。

## 控制面脱敏导出

控制面 JSON 导出只包含恢复路由 snapshot 所需的配置和目录表，不包含 usage 历史。账号的凭据值和 `credential_ciphertext` 永远不会输出：

- `credential_env` 只作为 Secret 名称保留，并附带 `{"kind":"secret_ref","name":"..."}`；
- 只有加密字段而没有 Secret 名称时输出不可解密的 `redacted` 占位；
- Virtual Key 的不可逆 `key_hash` 不导出，恢复时会计数为 `skipped_virtual_keys`，请在目标库重新签发 Key；
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

控制面 JSON 适合脱敏迁移；完整 PostgreSQL 备份适合灾难恢复，可能包含当前数据库中的加密凭据字段和全部历史，因此必须当作高敏感文件保护。建议使用 `.pgpass` 或 Secret 注入，避免把密码写进命令历史：

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

Compose 只负责 Gateway + PostgreSQL；备份文件应挂载到宿主机受限目录，不写入镜像层：

```bash
docker compose up -d --build
docker compose exec -T postgres pg_dump --format=custom --no-owner --no-acl \
  -U gateway -d gateway > backups/gateway-$(date -u +%Y%m%dT%H%M%SZ).dump
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
