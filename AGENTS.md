# my-ai-gateway 项目协作规范

本文件适用于仓库根目录及其所有子目录。若更深层目录存在 `AGENTS.md` 或 `AGENTS.override.md`，以更深层文件为准；直接的用户、开发者和系统指令优先于本文件。

## 项目定位

这是一个 Rust AI 网关项目，用于统一代理多个上游 Provider 和多个上游账号。

项目当前处于持续开发阶段，尚未承诺稳定版本或后向兼容。配置、HTTP API、数据库 Schema、内部类型和行为都可以按最新设计直接调整；除非用户明确要求，不要为了旧版本增加兼容别名、双写/双读、迁移垫片或废弃字段保留。发生破坏性调整时，应同步更新文档、示例、迁移和测试，并以当前代码与设计基线为准。

项目托管在 [GitHub](https://github.com/jianyun8023/my-ai-gateway)。GitHub Issues 用于维护问题、需求和开发任务，Pull Request 用于修复、评审和合并。任务进展、方案变更和验证结果应记录到对应 Issue/PR；只有在任务已关联 Issue/PR、具备访问权限且用户授权外部写入时才直接回写，否则在最终报告中提供待同步内容，不能让本地会话或临时文件成为唯一记录。

当前正式支持三类北向协议：

- OpenAI Chat Completions；
- OpenAI Responses；
- Anthropic Messages。

## 核心架构约束

1. Provider 原生支持某协议时，必须优先原生透传。
2. Provider 不支持某协议时，使用明确的 Adapter。
3. MiniMax、DeepSeek 等三协议 Provider 不应进入转换器。
4. Kimi Responses 使用内置 `kimi-responses-adapter` workspace crate。
5. 首选账号固定优先，失败后才进入 fallback 账号池。
6. 不允许在协议转换中静默丢失 Tools、Web Search、Thinking、Usage 或 Provider 扩展字段。
7. 默认不保存 prompt/response 正文。
8. 不实现余额、充值、额度扣减或规避 Provider 风控的逻辑。

## 已确认的设计基线

以下决策来自当前需求讨论，后续实现默认遵循；如需改变，先更新设计文档和相关任务说明：

- `Source` 负责 Base URL、协议 endpoint、模型目录和能力矩阵；`Account` 负责凭据、启用状态、权重和健康状态。第一版允许一对一，但类型和职责不能合并。
- PostgreSQL 是控制面事实来源；开发阶段的 `GATEWAY_CONFIG_JSON` 仅用于初始化、导入和测试。`UsageEvent` 区分逻辑请求与上游尝试，持久化时间统一使用 UTC。
- 路由结果保留 `protocol_in → protocol_upstream → endpoint/Adapter` 完整链路及能力状态；原生能力优先，Adapter 只允许一次直接转换。未知或 `unsupported` 能力不得猜测为支持，允许的损失必须显式标记为 `degraded`。
- 模型拆分为 `ProviderPreset`、`SourceModel`、`LogicalModel`、`ModelPreset` 和 `PricingProfile`；模型发现必须经过预设补齐和用户确认，刷新不得静默覆盖已确认字段。
- 模型展示列表与实际可路由 Binding 分离；`/v1/models` 只公开已确认且至少有可用 Binding 的逻辑模型。`SourceModelCapability` 按 `(source_id, upstream_model_id, protocol)` 记录，待确认、不可用和未知能力不能被隐式公开或路由。
- 第一版管理端主导航为 Overview、Analysis、Request Events；Token 时序、模型/Provider/Source 分布和请求归因优先，价格/成本只是可选次级视图。CPA 专属的 Auth Files、Ranking、配额和充值功能不属于本项目产品面。
- 可参考 New API 的接入向导、模型发现差异预览、模型广场、参数覆盖、健康路由和用量交互，但只借鉴产品思路与字段语义，不直接复制其代码或页面；协议感知路由和本项目的能力矩阵优先。

详细架构、字段定义和流程说明以 [`docs/ai-gateway-design.md`](docs/ai-gateway-design.md) 为准；本文件只保留执行任务时必须遵守的约束，避免与设计文档长期重复。

### 开发阶段的变更原则

- 以当前任务工作分支的基线和 `docs/ai-gateway-design.md` 为准；若任务明确要求从 `main` 开始，再以 `main` 为基线。不为历史分支、旧配置或旧客户端保留隐式行为。
- 破坏性变更应一次性完成调用方、示例配置、迁移、文档和回归测试的更新；不要通过静默降级掩盖不匹配。
- 数据库 Schema 变化仍必须新增 migration，但 migration 面向当前开发基线的前进演进，不要求兼容尚未稳定的旧 Schema。
- 若任务明确要求兼容某个外部协议或 Provider，那是协议兼容目标，不等同于本项目内部 API 或配置的后向兼容承诺。
- 开始实现前优先确认对应的 GitHub Issue、分支和已有 PR；完成后在具备权限且得到授权时于 Issue/PR 中记录范围、提交、测试结果和未覆盖风险，否则在最终报告中给出完整同步摘要。临时本地说明只能作为工作材料，不能替代正式记录。

## 目录约定

```text
src/                         Rust 网关主程序
crates/kimi-responses-adapter/ 内置 Kimi Responses Adapter
migrations/                  PostgreSQL migration
docs/                        需求、架构、接口和运行文档
config.example.json          Provider/Account/Route 示例
```

## 技术栈

- Mise：工具链与任务管理（Rust、Node 版本固定在 `mise.toml`，常用任务见 `mise tasks`）；
- Tokio + Axum：HTTP/SSE 服务；
- Reqwest：上游 HTTP；
- Serde：协议和配置模型；
- SQLx + PostgreSQL：持久化；
- Tracing：日志；
- Prometheus/OpenTelemetry：后续可观测性；
- React + TypeScript：复用并适配 CPA Usage Keeper 的 Overview、Analysis、Request Events 页面交互。

## 配置和凭据

- 开发阶段初始化、导入和测试可使用 `GATEWAY_CONFIG_JSON`；运行期配置以 PostgreSQL 控制面为准；
- 生产凭据使用 `credential_env` 或加密后的数据库字段；
- 不要把真实 API Key 提交到仓库；
- `GATEWAY_API_KEY` 当前只是临时静态入口保护，后续必须替换为 PostgreSQL-backed Virtual Key。

## 验证命令

```bash
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo fmt --all -- --check
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo check
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo clippy --all-targets -- -D warnings
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo test
python3 -m json.tool config.example.json >/dev/null
```

以上命令用于验证，不应主动修改无关文件；需要修复格式时才单独运行 `CARGO_HOME=/tmp/my-ai-gateway-cargo cargo fmt --all`。如环境允许，也可以直接使用普通 `cargo` 命令。当前开发环境默认 Cargo 缓存目录可能不可写，因此优先使用临时 `CARGO_HOME`。工具链由 Mise 管理时，也可以用 `mise run lint`（Rust + 前端静态检查）和 `mise run verify`（静态检查 + 构建 + 测试）作为组合门禁。

## Adapter 开发要求

- 每个协议转换路径单独命名；
- 请求、非流式响应和流式事件分别测试；
- SSE 转换必须维护事件顺序和状态；
- 能力不支持时默认返回结构化错误；
- 允许降级时必须记录 warning 和 `degraded` 状态；
- Kimi Adapter 的 thinking、signature、tool call、web search 和 usage 变更必须增加回归测试。

## 数据库开发要求

- 所有 schema 变化必须新增 migration；
- `request_id` 用于 usage 事件幂等；
- Usage 事件必须同时保留 logical/requested model 与实际 upstream model，并区分逻辑请求和上游尝试；fallback 重试不得重复统计最终请求或 Token；
- Token 统计必须记录 `usage_source`；
- 不将正文日志作为统计系统的默认数据源；
- 统计查询必须支持按时间、模型、Provider、账号、协议和 Virtual Key 过滤。

## 安全要求

- 日志中禁止输出 Authorization、API Key 和完整请求正文；
- Provider URL 需要 allowlist，禁止客户端任意指定上游 URL；
- 上游凭据需要加密或通过 Secret 注入；
- Admin API 与下游 Virtual Key 分离；
- 账号代理功能必须遵守上游服务条款；
- 不实现 TLS 指纹伪装、请求伪装或反封禁能力。

## 文档和外部资料

- API、SDK 或框架文档查询必须优先使用 Context7；Context7 不可用时，使用相关项目的官方文档或仓库内已有资料，并在结果中说明替代来源；
- 架构、接口和实现基线维护在 `docs/ai-gateway-design.md`；问题、需求、任务状态、修复和评审记录维护在 GitHub Issues/PR；
- CPA Usage Keeper 只复用页面结构、React 组件和交互；不得复用其 Go 后端、SQLite、Redis queue、CPA Management API 或凭据/配额逻辑。复用代码时保留其 MIT License 和来源说明；New API 只借鉴思路，不复制其 AGPL 代码或 UI；
- 新增环境变量、接口或数据库字段时，必须同步更新文档；
- Python 辅助项目优先使用 `uv` 管理环境，除非用户明确要求其他工具。
