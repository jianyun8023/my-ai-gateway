# my-ai-gateway

my-ai-gateway 是一个使用 Rust 编写的 AI 网关。它通过统一的下游入口代理多个上游 Provider、Source 和账号，并提供协议感知路由、故障切换、用量统计与控制面管理能力。

项目仍处于持续开发阶段，配置、HTTP API 和数据库 Schema 尚未承诺向后兼容，不建议在未经额外加固的情况下直接暴露到不可信网络。

## 支持的协议

| 协议 | 网关入口 | 上游处理方式 |
| --- | --- | --- |
| OpenAI Chat Completions | `POST /v1/chat/completions` | 优先原生透传 |
| OpenAI Responses | `POST /v1/responses` | 原生透传，或执行一次明确的 Adapter 转换 |
| Anthropic Messages | `POST /v1/messages` | 优先原生透传 |

MiniMax、DeepSeek 等原生支持三种协议的 Provider 不经过转换器。Kimi Code 自 2026-09 起官方原生支持 OpenAI Responses（`/v1/responses`），三协议全部原生透传，不再经过 Adapter。

## 核心能力

- PostgreSQL 是控制面和运行时路由的事实来源。
- `Source` 管理 Base URL、协议 endpoint、模型目录和能力矩阵；`Account` 独立管理凭据、权重、启用状态与健康状态。
- `LogicalModel`、`SourceModel`、`ModelBinding` 和 `Route` 职责分离；`/v1/models` 只公开已经确认且至少存在一个可用 Binding 的逻辑模型。
- 原生协议优先，Adapter 只允许一次直接转换；未知或不支持的能力会返回结构化错误，不会被猜测为可用。
- 固定首选账号优先；遇到 408、429、5xx、传输错误或账号不可用时，可以进入加权 fallback。
- 非流式 JSON 和流式 SSE 均支持 Usage 采集。逻辑请求与上游 attempt 分开记录，fallback 不会重复累计最终 Token。
- ProviderPreset、连接测试、模型发现、差异预览、模型编辑和批量确认流程已经集成。
- PostgreSQL-backed Virtual Key 支持模型白名单、轮换、撤销、scopes 和 key group。
- 账号健康状态支持被动失败冷却、主动探测、stale 过期放行、指数退避、重启恢复和人工启停。
- 凭据使用 AES-256-GCM 信封加密，支持多版本 keyring 和渐进式轮换；Admin 写操作具备脱敏审计日志。
- 管理端提供 Overview、Analysis 和 Request Events 页面。
- `/metrics` 提供 Prometheus 指标；配置 `OTEL_EXPORTER_OTLP_ENDPOINT` 后可通过 OTLP/gRPC 导出 OpenTelemetry trace。

完整设计、当前进度和管理 API 契约分别见：

- [`docs/ai-gateway-design.md`](docs/ai-gateway-design.md)
- [`docs/todo.md`](docs/todo.md)
- [`docs/admin-api.md`](docs/admin-api.md)

## 快速开始

### 准备工具链

仓库通过 [Mise](https://mise.jdx.dev/) 固定 Rust 1.97.1 和 Node.js 24：

```bash
mise install
mise run install
```

运行网关还需要 PostgreSQL 16。

### 本地运行

复制开发环境配置，并至少填写 `DATABASE_URL`：

```bash
cp .env.example .env
# 编辑 .env；至少配置 DATABASE_URL、Admin Key 和凭据主密钥。
mise run dev
```

服务启动后可以检查健康状态：

```bash
curl http://127.0.0.1:8787/healthz
```

网关与管理端静态页面默认监听 `127.0.0.1:8787`，Vite 开发服务器使用 `5173`。需要从局域网访问时，在 `.env` 中设置：

```dotenv
GATEWAY_LISTEN_ADDR=0.0.0.0:8787
VITE_DEV_HOST=0.0.0.0
```

对局域网开放前，必须配置独立的 `GATEWAY_ADMIN_KEY` 和 `GATEWAY_CREDENTIAL_MASTER_KEY`，并签发数据库 Virtual Key。真实上游凭据可以通过 `GATEWAY_ENV_FILE` 放在单独的、被 Git 忽略的环境文件中。

### 初始化控制面

空控制面首次启动时，可以通过 `GATEWAY_CONFIG_JSON` 导入 [`config.example.json`](config.example.json)：

```bash
export GATEWAY_CONFIG_JSON="$(<config.example.json)"
cargo run
```

该 JSON 只用于初始化、显式导入和测试，不是运行期配置源。数据库已有管理数据后，普通启动不会再次解析或覆盖它。只有显式设置 `GATEWAY_CONFIG_IMPORT=true` 才会事务化替换当前控制面，因此使用前必须确认数据影响。

示例配置只引用 `credential_env`。不要把真实 API Key 写入 JSON、仓库或日志。

## Docker 部署

### Docker Compose

仓库提供多阶段 [`Dockerfile`](Dockerfile) 和 [`docker-compose.yml`](docker-compose.yml)，可在单机上运行 Gateway 与 PostgreSQL 16：

```bash
cp .env.compose.example .env.compose
# 编辑 .env.compose，至少替换数据库密码、Admin Key 和凭据主密钥。
docker compose --env-file .env.compose config --quiet
docker compose --env-file .env.compose up -d --build
curl http://127.0.0.1:8787/healthz
```

镜像默认标记为 `my-ai-gateway:local`，可以通过 `GATEWAY_IMAGE` 覆盖。PostgreSQL 数据保存在 `gateway_pgdata` 命名卷中，端口 `8787` 默认只发布到宿主机回环地址。

完整的启动、升级、备份和恢复说明见 [`docs/deployment.md`](docs/deployment.md) 与 [`docs/operations.md`](docs/operations.md)。

### Kubernetes / K3s

仓库提供脱敏的 Kustomize 部署基线，默认部署 Gateway 并连接已有 PostgreSQL；清单、外部 Secret、Ingress、升级和回滚说明见 [`docs/kubernetes.md`](docs/kubernetes.md)，清单位于 [`deploy/kubernetes/`](deploy/kubernetes/)。

### 预构建镜像

GitHub Actions 会向 GitHub Container Registry 发布同时支持 `linux/amd64` 和 `linux/arm64` 的多架构镜像：

```text
ghcr.io/jianyun8023/my-ai-gateway:<tag>
```

标签规则如下：

- 推送到 `main`：只更新 `main` 标签，不更新 `latest`。
- 推送 `vMAJOR.MINOR.PATCH` Release tag：发布原始 tag，并生成去掉 `v` 的完整版本和 `MAJOR.MINOR` 标签。
- 稳定 Release tag：同时更新 `latest`；Prerelease tag 不更新 `latest`。

## 调用网关

正式客户端使用 PostgreSQL-backed Virtual Key；静态 `GATEWAY_API_KEY` 只保留为过渡兼容入口。三种协议入口都接受 `Authorization: Bearer ...`，兼容客户端也可以使用 `x-api-key`。

```bash
curl http://127.0.0.1:8787/v1/responses \
  -H "Authorization: Bearer $GATEWAY_VIRTUAL_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"kimi-for-coding-highspeed","input":"hello","stream":true}'
```

Kimi Responses 路由会在进程内完成 Responses 与 Anthropic Messages 的转换，并保留 thinking/signature、tool call、web search、Usage 和 SSE 事件顺序。

## 控制面与 Usage

核心管理资源包括：

```text
/admin/sources
/admin/accounts
/admin/logical-models
/admin/model-bindings
/admin/routes
```

集合路径支持 `GET`、`POST`，单资源路径支持 `GET`、`PUT`、`DELETE`，启停操作使用 `PUT .../{id}/enabled`。ProviderPreset、连接测试、模型发现、能力矩阵、凭据轮换、健康探测和 Usage 查询等完整契约见 [`docs/admin-api.md`](docs/admin-api.md)。

创建 Virtual Key 需要配置 `GATEWAY_CREDENTIAL_MASTER_KEY`。鉴权使用不可逆哈希；原始值以 AES-GCM 密文保存，并可由 Admin 显式查看和复制：

```bash
curl -X POST http://127.0.0.1:8787/admin/keys \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"service-a","allowed_models":["MiniMax-M2.7"]}'

curl http://127.0.0.1:8787/admin/keys/1/value \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"
```

Usage API 统一使用 UTC，并支持按时间、逻辑模型、上游模型、Provider、Source、账号、协议、Virtual Key、状态和 `usage_source` 组合筛选。逻辑事件只累计最终 Usage；每次上游尝试通过 `upstream_attempts` 单独统计。事件与导出默认不保存 prompt/response 正文。

## 流式请求

`GATEWAY_SSE_*_MS` 用于配置心跳、连接、首事件、空闲和总时限，具体参数见 [`.env.example`](.env.example)。网关心跳使用 `: gateway-heartbeat` SSE comment，不会改变 Provider 事件顺序、Usage、序列号或 TTFT。

反向代理部署时应关闭响应缓冲、保留 `text/event-stream`，并将代理读取超时设置为大于网关总时限。代理的 stream idle timeout 应长于心跳间隔，且不能合并、删除或改写以 `:` 开头的 SSE comment。

## 安全边界

- `GATEWAY_ADMIN_KEY` 与数据面的 Virtual Key 完全分离；未配置 Admin Key 时，所有 Admin API 都会 fail closed 并返回 `401`。
- Key 列表、日志和脱敏控制面导出均不返回哈希或密文；只有 `/admin/keys/:id/value` 执行显式解密。0016 之前创建的 hash-only Key 必须先轮换才能查看。
- Provider Base URL 默认拒绝私网、loopback、link-local、云元数据地址和不安全重定向；私网自托管来源必须通过服务端 allowlist 显式放行。
- 日志禁止输出 Authorization、API Key 和完整请求正文。
- 生产凭据应通过 `credential_env` 或受保护的 Secret 注入，不能写入镜像或提交到仓库。

更多说明见 [`docs/security.md`](docs/security.md)。

## 开发与验证

常用任务：

```bash
mise run dev          # 启动网关与前端开发服务器
mise run build        # 构建前端静态资源与 Rust 网关
mise run lint         # Rust 与前端静态检查
mise run test         # Rust、前端和本地契约测试
mise run test-db      # 使用 .env.test 运行完整 PostgreSQL 回归
mise run verify       # 完整本地门禁
```

Pull Request 会分别执行以下两个检查：

- `Validate and build`：配置解析、Rust/前端静态检查和完整构建。
- `Unit and PostgreSQL tests`：Rust、前端、契约测试和 PostgreSQL 集成测试。

真实 Provider 与 Codex CLI E2E 测试均为显式 opt-in，不会进入默认 CI，也不会自动消耗上游 Token：

- [`docs/live-provider-smoke.md`](docs/live-provider-smoke.md)
- [`docs/codex-e2e.md`](docs/codex-e2e.md)
- [`docs/ci.md`](docs/ci.md)

## 项目结构

```text
src/                           Rust 网关主程序
migrations/                    PostgreSQL migrations
docs/                          设计、接口、部署和运维文档
web/                           React + TypeScript 管理端
config.example.json            初始化与导入示例
```

## 第三方代码

my-ai-gateway 不是 CPA Usage Keeper，也不使用其 Go 后端、SQLite、Redis queue、CPA Management API、Auth Files、Ranking、配额或充值逻辑。

管理端的 Overview、Analysis 和 Request Events 页面结构及部分 React 交互基于 CPA Usage Keeper 的 MIT 代码适配。来源、复用边界和许可证见 [`web/THIRD_PARTY_NOTICES.md`](web/THIRD_PARTY_NOTICES.md) 与 [`web/licenses/CPA_USAGE_KEEPER_LICENSE`](web/licenses/CPA_USAGE_KEEPER_LICENSE)。
