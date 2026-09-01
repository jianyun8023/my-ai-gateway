# Docker Compose 部署

本文是当前仓库的单机部署说明。Compose 负责运行 Gateway 和 PostgreSQL 16；Gateway 镜像在本机构建，数据库数据保存在命名卷中。应用启动时会自动执行内置 migration，并从 PostgreSQL 控制面加载运行时 snapshot。

这套配置适合开发、测试和受控的内部部署。对公网提供服务前，还需要在网关前配置 TLS、访问边界、日志/指标采集、备份和 Secret 管理。

## 准备环境文件

```bash
cp .env.compose.example .env.compose
chmod 600 .env.compose
```

编辑 `.env.compose`，至少替换以下值：

- `POSTGRES_PASSWORD`：数据库密码；它会被 Compose 拼入容器内的 `DATABASE_URL`，应使用 URL-safe 字符（例如十六进制随机值）；
- `GATEWAY_API_KEY`：临时数据面静态入口 Key；
- `GATEWAY_ADMIN_KEY`：独立的 Admin API Key，不能与数据面 Key 或 Provider Key 复用；
- 与 `config.example.json` 中 `credential_env` 对应的 Provider Key。

如果使用 `credential_ciphertext`，还要设置 `GATEWAY_CREDENTIAL_MASTER_KEY` 或 keyring 变量。不要把真实凭据写入 `GATEWAY_CONFIG_JSON`、镜像层或 Git。

## 启动与检查

先校验 Compose 展开结果，再构建并后台启动：

```bash
docker compose --env-file .env.compose config --quiet
docker compose --env-file .env.compose up -d --build
docker compose --env-file .env.compose ps
curl http://127.0.0.1:8787/healthz
```

管理端位于 `http://127.0.0.1:8787/admin/`；健康检查和数据面端口只发布 Gateway，PostgreSQL 仅在 Compose 网络内可访问。默认发布地址是 `127.0.0.1:8787`，可以在 `.env.compose` 中设置 `GATEWAY_BIND_ADDRESS=0.0.0.0`，但应先确认主机防火墙和上游访问边界。

查看日志或停止服务：

```bash
docker compose --env-file .env.compose logs -f gateway
docker compose --env-file .env.compose down
```

`down` 不会删除 PostgreSQL 命名卷。`down -v` 会删除卷中的控制面和用量数据，只能在确认数据不再需要时使用。

## 初始化控制面

空数据库首次启动可以通过环境变量导入配置示例。`GATEWAY_CONFIG_JSON` 是一次性初始化输入，不是每次启动同步源：

```bash
export GATEWAY_CONFIG_JSON="$(<config.example.json)"
docker compose --env-file .env.compose up -d --build
unset GATEWAY_CONFIG_JSON
```

如果 Gateway 已经使用空配置启动，或者需要替换已有开发控制面，必须明确启用导入：

```bash
export GATEWAY_CONFIG_JSON="$(<config.example.json)"
GATEWAY_CONFIG_IMPORT=true docker compose --env-file .env.compose up -d
unset GATEWAY_CONFIG_JSON
```

导入会清理并重建 Source、Account、模型、Binding 和 Route。生产环境不要用它替代控制面管理 API 或恢复流程。

## 镜像构建与更新

Dockerfile 分为前端构建、Rust 构建和精简运行时三阶段。运行时使用非 root 用户，只包含网关二进制、管理端静态资源、CA 证书和健康检查所需的 curl。默认构建标签为 `my-ai-gateway:local`，可以覆盖镜像名或工具链版本：

```bash
GATEWAY_IMAGE=registry.example.com/my-ai-gateway:dev \
  docker compose --env-file .env.compose build gateway
```

更新代码后重新执行 `up -d --build`；数据库 migration 由新 Gateway 进程在启动时执行。升级前应先做 PostgreSQL 备份，并观察 `docker compose ... ps` 中的健康状态。

## 备份与恢复

备份和控制面脱敏导出见 [`operations.md`](operations.md)。Compose 下的物理备份示例：

```bash
umask 077
mkdir -p backups
docker compose --env-file .env.compose exec -T postgres \
  sh -c 'pg_dump --format=custom --no-owner --no-acl -U "$POSTGRES_USER" -d "$POSTGRES_DB"' \
  > "backups/gateway-$(date -u +%Y%m%dT%H%M%SZ).dump"
```

物理 dump 可能包含数据库中的加密凭据和全部历史，必须按高敏感备份保护。迁移到新环境时，优先使用脱敏控制面导出或恢复到新库，不要直接覆盖现有生产卷。
