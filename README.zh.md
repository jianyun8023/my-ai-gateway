# my-ai-gateway

Rust AI 网关，用一个下游入口统一代理多个上游 Provider、Source 和账号。项目当前处于持续开发阶段，配置、HTTP API 和数据库 Schema 尚未承诺后向兼容，不建议未经额外加固直接暴露到不可信网络。

项目正式支持三类北向协议：

| 协议 | 网关入口 | 上游处理 |
| --- | --- | --- |
| OpenAI Chat Completions | `POST /v1/chat/completions` | 原生透传优先 |
| OpenAI Responses | `POST /v1/responses` | 原生透传或一次明确的 Adapter 转换 |
| Anthropic Messages | `POST /v1/messages` | 原生透传优先 |

MiniMax、DeepSeek 等原生支持三协议的 Provider 不进入转换器。Kimi Code 的 Responses 路径使用仓库内置的 `kimi-responses-adapter`，不需要额外部署 Adapter 服务。

## 当前能力

- PostgreSQL 是控制面和运行时路由的事实来源。
- `Source` 管理 Base URL、协议 endpoint、模型目录和能力；`Account` 独立管理凭据、权重、启用状态和健康状态。
- `LogicalModel`、`SourceModel`、`ModelBinding`、`Route` 分离；`/v1/models` 只公开已确认且至少有可用 Binding 的逻辑模型。
- 原生协议优先，Adapter 只允许一次直接转换；不支持或未知能力返回结构化错误，不会猜测为支持。
- 首选 Binding 固定优先；408、429、5xx、传输错误或首选账号不可用时，可进入加权 fallback。
- 非流式 JSON 与流式 SSE 均支持 usage 采集；逻辑请求和每次上游 attempt 分开记录，fallback 不会重复累计最终 Token。
- Usage 记录实际 `upstream_model_id`、`source_id`、独立的 `client_source`、`usage_source` 和流式 TTFT。
- PostgreSQL-backed Virtual Key 支持创建、列表、撤销和模型白名单。
- 管理端已有 Overview、Analysis、Request Events 三个网关原生用量页面。
- ProviderPreset、连接测试、模型发现、差异预览、编辑和批量确认 API 已实现。
- `/admin/capabilities` 从当前 DB runtime snapshot 输出三协议有效能力矩阵和完整转换链。

完整设计和当前进度见 [`docs/ai-gateway-design.md`](docs/ai-gateway-design.md) 与 [`docs/todo.md`](docs/todo.md)。管理 API 契约见 [`docs/admin-api.md`](docs/admin-api.md)。

## 快速开始

### 前置条件

- PostgreSQL 16；
- Rust 1.97.1；
- Node.js 24（需要构建管理端时）；
- 推荐使用 [Mise](https://mise.jdx.dev/) 安装仓库锁定的工具版本。

```bash
mise install
mise run install
```

### 本地运行

`DATABASE_URL` 是 DB-first 运行时的必填项。下面的值仅用于本地开发，请替换示例 Key：

```bash
export DATABASE_URL='postgres://gateway:gateway@127.0.0.1:5432/gateway'
export GATEWAY_API_KEY='replace-with-a-random-data-plane-secret'
export GATEWAY_ADMIN_KEY='replace-with-a-different-admin-secret'
cargo run
```

另一个终端中检查服务：

```bash
curl http://127.0.0.1:8787/healthz
```

监听地址通过 `GATEWAY_LISTEN_ADDR` 独立设置，默认是 `127.0.0.1:8787`。

### 初始化控制面

空控制面首次启动时，可以把 [`config.example.json`](config.example.json) 作为 `GATEWAY_CONFIG_JSON` 提供。该 JSON 只用于初始化、显式导入和测试，不是运行期配置源：

```bash
export GATEWAY_CONFIG_JSON="$(<config.example.json)"
cargo run
```

数据库中已有任意管理数据后，普通启动不会再次解析或覆盖该 JSON。只有显式设置 `GATEWAY_CONFIG_IMPORT=true` 才会事务化替换当前开发控制面；该操作会清理并重新导入 Source、Account、模型、Binding 和 Route，不应在未确认数据影响时使用。

示例只引用 `credential_env`，不要把真实 API Key 写入 JSON、仓库或日志。

### Docker Compose

仓库中的 [`docker-compose.yml`](docker-compose.yml) 提供 Gateway + PostgreSQL 16 的开发部署结构：

```bash
export GATEWAY_API_KEY='replace-with-a-random-secret'
export GATEWAY_ADMIN_KEY='replace-with-a-different-admin-secret'
docker compose up --build
```

Compose 示例使用开发数据库账号，不是生产安全模板。生产部署前需要单独配置强凭据、Admin Key、TLS、网络边界、备份和 Secret 注入。

## 调用网关

静态 `GATEWAY_API_KEY` 是过渡入口保护；正式客户端优先使用 PostgreSQL-backed Virtual Key。三类协议入口接受 `Authorization: Bearer ...`；兼容客户端也可以使用 `x-api-key`。

```bash
curl http://127.0.0.1:8787/v1/responses \
  -H "Authorization: Bearer $GATEWAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"kimi-for-coding-highspeed","input":"hello","stream":true}'
```

Kimi Responses 路由在进程内完成 Responses 与 Anthropic Messages 的转换，并保留 thinking/signature、tool call、web search、usage 和 SSE 事件顺序。

Codex CLI 到 Gateway 的工具调用与网络搜索 E2E 使用显式 opt-in 的
`mise run test-codex-e2e`。默认回归只执行本地契约单测；真实运行需要隔离的
`CODEX_HOME`、Gateway Key 和 Admin Key。详细配置、case 维护、Usage 校验和安全边界见
[`docs/codex-e2e.md`](docs/codex-e2e.md)。

## 控制面

主要资源路径如下。集合路径支持 `GET`、`POST`，单资源路径支持 `GET`、`PUT`、`DELETE`，启停使用 `PUT .../{id}/enabled`：

```text
/admin/sources
/admin/accounts
/admin/logical-models
/admin/model-bindings
/admin/routes
```

其他管理入口包括：

- `/admin/keys` 与 `/admin/keys/:id/revoke`；
- `/admin/provider-presets`；
- `/admin/sources/:source_id/connection-tests`；
- `/admin/sources/:source_id/discoveries`；
- `/admin/sources/:source_id/models` 与确认接口；
- `/admin/capabilities`；
- `/admin/usage/summary|timeseries|breakdown|events|export`。

创建 Virtual Key 时，原始 Key 只在创建响应中返回：

```bash
curl -X POST http://127.0.0.1:8787/admin/keys \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name":"service-a","allowed_models":["MiniMax-M2.7"]}'
```

Account 响应不会返回 `credential_ciphertext` 或明文凭据。控制面错误统一使用 `{"error":{"code":"...","message":"..."}}`。

## Usage 语义

Usage API 使用 `version: "v1"` 和 `timezone: "UTC"`。所有查询共享以下组合筛选：

```text
from, to, logical_model, upstream_model, provider, source_id,
client_source, account, protocol_in, protocol_upstream,
virtual_key, status, status_code, usage_source
```

`from` 和 `to` 接受带 offset 的 RFC3339，并按半开区间 `[from,to)` 解释。`events` 固定按 `(created_at DESC, request_id DESC)` 排序，后续页应原样传回不透明的 `page.next_cursor`。

`provider_id` 来自 Source 创建时固化的 `provider_preset_id`，因此同一 ProviderPreset 下的多个 Source 会归入同一 Provider；`source_id` 表示 DB-first Runtime Binding 最终实际选中的 Source。可选请求头 `X-Client-Source` 只记录为独立 `client_source`，不参与路由或鉴权。逻辑事件成功时归因最终成功 attempt，全部失败时归因最终实际 attempt。Token 聚合只累计每个逻辑请求的最终 Usage，`upstream_attempts` 单独统计上游尝试。

```bash
curl 'http://127.0.0.1:8787/admin/usage/timeseries?granularity=day&logical_model=MiniMax-M2.7' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"

curl 'http://127.0.0.1:8787/admin/usage/breakdown?breakdown=source_id&usage_source=upstream' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY"

curl 'http://127.0.0.1:8787/admin/usage/export?format=csv&status=failure' \
  -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" -o usage-events.csv
```

CSV/JSON 导出复用 events 的筛选和排序，并有 10,000 行保护上限。Usage 事件契约默认不保存 prompt/response 正文；失败只保留状态码和脱敏的 `error_summary`。

## 安全边界

当前版本已经具备凭据响应脱敏、正文默认不落库、Virtual Key 哈希存储、Admin API fail-closed，以及 Provider URL allowlist、解析后 IP 校验和重定向限制。仍待完成的生产安全工作包括：

- 统一 Secret Resolver 与凭据信封加密（#47）；
- Admin 写操作审计日志（#48）。

`GATEWAY_ADMIN_KEY` 与数据面 `GATEWAY_API_KEY` 完全分离。未设置 Admin Key 时网关仍可启动，但所有 Admin API 请求固定返回 `401`；数据面 Key 和 PostgreSQL Virtual Key 都不能调用管理接口。Provider Base URL 默认拒绝私网、loopback、link-local、云元数据地址和不安全重定向，私网自托管来源必须通过服务端 allowlist 显式放行。

日志中禁止输出 Authorization、API Key 和完整请求正文。生产凭据应通过 `credential_env` 或受保护的 Secret 注入，不要把真实 Key 提交到仓库。

## 开发与验证

常用组合任务：

```bash
mise run dev
mise run build
mise run test
mise run lint
mise run verify
```

Rust 基础门禁：

```bash
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo fmt --all -- --check
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo check
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo clippy --all-targets -- -D warnings
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo test
python3 -m json.tool config.example.json >/dev/null
```

PostgreSQL 集成测试只连接显式的 `TEST_DATABASE_URL`，每次运行创建并清理独立 schema。完整控制面回归是 ignored test，验收时必须显式运行，不能把缺少数据库导致的跳过当作通过：

```bash
TEST_DATABASE_URL='postgres://gateway:gateway@127.0.0.1:5432/gateway_test' \
  cargo test postgres_db_first_crud_rollback_snapshot_and_models_contract -- --ignored
```

## 项目结构

```text
src/                           Rust 网关主程序
crates/kimi-responses-adapter/ 内置 Kimi Responses Adapter
migrations/                    PostgreSQL migrations
docs/                          设计、管理 API 和运行文档
web/                           React + TypeScript 管理端
config.example.json            初始化/导入示例
```

## 与 CPA Usage Keeper 的关系

my-ai-gateway 不是 CPA Usage Keeper，也不使用其 Go 后端、SQLite、Redis queue、CPA Management API、Auth Files、Ranking、配额或充值逻辑。

管理端的 Overview、Analysis、Request Events 页面结构与部分 React 交互基于 CPA Usage Keeper 的 MIT 代码适配。来源、复用边界和完整许可证保存在 [`web/THIRD_PARTY_NOTICES.md`](web/THIRD_PARTY_NOTICES.md) 与 [`web/licenses/CPA_USAGE_KEEPER_LICENSE`](web/licenses/CPA_USAGE_KEEPER_LICENSE)。
