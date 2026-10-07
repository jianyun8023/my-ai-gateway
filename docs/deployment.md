# 部署参考

本文包含当前仓库的 Docker Compose 单机部署和 Kubernetes/K3s 部署参考。Compose 负责运行 Gateway 和 PostgreSQL 16；Kubernetes 清单只部署 Gateway，连接已有 PostgreSQL。应用启动时会自动执行内置 migration，并从 PostgreSQL 控制面加载运行时 snapshot。

这套配置适合开发、测试和受控的内部部署。对公网提供服务前，还需要在网关前配置 TLS、访问边界、日志/指标采集、备份和 Secret 管理。

Kubernetes/K3s 的完整清单、外部 Secret、Ingress、升级和回滚说明见 [`kubernetes.md`](kubernetes.md)。

## 准备环境文件

```bash
cp .env.compose.example .env.compose
chmod 600 .env.compose
```

编辑 `.env.compose`，至少替换以下值：

- `POSTGRES_PASSWORD`：数据库密码；它会被 Compose 拼入容器内的 `DATABASE_URL`，应使用 URL-safe 字符（例如十六进制随机值）；
- `GATEWAY_ADMIN_KEY`：独立的 Admin API Key，不能与数据面 Key 或 Provider Key 复用；
- `GATEWAY_CREDENTIAL_MASTER_KEY`：创建、轮换和查看数据库 Virtual Key 所需的加密主密钥；
- `GATEWAY_API_KEY`：可选的过渡数据面静态入口 Key，新客户端不应依赖；
- 与 `config.example.json` 中 `credential_env` 对应的 Provider Key。

数据库 Virtual Key 的受控查看及 `credential_ciphertext` 都要求设置 `GATEWAY_CREDENTIAL_MASTER_KEY` 或 keyring 变量。不要把真实凭据写入 `GATEWAY_CONFIG_JSON`、镜像层或 Git。

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

## Kubernetes / K3s

仓库提供 [`deploy/kubernetes/`](../deploy/kubernetes/) 作为脱敏 Kustomize 基线。它默认使用 `apps` 命名空间、`8787` 端口、Traefik Ingress 和 GHCR `main` 镜像，并依赖外部创建的数据库 Secret、Provider Secret 与 GHCR 拉取 Secret。

不要直接把真实 Secret 写入该目录；请按 [`docs/kubernetes.md`](kubernetes.md) 准备外部资源后再应用清单。K3s 的实际集群入口、域名和 Argo CD 操作记录在本机私有的 `.private/k3s-deployment.md`，不会进入 Git。

## 图片历史与请求体限制

`/v1/chat/completions`、`/v1/responses`、`/v1/messages` 的请求体默认上限为 **32 MiB（33,554,432 bytes）**，可通过进程环境变量 `GATEWAY_MAX_REQUEST_BODY_BYTES` 调整，修改后需重启。只接受正整数字节数，`0`、空值或非法值会使启动失败；未设置时使用默认值。管理 API 保持原有独立限制。请求体包含 JSON、全部历史和 base64 图片，字节上限与模型 token/context 上限是不同约束。

旧版未显式配置 Axum `Bytes` 上限，使用框架默认 2 MiB；超限会在进入代理服务之前返回纯文本 `Failed to buffer the request body: length limit exceeded`。新版超限返回 HTTP 413、协议 JSON `error.code=request_too_large` 和 `x-request-id`，不会调用上游；读取请求体失败也返回协议 JSON。启动日志会记录实际字节上限，不记录图片/正文。提高上限会增加并发请求的内存占用，32 MiB 是默认单请求限制，并非内存预算。

入口反代的限制应不小于应用上限。Nginx 使用 `client_max_body_size 32m;`；ingress-nginx 使用 `nginx.ingress.kubernetes.io/proxy-body-size: "32m"`，并关闭 SSE 响应缓冲。Traefik 见 [`kubernetes.md`](kubernetes.md)；Nginx 注解不适用于 Traefik。

客户端仍应压缩截图，并将已识别的信息转为文字，减少图片历史累积。网关原样转发图片与 Provider 扩展字段，不自动缩图、裁剪历史或丢弃内容；上游自身的图片、请求体和上下文限制仍然生效。
