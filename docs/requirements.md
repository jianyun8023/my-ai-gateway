# AI 网关需求与 MVP 设计

> 本文件保留为历史入口；完整、最新的需求、架构、已完成和待办状态请阅读 [ai-gateway-design.md](ai-gateway-design.md)。

## 1. 目标与边界

网关为多个上游账号提供统一入口，并向内部服务签发多个 Virtual Key。第一阶段只做鉴权、账号路由、协议透传/转换和 Token 统计；不做余额、充值、计费或额度扣减。

“不做额度管理”不等于“不记录上游限流”：账号的 429、并发限制和健康状态仍需采集，用于故障转移和运维告警。

## 2. 北向协议

| 协议 | 建议入口 | 说明 |
| --- | --- | --- |
| OpenAI Chat Completions | `POST /v1/chat/completions` | `messages`、tool calls、流式 delta |
| OpenAI Responses | `POST /v1/responses` | `input`、typed output items、Responses SSE events |
| Anthropic Messages | `POST /v1/messages` | Claude/Claude Code 使用的 Messages 与 SSE |

OpenAI Chat 与 Responses 的输入/输出模型并不等价，因此转换层使用指定路径的 provider adapter，不能靠简单字段重命名。

## 3. 请求处理链

```text
客户端 → Virtual Key 鉴权 → 协议识别 → 模型/路由匹配
      → capability matrix（原生透传优先）
      → 上游账号选择 → 原生请求或 IR 转换
      → SSE/JSON 响应转换 → usage_events 异步落库
```

路由规则按以下顺序解析：精确的“协议 + 模型 + 账号”绑定、模型级默认账号、provider 默认账号。若绑定账号不可用，默认快速失败；只有显式开启 fallback 才允许切换到其他账号，避免请求悄悄落到错误账号。

对于 MiniMax、DeepSeek 这类三种协议均由上游原生提供的渠道，三种入口全部走原生透传，Web Search、Tools、Thinking、Usage 和 provider 扩展字段不进入转换器。Kimi Code 的 Responses 入口才使用专用 `kimi_responses_adapter`。

## 4. 核心领域模型

- `Provider`：上游服务、base URL、支持的协议、模型目录。
- `Account`：provider 下的 API Key/OAuth 凭据、启用状态、健康状态、冷却时间。
- `VirtualKey`：只保存哈希；原始值创建时显示一次；可撤销、轮换、绑定模型/路由组。
- `Route`：协议、模型模式、首选账号、fallback 账号、adapter 名称。
- `UsageEvent`：请求、Key、账号、provider、模型、协议、状态、延迟和 Token 计数。

第一阶段先实现指定路径的转换器：Chat ↔ Anthropic Messages、Responses ↔ Anthropic Messages。转换器必须保留消息、多模态、工具、reasoning/thinking、停止原因、usage 和流式事件；无法转换时返回显式错误，不静默丢弃。

## 5. Token 统计

建议 `usage_events` 字段：`request_id`、`virtual_key_id`、`account_id`、`provider_id`、`model`、`protocol_in`、`protocol_out`、`status_code`、`retry_count`、`ttft_ms`、`latency_ms`、`input_tokens`、`output_tokens`、`reasoning_tokens`、`cached_tokens`、`total_tokens`、`usage_source`（upstream/parsed/estimated）、`created_at`。

统计页面先提供：按时间、Key、账号、provider、模型、协议筛选；请求数、成功率、平均/TP95 延迟、Token 总量和最近错误。默认不保存 prompt/response 正文，必要时通过受控开关和脱敏策略开启。

## 6. PostgreSQL 最小表

`providers`、`accounts`、`virtual_keys`、`routes`、`usage_events`、`health_snapshots`、`audit_logs`。凭据使用应用层信封加密；Virtual Key 使用不可逆哈希并加唯一索引；usage 写入可通过异步队列批量提交。

## 7. Rust 实现建议

- HTTP/SSE：Tokio + Axum + Reqwest。
- JSON/协议模型：Serde；每种协议独立 request/response/event 类型。
- 数据库：SQLx + PostgreSQL，迁移文件纳入版本控制。
- 可观测性：tracing，Prometheus/OpenTelemetry 指标。
- 适配器接口：`ProviderAdapter::capabilities()`、`send()`、`stream()`；Kimi Responses 适配器复用现有 `kimi-responses-adapter`。

## 8. 分阶段交付

1. **MVP 数据面**：入口、Virtual Key 鉴权、固定账号路由、原生透传、健康检查。
2. **转换层**：统一 IR、OpenAI Chat/Responses ↔ Anthropic Messages，接入 Kimi 适配器。
3. **持久化与统计**：PostgreSQL、usage_events、聚合查询和最小管理 API/UI。
4. **生产化**：账号健康/冷却、可选 fallback、SSE 心跳、OTel、凭据轮换和审计。

## 9. 未决定义

- Virtual Key 是否允许绑定账号组，还是只能绑定模型/路由组。
- Kimi、MiniMax 等账号凭据是 API Key 还是 OAuth；这会决定加密字段和刷新任务。
- 是否需要保存原始请求/响应正文，以及保存时长和脱敏规则。
