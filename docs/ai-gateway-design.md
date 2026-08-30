# AI Gateway 需求与实现状态

本文档是项目当前唯一的总体设计与进度基线，记录产品需求、架构决策、已经完成的实现、当前限制和后续工作。

## 1. 项目目标

构建一个 Rust AI 网关，将多个上游 Provider 和多个上游账号统一代理为一个服务入口，并为内部服务提供多个下游访问 Key。

核心原则：

1. 上游原生支持某协议时，优先原样透传。
2. 上游不支持某协议时，使用明确的协议 Adapter 转换。
3. 协议转换不能静默丢失工具、搜索、思考过程或其他扩展字段。
4. 主账号优先，失败后才进入 fallback 账号池。
5. 统计重点是请求和 Token 使用，不实现余额、充值和额度扣减。

## 2. 协议范围

当前正式支持三类北向协议：

| 协议 | 网关入口 | 处理方式 |
| --- | --- | --- |
| OpenAI Chat Completions | `POST /v1/chat/completions` | Provider 原生支持时透传 |
| OpenAI Responses | `POST /v1/responses` | 原生透传或通过 Adapter 转换 |
| Anthropic Messages | `POST /v1/messages` | Claude/Coze 客户端兼容面 |

本项目不单独实现 Coze Bot/Workflow API，也不把它作为第四种协议。

## 3. Provider 处理策略

### 3.1 三协议原生 Provider

MiniMax、DeepSeek 等 Provider 如果同时提供三种协议接口，则为每个协议配置对应 endpoint：

```text
/v1/chat/completions → Provider Chat 接口
/v1/responses        → Provider Responses 接口
/v1/messages         → Provider Anthropic 接口
```

网关仅负责：

- 下游 Key 鉴权；
- 模型和路由匹配；
- 上游账号选择；
- 上游凭据替换；
- 超时和失败转移；
- JSON/SSE 响应转发；
- usage 采集。

Web Search、Tools、Thinking、Vision、Provider 扩展字段在原生透传路径中保持原始 JSON 和 SSE 语义。

### 3.2 Kimi Code

Kimi Code 当前按以下方式处理：

```text
Chat Completions       → 原生透传
Anthropic Messages     → 原生透传
OpenAI Responses       → 内置 kimi-responses-adapter
```

`kimi-responses-adapter` 已作为 workspace crate 内置到当前二进制中，不再依赖独立容器或 `adapter_base_url`。

Adapter 负责：

- Responses 请求转换为 Anthropic Messages；
- Anthropic 响应转换为 Responses；
- thinking/signature；
- function call/function call output；
- web search；
- Responses SSE；
- usage 映射；
- `response.incomplete`。

## 4. 请求处理流程

```text
客户端请求
  ↓
入口协议识别
  ↓
GATEWAY_API_KEY 鉴权
  ↓
读取 model
  ↓
匹配 protocol + model Route
  ↓
检查 Provider 原生能力
  ├─ native → 原生透传
  └─ adapter → 调用内置 Adapter
  ↓
主账号请求
  ├─ 成功 → 返回客户端
  └─ 408/429/5xx/网络错误 → fallback
  ↓
记录 usage_events
```

当前 fallback 规则：

- 首选账号固定优先；
- HTTP 408、429、5xx 和网络错误允许 fallback；
- fallback 账号必须启用且属于同一 Provider；
- 响应已经开始流式输出后不能切换账号；
- 当前实现对 HTTP 响应 fallback 使用权重选择，对网络错误暂按 fallback 列表第一项重试，后续需要统一为完整权重策略。

## 5. 当前配置模型

配置入口为 `GATEWAY_CONFIG_JSON`，完整示例见 [`config.example.json`](../config.example.json)。

### Provider

```json
{
  "id": "minimax",
  "name": "MiniMax",
  "base_url": "https://provider.example.com",
  "models": ["MiniMax-M2.7"],
  "native_protocols": [
    "openai_chat_completions",
    "openai_responses",
    "anthropic_messages"
  ],
  "endpoints": {
    "openai_chat_completions": "/v1/chat/completions",
    "openai_responses": "/v1/responses",
    "anthropic_messages": "/v1/messages"
  },
  "capabilities": {
    "streaming": true,
    "tools": true,
    "thinking": true,
    "web_search": true,
    "usage": true
  }
}
```

### Account

```json
{
  "id": "minimax-main",
  "provider_id": "minimax",
  "display_name": "MiniMax 主账号",
  "credential_env": "MINIMAX_API_KEY",
  "enabled": true,
  "weight": 100
}
```

生产环境使用 `credential_env`，不建议在 JSON 中直接写 `credential`。

### Route

原生透传：

```json
{
  "id": "minimax-all-native",
  "model": "MiniMax-M2.7",
  "provider_id": "minimax",
  "protocols": [
    "openai_chat_completions",
    "openai_responses",
    "anthropic_messages"
  ],
  "primary_account_id": "minimax-main",
  "fallback_accounts": ["minimax-backup"],
  "mode": "native"
}
```

Kimi Adapter：

```json
{
  "id": "kimi-responses-adapter",
  "model": "kimi-for-coding-highspeed",
  "provider_id": "kimi_code",
  "protocols": ["openai_responses"],
  "primary_account_id": "kimi-main",
  "mode": "adapter",
  "adapter": "kimi_responses_adapter",
  "allow_lossy_conversion": false
}
```

## 6. 已完成实现

### 数据面

- Axum HTTP 服务；
- `/healthz`；
- `/v1/models`；
- `/v1/chat/completions`；
- `/v1/responses`；
- `/v1/messages`；
- 原生 JSON/SSE 上游透传；
- 自定义 Provider endpoint；
- 上游凭据替换；
- `GATEWAY_API_KEY` 基础鉴权。

### 路由

- Provider、Account、Route 配置模型；
- 精确模型匹配；
- `*` 前缀模型匹配；
- 主账号优先；
- fallback 账号；
- 权重选择；
- Adapter/native 模式区分。

### Kimi Adapter

- 以 workspace crate 内置；
- 网关进程内直接调用 Adapter Router；
- 不再需要独立 Adapter 服务；
- 保留 Kimi 原项目 MIT License。

### PostgreSQL 基础

- `DATABASE_URL` 可选；
- 启动时自动创建 `usage_events` 表；
- 请求结束后写入基础请求事件；
- `request_id` 唯一防重复。

### Virtual Key 与统计 API

- `POST /admin/keys` 创建 Virtual Key；
- `GET /admin/keys` 查询 Key；
- `POST /admin/keys/:id/revoke` 撤销 Key；
- Key 使用 SHA-256 哈希存储，原始值只在创建时返回；
- 支持 `allowed_models` 模型白名单；
- 成功鉴权后更新 `last_used_at`；
- `GET /admin/usage/summary` 返回基础请求数、成功数和 Token 汇总。

### 部署

- [Dockerfile](../Dockerfile)；
- [docker-compose.yml](../docker-compose.yml)；
- PostgreSQL 16；
- Gateway + PostgreSQL 单机部署结构。

## 7. 当前未完成工作

### 7.1 Virtual Key 正式系统

已完成数据库-backed Key 创建、列表、撤销和模型白名单鉴权。仍待完成：

- Key 轮换；
- Key 分组和路由白名单；
- 更完整的 Admin Session 与审计。

### 7.2 PostgreSQL 领域表

当前已经创建 `usage_events` 和 `virtual_keys` 表，仍待增加：

- `providers`；
- `accounts`；
- `routes`；
- `health_snapshots`；
- `audit_logs`。

### 7.3 Token 统计

已完成非流式 JSON usage 提取和统一归一化，支持 OpenAI Chat/Responses、Anthropic Messages 字段，并增加 SSE 末事件解析函数。当前非流式请求已将 usage 写入 `usage_events`；仍待完成：

- Chat Completions 流末 usage 解析；
- Responses 流末 usage 解析；
- tokenizer 估算；
- TTFT 和真实流式完成时间记录。

### 7.4 统计接口和页面

待完成：

- Usage Overview API；
- Analysis API；
- Events 查询 API；
- 时间、模型、Provider、账号、Key 筛选；
- CSV/JSON 导出；
- Keeper React UI vendor；
- Keeper UI 字段改为网关原生字段；
- Admin Session 登录。

CPA Usage Keeper 只复用 React 页面和交互，不复用其 Go 后端、SQLite、CPA Redis queue 或 CPA Management API。[CPA Usage Keeper](https://github.com/Willxup/cpa-usage-keeper)

### 7.5 账号健康和生产化

- 账号健康状态持久化；
- 冷却时间；
- 连续失败计数；
- 完整 fallback 权重策略；
- 上游连接池和超时分级；
- Prometheus/OpenTelemetry；
- 凭据加密存储；
- 审计日志；
- SSRF 防护和 Provider URL allowlist。

### 7.6 测试

- 已增加 Kimi 内置 Adapter 的 mock 上游端到端测试；
- 已覆盖非流式 thinking/web search 转换；
- 已覆盖流式 Anthropic SSE → Responses SSE；
- 已增加 OpenAI/Anthropic usage JSON 和 SSE 提取单测。

## 8. 验收标准

### 原生 Provider

- MiniMax 三种协议均可透传；
- DeepSeek 三种协议均可透传；
- Web Search、Tools、Thinking 字段不被网关修改；
- 上游 SSE 可被客户端持续读取；
- 429/5xx 可触发 fallback。

### Kimi

- Chat Completions 原生透传；
- Anthropic Messages 原生透传；
- Responses 使用内置 Adapter；
- thinking/signature 保留；
- function call 保留；
- web search 保留；
- 非流式和流式均可工作。

### 统计

- 每个请求有唯一 request_id；
- 成功和失败请求都可查询；
- 能按协议、模型、Provider、账号和 Key 聚合；
- 上游 usage 优先；
- 缺失 usage 时明确标记估算或缺失。

## 9. 开发顺序

1. 完成 Virtual Key 轮换、分组和更完整的 Admin API；
2. 扩展 PostgreSQL providers/accounts/routes 表；
3. 完成流式 usage 提取、tokenizer 估算和异步落库；
4. 接入 Keeper React UI；
5. 扩展 Kimi Adapter 端到端测试矩阵；
6. 增加账号健康、冷却和完整 fallback；
7. 增加生产环境安全、监控和备份。
