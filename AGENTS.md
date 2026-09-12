# my-ai-gateway 项目协作规范

本文件适用于仓库根目录及其所有子目录；更深层的 `AGENTS.md` / `AGENTS.override.md` 优先，系统、开发者和直接用户指令优先于本文件。优先使用中文沟通。

## 1. 项目定位与事实来源

这是一个单用户、自托管、界面优先的 AI Provider 聚合端：Rust 网关统一代理多个 Provider / Source 和上游账号，React 控制台负责接入、路由、用量与运维。正式支持 OpenAI Chat Completions、OpenAI Responses、Anthropic Messages 三类北向协议。

项目持续开发中，尚未承诺稳定版本或内部后向兼容。不要为历史配置、HTTP API、Schema 或内部类型主动增加兼容别名、双读/双写、迁移垫片或废弃字段；外部协议兼容目标和明确的用户要求除外。

| 要确认的信息 | 主要依据 |
| --- | --- |
| 实际行为、已接入端点与测试 | 当前任务分支的源码、路由注册、migration、测试；已合并 PR 用于追溯 |
| 架构与领域设计约束 | [设计文档](docs/ai-gateway-design.md)与本文件 |
| 需求、任务状态、评审及验证记录 | [GitHub Issues](https://github.com/jianyun8023/my-ai-gateway/issues) / [Pull Requests](https://github.com/jianyun8023/my-ai-gateway/pulls) |
| HTTP 接口与配置 | [Admin API](docs/admin-api.md)、[README.zh.md](README.zh.md)、[配置示例](config.example.json)、[环境变量示例](.env.example) |
| 测试与发布验收 | [测试说明](docs/testing.md)、[CI 说明](docs/ci.md)、[v0.1.0 验收清单](docs/v0.1.0-acceptance.md) |
| 部署与运维 | [部署](docs/deployment.md)、[Kubernetes](docs/kubernetes.md)、[运维](docs/operations.md)、[安全](docs/security.md) |

发现设计、TODO、Issue 状态与代码不一致时，明确指出差异；不要通过改写约束掩盖实现缺口。Issue 关闭、PR 合并、CI 通过和生产验收完成是不同事实。[docs/todo.md](docs/todo.md)仅作索引，不能替代实时 Issue 状态和验收证据。

## 2. 当前实现快照

以下在 **2026-09-09、main `e3c9e24`（PR #183 合并提交）** 核对；用于避免重复实现，后续任务仍需检查自己的分支。

- **数据面**：三协议 JSON/SSE 原生转发、首选账号与跨 Source/Provider fallback、健康冷却/主动探测、SSE 心跳/取消/超时、请求及 attempt 用量记录均已接入。
- **控制面**：启动必须配置 PostgreSQL；Source / Account / LogicalModel / ModelBinding / Route CRUD、模型发现与确认、模型级协议能力声明、有效能力矩阵、runtime snapshot 更新已实现。
- **鉴权与运维**：PostgreSQL-backed Virtual Key 已实现创建、查询、轮换、撤销、模型白名单及 Admin 显式读取加密保存的 Key；已有 Secret Resolver、凭据信封加密、Admin 写审计、保留清理、控制面导入/导出。`GATEWAY_API_KEY` 仍有过渡静态入口实现，不能再把 Virtual Key 写成未来功能。
- **可观测性**：Prometheus `/metrics` 已接入；OpenTelemetry OTLP/gRPC tracing 已由 [PR #80](https://github.com/jianyun8023/my-ai-gateway/pull/80) 实现，通过 `OTEL_EXPORTER_OTLP_ENDPOINT` 启用，未设置时仅本地 tracing 日志。是否在生产配置、采集成功需另外验证。
- **事件与控制台**：#110 按方案 C 新增窄 `system_events`、统一 `/admin/events` 读模型与“运行事件”页；既有事实表不双写，普通成功请求不进入该时间线。`/admin/` 下共有总览、用量分析、请求事件、运行事件、来源管理、模型与路由、能力矩阵、设置八个导航入口；前三项仍是用量主导航。#199 已将原“模型发现”从一级导航下沉为来源生命周期内的“模型更新审核”（`#sources/<id>` 详情、`#sources/<id>/edit` 编辑、`#sources/<id>/review` 审核），来源列表/详情/编辑按 `docs/design/source-management-ui/` 原型重组。
- **前端治理**：#166 的 54 项原始范围与验收项已完成并关闭，PR #169–#182 已合并；逐批证据与边界保留在 `docs/mantine-migration.md`。这不替代 #110 第九页、真实 Provider、生产配置或性能验收。
- **Provider / Adapter**：DeepSeek、MiniMax 最新内置 preset 为 `@3`，Kimi Code CN 为 `@5`（2026-09-12 补充：启用鉴权 `GET /v1/models`，迁移 `0026_kimi_code_cn_discovery.sql` 更新存量 Source 的 discovery 与旧默认名称）；通用 `BUILTIN_PROVIDER_PRESET_VERSION` 仍为 `3`，Kimi 单独版本常量为 `5`，内部 ID 保持 `kimi_code`。Kimi Responses 已改为原生 `/v1/responses`，迁移见 `0023_kimi_native_responses.sql`（[PR #158](https://github.com/jianyun8023/my-ai-gateway/pull/158)）。生产 Adapter 注册表为空，原 `crates/kimi-responses-adapter` 已删除；`cfg(test)` 中的旧名称用于框架测试，不代表生产支持。

当前跟踪与验收边界：

- 当前开放任务是 [#113 测试总计划](https://github.com/jianyun8023/my-ai-gateway/issues/113)与 [#120 性能基线与故障注入](https://github.com/jianyun8023/my-ai-gateway/issues/120)。[#110 事件中心](https://github.com/jianyun8023/my-ai-gateway/issues/110)的方案 C 已由 [PR #183](https://github.com/jianyun8023/my-ai-gateway/pull/183) 合并实现，7/7 项代码验收完成并关闭 Issue；这不代表部署后的 migration / 保留清理、生产历史规模查询或真实 Provider / live 验收已完成。[PR #159](https://github.com/jianyun8023/my-ai-gateway/pull/159) 已实现 `test-faults` / `test-load`，运行证据与覆盖边界见验收清单；不能将本地 Mock 性能视为生产性能或完整 live 验收。
- #1、#50 已关闭；本轮已修正 TODO / 设计文档中的 OTel 待办等过时描述。#96 / #97 / #98 已关闭，但验收清单的生产复验未勾选，#98 评论仍明确缺根因结论；不得据关闭状态宣称生产问题已经验证解决。
- PR #159 已合并修复：SSE usage 提取失败日志仅记录元数据，移除 Base64 正文预览与正文指纹；Chat 缺少 `[DONE]` 时，只有所有已出现 choice 均提供 `finish_reason` 才补结束标记，否则报告流截断错误。
- 尚无 GitHub Release 或 tag。已有验收记录早于 Kimi 原生切换；完整 live 覆盖、部署后的 migration / Provider 行为与生产数据复验应按实际证据报告，不由历史勾选推断。

本节不维护完整已完成任务清单；新进展记录到关联 Issue/PR，更新快照时保留日期与代码依据。

## 3. 必须保持的领域与协议约束

改变以下设计基线前，同步更新设计文档与关联任务说明。

### 来源、模型与路由

- `Source` 负责 Base URL、协议 endpoint、模型目录和能力矩阵；`Account` 负责凭据、启用状态、权重及健康状态。允许一对一部署，但职责与类型不能合并。
- 模型分为 `ProviderPreset`、`SourceModel`、`LogicalModel`、`ModelPreset`、`PricingProfile`；模型发现经过预设补齐与用户确认，刷新不得静默覆盖已确认字段。版本化 preset 是不可变快照；更新存量 Source 必须有明确变更路径。
- 模型展示目录与可路由 Binding 分离。`SourceModelCapability` 按 `(source_id, upstream_model_id, protocol)` 记录；`/v1/models` 只公开已确认且至少有可用 Binding 的逻辑模型，并遵守账号启用/健康过滤。待确认、不可用、未知能力不得隐式公开或路由。
- Provider 支持原生协议时优先透传，不能仅因 Provider 名称就猜测所有模型/功能均支持。非原生路径必须有明确注册的 Adapter，只允许一次直接转换；当前没有生产 Adapter，不能配置不存在的转换器。
- 路由保留 `protocol_in → protocol_upstream → endpoint/Adapter` 完整链和能力状态。`unknown` / `unsupported` 不得当作支持，能力不支持时默认返回结构化错误；允许的能力损失必须显式标为 `degraded` 并记录 warning。
- 首选账号固定优先，失败后才进入 fallback 池；保留账号级模型重写和逐 attempt 的实际 Source / Provider / upstream model 归因。
- 不得静默丢失 Tools、Web Search、Thinking、signature、Usage 或 Provider 扩展字段。每个 Adapter 转换路径单独命名，分别覆盖请求、非流式响应与 SSE；SSE 必须保持事件顺序、状态和异常终止语义。

### 持久化与用量

- PostgreSQL 是运行期控制面的事实来源。`GATEWAY_CONFIG_JSON` 仅用于空库初始化、显式导入和测试；强制导入由 `GATEWAY_CONFIG_IMPORT` 控制，不能回退成长期配置真源。
- 控制面变更遵循事务写入、验证并构建 snapshot、原子替换运行时快照的既有流程；不要绕开 repository/service 直接拼接运行配置。
- 所有 Schema 变化新增 migration，面向当前开发基线前进演进；破坏性调整同步调用方、示例、文档和回归测试，不以静默降级掩盖不匹配。
- `request_id` 标识逻辑请求并用于 Usage 幂等；上游尝试按 `(request_id, attempt_no)` 记录。fallback/retry 不得重复统计最终请求或 Token。
- 同时保留 requested/logical model 与实际 upstream model，区分 Provider、实际 `source_id`、客户端 `client_source` 与 Virtual Key。显式 `X-Client-Source` 优先，缺失时已有鉴权身份回填逻辑。
- Token 必须记录 `usage_source`，区分上游报告、流式解析、估算与缺失；失败且无上游 usage 时不得估算出 Token。缓存、推理 Token 语义以现有契约为准。
- 时间统一 UTC 持久化；统计支持按时间、模型、Provider、Source、账号、协议及 Virtual Key 筛选。默认不保存 prompt/response 正文，不把正文日志作为统计来源。

### 安全与产品范围

- 生产上游凭据通过 `credential_env` / Secret 注入或数据库密文解析；真实 Key 不得进入仓库、日志、审计 diff 或普通管理响应。Virtual Key 的显式读取端点属于已有受 Admin 鉴权保护的例外，不能扩展到普通列表/导出。
- Admin API 使用 `GATEWAY_ADMIN_KEY`，与下游 Virtual Key / `GATEWAY_API_KEY` 分离；未配置 Admin Key 时管理 API 必须拒绝请求。
- Provider URL 必须经过 `SourceUrlPolicy` / 共享 HTTP client 的 allowlist、DNS 与重定向校验，禁止客户端任意指定上游 URL；私网自托管来源必须显式放行。
- 日志禁止输出 Authorization、API Key 和完整请求正文；默认不记录响应正文片段。Base64/hex 是可逆编码，不能当作正文脱敏。
- 账号代理必须遵守上游服务条款。不实现余额、充值、账单、额度扣减、规避 Provider 风控、TLS 指纹/请求伪装或反封禁；不扩展为多租户 SaaS、复杂 RBAC 或大规模账号运营产品。
- 用量界面优先 Token 时序、模型/Provider/Source 分布和请求归因，价格/成本仅为可选次级视图；协议感知路由与本项目能力矩阵优先于外部产品范例。
- 可复用 CPA Usage Keeper 的 MIT 页面组件、结构与交互，保留 License 和来源说明；不复用其 Go 后端、SQLite、Redis queue、CPA Management API、Auth Files、Ranking 或凭据/配额逻辑。New API 只借鉴接入、发现、模型与用量的产品思路/字段语义，不复制 AGPL 代码或 UI。

## 4. 代码导航与修改入口

| 路径 | 职责 |
| --- | --- |
| `src/main.rs` → `src/lib.rs` → `src/runtime.rs` | 启动、DB-first 初始化、探测循环、ops CLI |
| `src/app.rs`、`src/api/` | HTTP 路由、Admin 鉴权/审计中间件、协议与管理处理器 |
| `src/state.rs`、`src/auth.rs` | 共享运行状态与快照发布；独立鉴权身份解析 |
| `src/domain/` | 配置、协议、preset、模型目录、路由及能力矩阵 |
| `src/control_plane/` | 资源用例、事务生命周期、校验、导入、snapshot 构建、模型发现/确认 |
| `src/proxy/` | 请求编排、fallback、转发、客户端归因、重试策略、流式结算及 transport/SSE/usage |
| `src/http.rs`、`src/http/response.rs`、`src/source_url.rs` | 共享上游 HTTP client、HTTP 错误封装与 URL/SSRF 边界 |
| `src/infra/`、`src/infra/db/` | PostgreSQL、健康、Secret、审计、运维、指标/trace |
| `migrations/` | SQLx 前进式数据库迁移；按现有最大编号新增 |
| `web/src/App.tsx`、`web/src/lib/consoleNavigation.ts` | 控制台导航与页面接入 |
| `web/src/pages/`、`web/src/features/` | 页面组合、用量与控制面功能；表单和详情放在所属功能目录 |
| `web/src/admin-api/`、`web/src/gateway-usage/`、`web/src/hooks/` | 共享 Admin 传输、用量适配与查询生命周期；分层和门禁见 [前端架构](docs/frontend-architecture.md) |
| `tests/`、`scripts/`、`examples/conformance_target.rs` | Mock、Contract、覆盖矩阵、外部扫描、SDK、差分和 live smoke |
| `deploy/`、`Dockerfile`、`.github/workflows/` | 部署配置、镜像构建、PR CI |

技术栈：Tokio + Axum、Reqwest、Serde、SQLx/PostgreSQL、Tracing + Prometheus/OpenTelemetry；前端 React + TypeScript + Vite。Rust / Node 版本与命令以 `mise.toml` 为准，不另建平行工具链。

调查时追踪本次修改的实际调用链。例如数据面是 `app → api/proxy → proxy/service → routing/transport/stream → usage/db`，控制面是 `app → api/admin → control_plane → transaction/snapshot → state.reload_snapshot`。不要仅凭文件名或旧文档判断功能是否已接入。

## 5. Issue、worktree 与 PR 工作流

1. 开始前检查本地状态、当前分支、相关 Issue 和已有 PR，复用同一任务，避免重复创建。以当前任务分支与设计基线为准；明确从 `main` 开始的任务使用最新 `main`。
2. 问题修复/新特性默认使用独立 worktree 和关联分支（如 `codex/123-short-description`）；从目标分支最新提交创建，不混入脏工作区或其他任务改动。
3. 具备访问权限且用户已授权外部写入时，没有对应 Issue 则创建，写清背景、范围、验收与验证；实现后提交、推送并创建 PR，记录最终范围、取舍、验证结果和未覆盖项。可随合并关闭的任务使用 `Closes #123`；仅诊断或部分修复不要据此关闭未完成的验收。
4. CI、必要评审和任务要求验证通过后，按已有合并授权处理；未获合并授权时停在可评审状态。确认目标分支包含提交，且 worktree 无未提交/未推送工作后，再清理 worktree 与本地分支；远端分支按仓库策略清理，不丢弃未保存工作。
5. 只有在任务关联 Issue/PR、具备权限且已有外部写入授权时回写进展/结论。没有授权或无法访问时，完成可执行的本地工作，并在最终报告给出可直接同步的标题、范围、验证摘要及未覆盖风险；临时文件和会话不能成为唯一任务记录。

纯调查、问答和轻量文档维护可以不单独创建 Issue/worktree/PR；跳过时说明原因。不要为了遵循工作流擅自向 GitHub 写入或请求用户重复授权。

## 6. 验证与外部资料

按修改范围选择必要验证；只改文档时检查 diff、路径/链接和事实即可，不为其重跑完整 Provider/数据库验收。不要将未执行、跳过或占位命令记为通过。

| 场景 | 命令 / 要求 |
| --- | --- |
| 配置和测试元数据 JSON | `mise run config-check` |
| Rust + Web 静态检查 | `mise run lint`（fmt check、cargo check/clippy、ESLint、TypeScript） |
| 构建 | `mise run build` |
| 默认回归 | `mise run test`（Rust 含 `test-support`、串行执行，Web 和脚本单测） |
| 故障 / 本地性能 | `test-faults`；先 `build-load-tools` 再 `test-load`，详见 `tests/load/README.md`；`test-all-offline` 包含本地负载 |
| 三协议 Contract | `mise run test-contract` |
| PostgreSQL 集成 | 独立测试库设置 `TEST_DATABASE_URL` 后运行 `mise run test-postgres`；`test-db` 从 `.env.test` 读取配置 |
| 组合门禁 | `mise run verify`；未设置 `TEST_DATABASE_URL` 会跳过 ignored PostgreSQL 测试，必须如实报告 |
| 外部协议 / SDK / 差分 | `test-conformance`、`test-sdk-smoke`、`test-differential`；先读 `docs/testing.md` 中目标服务与 opt-in 要求 |
| 真实 Provider / Codex E2E | `test-live` / `test-codex-e2e`；按 `docs/live-provider-smoke.md` / `docs/codex-e2e.md` 使用独立测试库与显式 opt-in，普通 CI 不发送真实模型请求 |

默认 Cargo 缓存不可写时，对上述命令设置 `CARGO_HOME=/tmp/my-ai-gateway-cargo`。单独验证 Rust 可用 `cargo fmt --all -- --check`、`cargo check --workspace`、`cargo clippy --all-targets --features test-support -- -D warnings`、`cargo test --workspace --features test-support -- --test-threads=1`；需要修复本次格式问题才运行 `cargo fmt --all`，不格式化无关文件。

PR CI 定义在 `.github/workflows/pull-request.yml`，包含静态检查/构建和单元/Contract/PostgreSQL 测试。镜像发布工作流为 `main` 与版本 tag 构建 Linux amd64/arm64 镜像；镜像发布成功不等于已部署或生产复验通过。

- API、SDK、框架文档查询优先使用 Context7；不可用时用官方文档或仓库资料，并说明替代来源。仓库状态查询直接使用 GitHub 与代码依据。
- Python 辅助项目优先 `uv run` / `uv sync` / `uv add`，除非明确要求，不使用 pip、conda 或系统 Python；现有 JSON 校验优先复用 `mise run config-check`。
- 新增环境变量、接口或数据库字段必须同步相关文档；改协议、Adapter、thinking、signature、tool call、web search、usage 或 SSE 时补充对应行为回归。
