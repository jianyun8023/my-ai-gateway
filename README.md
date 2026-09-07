# my-ai-gateway

面向单用户自托管的 AI 网关，用统一 API 接入多个 Provider 和账号，通过 Web 控制台管理模型、路由与用量。后端使用 Rust，前端使用 React，配置与用量持久化到 PostgreSQL。

项目持续开发中，配置、API 和数据库 Schema 尚未承诺向后兼容。

## 核心能力

- **统一接入**：支持 OpenAI Chat Completions、OpenAI Responses 和 Anthropic Messages，原生转发 JSON 与 SSE。
- **可视化管理**：来源接入、模型发现与确认、协议能力检查、模型与路由配置。
- **故障切换**：首选账号优先，失败后自动 fallback，支持健康探测与冷却。
- **用量与运维**：Token 统计、请求归因、Virtual Key 管理、凭据加密、Prometheus 指标与可选 OpenTelemetry tracing。

| 协议 | 接口 |
| --- | --- |
| OpenAI Chat Completions | `POST /v1/chat/completions` |
| OpenAI Responses | `POST /v1/responses` |
| Anthropic Messages | `POST /v1/messages` |

内置 DeepSeek、MiniMax、Kimi Code 预设，三协议均使用原生路径。具体模型的可用功能以控制台能力矩阵为准。

## 快速开始

在仓库根目录使用 Docker Compose 启动网关和 PostgreSQL 16：

```bash
cp .env.compose.example .env.compose
# 编辑 .env.compose：替换数据库密码、GATEWAY_ADMIN_KEY 和 GATEWAY_CREDENTIAL_MASTER_KEY。
docker compose --env-file .env.compose up -d --build
curl http://127.0.0.1:8787/healthz
```

打开 [控制台](http://127.0.0.1:8787/admin/)，使用 `GATEWAY_ADMIN_KEY` 登录。默认仅监听宿主机回环地址；公网部署见[部署与安全说明](docs/deployment.md)。

### 首次接入

1. 添加来源和账号凭据，选择 Provider 预设并测试连接。
2. 发现并确认模型，配置逻辑模型、Binding 与路由，检查能力矩阵。
3. 在设置中创建 Virtual Key，供客户端调用网关。

运行期配置以 PostgreSQL 为准。可通过 [`config.example.json`](config.example.json) 初始化或显式导入；上游 Key 通过环境变量或加密凭据保存。

## 调用网关

将 Virtual Key 保存到 `GATEWAY_VIRTUAL_KEY`，先查询可用模型：

```bash
curl http://127.0.0.1:8787/v1/models \
  -H "Authorization: Bearer $GATEWAY_VIRTUAL_KEY"
```

将 `your-model-id` 替换为返回的模型 ID，即可发起流式请求：

```bash
curl -N http://127.0.0.1:8787/v1/responses \
  -H "Authorization: Bearer $GATEWAY_VIRTUAL_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"your-model-id","input":"hello","stream":true}'
```

## 超时配置

流式与非流式请求默认等待上游响应头 **120 秒**，由 `GATEWAY_SSE_CONNECTION_TIMEOUT_MS=120000` 控制。来源连接测试和健康探测也使用独立的 120 秒时限。

SSE 首事件、空闲和请求总时限分别为 30、60、300 秒，心跳间隔为 15 秒。完整参数见 [`.env.example`](.env.example)；反向代理需关闭流式缓冲并匹配超时设置。已有部署若设置了 `GATEWAY_SSE_CONNECTION_TIMEOUT_MS=10000`，需改为 `120000` 并重启。

## 本地开发

准备 PostgreSQL 16，使用 [Mise](https://mise.jdx.dev/) 安装仓库固定的 Rust 与 Node.js 工具链：

```bash
mise install
mise run install
cp .env.example .env
# 编辑 .env：配置 DATABASE_URL、GATEWAY_ADMIN_KEY 和 GATEWAY_CREDENTIAL_MASTER_KEY。
mise run dev
```

开发控制台位于 [localhost:5173](http://127.0.0.1:5173/)，网关位于 `127.0.0.1:8787`。

```bash
mise run build   # 构建前端与网关
mise run lint    # 静态检查
mise run test    # Rust、前端与脚本测试
mise run test-db # 使用独立的 .env.test 运行 PostgreSQL 回归
```

更多测试命令与验收范围见[测试说明](docs/testing.md)。

## 文档

| 需要了解 | 文档 |
| --- | --- |
| 部署与升级 | [Docker Compose](docs/deployment.md) · [Kubernetes / K3s](docs/kubernetes.md) |
| 配置与接口 | [环境变量](.env.example) · [Admin API](docs/admin-api.md) |
| 运维与安全 | [运维手册](docs/operations.md) · [安全说明](docs/security.md) |
| 设计与开发 | [架构设计](docs/ai-gateway-design.md) · [前端规范](design.md) · [CI](docs/ci.md) |
| 任务与验收 | [Issues](https://github.com/jianyun8023/my-ai-gateway/issues) · [PR](https://github.com/jianyun8023/my-ai-gateway/pulls) · [验收记录](docs/v0.1.0-acceptance.md) |
