# my-ai-gateway 项目协作规范

## 项目定位

这是一个 Rust AI 网关项目，用于统一代理多个上游 Provider 和多个上游账号。

当前正式支持三类北向协议：

- OpenAI Chat Completions；
- OpenAI Responses；
- Anthropic Messages（Claude/Coze 客户端兼容面）。

## 核心架构约束

1. Provider 原生支持某协议时，必须优先原生透传。
2. Provider 不支持某协议时，使用明确的 Adapter。
3. MiniMax、DeepSeek 等三协议 Provider 不应进入转换器。
4. Kimi Responses 使用内置 `kimi-responses-adapter` workspace crate。
5. 首选账号固定优先，失败后才进入 fallback 账号池。
6. 不允许在协议转换中静默丢失 Tools、Web Search、Thinking、Usage 或 Provider 扩展字段。
7. 默认不保存 prompt/response 正文。
8. 不实现余额、充值、额度扣减或规避 Provider 风控的逻辑。

## 目录约定

```text
src/                         Rust 网关主程序
crates/kimi-responses-adapter/ 内置 Kimi Responses Adapter
migrations/                  PostgreSQL migration
docs/                        需求、架构、接口和运行文档
config.example.json          Provider/Account/Route 示例
```

## 技术栈

- Tokio + Axum：HTTP/SSE 服务；
- Reqwest：上游 HTTP；
- Serde：协议和配置模型；
- SQLx + PostgreSQL：持久化；
- Tracing：日志；
- Prometheus/OpenTelemetry：后续可观测性；
- React + TypeScript：后续复用 Keeper 统计 UI。

## 配置和凭据

- 开发配置使用 `GATEWAY_CONFIG_JSON`；
- 生产凭据使用 `credential_env` 或加密后的数据库字段；
- 不要把真实 API Key 提交到仓库；
- `GATEWAY_API_KEY` 当前只是临时静态入口保护，后续必须替换为 PostgreSQL-backed Virtual Key。

## 验证命令

```bash
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo fmt --all
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo check
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo clippy --all-targets -- -D warnings
CARGO_HOME=/tmp/my-ai-gateway-cargo cargo test
python3 -m json.tool config.example.json >/dev/null
```

如环境允许，也可以直接使用普通 `cargo` 命令。当前开发环境默认 Cargo 缓存目录可能不可写，因此优先使用临时 `CARGO_HOME`。

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

- API、SDK 或框架文档查询必须优先使用 Context7；
- 项目需求和实现状态统一维护在 `docs/ai-gateway-design.md`；
- 新增环境变量、接口或数据库字段时，必须同步更新文档；
- Python 辅助项目优先使用 `uv` 管理环境，除非用户明确要求其他工具。
