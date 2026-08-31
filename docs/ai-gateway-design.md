# AI Gateway 需求与实现状态

本文档是项目当前唯一的总体设计与进度基线，记录产品需求、架构决策、已经完成的实现、当前限制和后续工作。

当前详细 TODO 见 [todo.md](todo.md)。

## 1. 项目目标

构建一个面向个人自用的 Rust AI Provider 聚合端，将多个上游 Provider、模型来源和少量上游账号统一管理为一个服务入口，并为本地工具和应用提供统一调用方式。

产品形态是单用户、自托管、界面优先的聚合管理端，而不是多租户 API SaaS 或终端聊天应用。用户通常只有一个或两个上游账号，但会接入多个 Provider、多个模型来源和不同协议 endpoint。

核心原则：

1. 上游原生支持某协议时，优先原样透传。
2. 上游不支持某协议时，使用明确的协议 Adapter 转换。
3. 协议转换不能静默丢失工具、搜索、思考过程或其他扩展字段。
4. 同一逻辑模型可以绑定多个上游来源；默认来源优先，失败后才进入明确配置的 fallback 来源。
5. 统计重点是请求和 Token 使用，不实现余额、充值和额度扣减。

## 1.1 产品功能面

产品由四个相互关联的功能面组成：

1. **Provider/来源配置**：通过 UI 配置 Provider、base URL、凭据、模型、协议 endpoint 和能力。
2. **模型与路由管理**：维护逻辑模型到多个实际来源的绑定、优先级、fallback 和协议转换。
3. **统一调用入口**：对外提供 OpenAI Chat Completions、OpenAI Responses、Anthropic Messages。
4. **用量分析**：统计请求、Token、延迟、错误、来源和协议分布；第一版 UI 直接复用 Usage Keeper 的交互和页面结构，后续再按网关字段调整。

模型配置采用“发现 + 预设 + 用户确认”的交互：配置来源和协议 endpoint 后，可从上游发现模型；对于上游未返回的上下文窗口、工具、多模态和思考能力等元数据，由内置模型预设补齐，最后允许用户覆盖并确认。

第一版主要通过 Web UI 操作，API 用于客户端调用、自动化和后续扩展。数据库固定以 PostgreSQL 为主，MySQL 仅作为未来可能的兼容方向，不考虑 SQLite。

## 2. 协议范围

当前正式支持三类北向协议：

| 协议 | 网关入口 | 处理方式 |
| --- | --- | --- |
| OpenAI Chat Completions | `POST /v1/chat/completions` | Provider 原生支持时透传 |
| OpenAI Responses | `POST /v1/responses` | 原生透传或通过 Adapter 转换 |
| Anthropic Messages | `POST /v1/messages` | Claude/Coze 客户端兼容面 |

## 3. Provider 处理策略

### 3.1 上游能力矩阵

Provider/Account 来源配置必须显式描述每种北向协议的处理能力，而不能只配置一个总的 `native_protocols` 列表。

对每个来源，需要分别记录：

- `openai_chat_completions`：原生、转换或不支持；
- `openai_responses`：原生、转换或不支持；
- `anthropic_messages`：原生、转换或不支持。

当某协议不是原生能力时，必须同时配置转换来源协议和具体 Adapter。例如：

```json
{
  "protocol_capabilities": {
    "openai_chat_completions": {"mode": "native"},
    "openai_responses": {
      "mode": "adapter",
      "source_protocol": "openai_chat_completions",
      "adapter": "chat_to_responses"
    },
    "anthropic_messages": {
      "mode": "adapter",
      "source_protocol": "openai_chat_completions",
      "adapter": "chat_to_anthropic_messages"
    }
  }
}
```

约束如下：

- `native` 必须对应实际存在的上游 endpoint；
- `adapter` 必须声明 `source_protocol` 和 Adapter 名称；
- `source_protocol` 必须是该来源已声明为原生或配置了非空 endpoint 的协议；
- Adapter 名称、输入/输出协议方向必须匹配内置 Adapter 注册表；不允许多段转换或循环；
- 不支持且没有合法 Adapter 的协议，在路由解析阶段返回结构化错误；
- Tools、Web Search、Thinking、Usage 等能力也应按协议/来源分别声明，转换可能造成的能力损失必须显式标记。

配置模型使用 `protocol_capabilities` 矩阵和 `capabilities` 功能矩阵。协议模式为 `native`、`adapter` 或 `unsupported`；`adapter` 必须同时提供 `source_protocol` 与 `adapter`，且来源协议必须可解析。功能模式为 `native`、`translated` 或 `unsupported`。旧的 `native_protocols`/endpoint 仍可作为未填写协议矩阵时的默认推导。

Provider 是来源级默认值，账号和模型可以逐级覆盖。解析优先级为：`Account + Model` → `Provider + Model` → `Account` → `Provider` → legacy 默认值。示例：

```json
{
  "protocol_capabilities": {
    "openai_chat_completions": {"mode": "native"},
    "openai_responses": {"mode": "adapter", "source_protocol": "openai_chat_completions", "adapter": "chat_to_responses"},
    "anthropic_messages": {"mode": "unsupported"}
  },
  "capabilities": {"tools": "native", "thinking": "translated", "web_search": "unsupported"},
  "model_overrides": {
    "model-a": {
      "protocol_capabilities": {"anthropic_messages": {"mode": "native"}},
      "capabilities": {"thinking": "native"}
    }
  }
}
```

非法组合（例如 `native` 携带 adapter、`unsupported` 携带 source_protocol、adapter 缺少任一字段、未知 Adapter、方向不匹配、adapter 来源不可用或形成多段链/循环）会在 `GatewayConfig::validate()` 中返回带配置路径的结构化错误；不会静默降级。Adapter 注册表同时声明每个 feature 的 `native`、`translated` 或 `unsupported` 结果，未知能力保持 `unsupported`，不会被推断为支持。

路由解析接口 `GET /admin/routes/:protocol/:model` 返回完整的 `ResolvedRoute`：包括 `protocol_in`、`protocol_upstream`、实际 `upstream_endpoint`、Provider/首选与 fallback 账号、`mode`/`adapter`、能力交集 `effective_capabilities`、允许丢失时的 `degraded_features` 以及 `allow_lossy_conversion`。解析失败返回 `{error:{code,message,route_id}}`；native 优先于同等匹配的 adapter，精确模型优先于通配模型。

### 3.2 三协议原生 Provider

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

### 3.3 Kimi Code

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

### 3.4 模型发现与模型元数据

模型发现和模型能力描述是两个不同概念：

- **发现**回答“这个来源当前能看到哪些模型”；
- **元数据**回答“这个模型的上下文、输出限制、工具和多模态能力是什么”。

UI 流程为：

```text
配置 Source 和协议 endpoint
  ↓
测试连接
  ↓
调用厂商预设定义的模型发现接口
  ↓
展示发现结果
  ↓
匹配内置模型预设并补齐元数据
  ↓
用户选择、编辑并确认
```

模型元数据至少包括：

- 上游模型 ID 和逻辑模型名；
- 上下文窗口和最大输出 Token；
- 输入/输出模态（文本、图片、音频等）；
- Tools、Thinking/Reasoning、Web Search、Structured Output；
- Streaming 和 Usage 支持；
- 元数据来源：`upstream`、`preset`、`user` 或 `unknown`。

字段优先级为：用户覆盖 > 模型预设 > 上游发现 > unknown。刷新模型时不得覆盖用户已经确认的字段；新发现的模型先进入待确认列表，不自动改变现有路由。

### 3.5 模型目录持久化基线

模型目录使用独立领域表，和当前 `ProviderConfig.models`、`routes` 运行时配置分离：

```text
ProviderPreset(version) ──创建时复制──> Source snapshot ──> Account
                                              │
                                              └──> SourceModel
                                                    │
ModelPreset(version) ──补齐元数据───────────────────┤
                                                    ↓
LogicalModel <──────────────────────────── ModelBinding
                                                    │
                                                    └──> SourceModelCapability(protocol)
```

- `provider_presets` 以 `(id, version)` 唯一，版本内容不可原地覆盖；`sources` 保存创建时的预设 ID、版本和完整 JSON 快照，后续预设升级不修改已有 Source。
- `model_presets` 以 `(id, version)` 唯一，保存 canonical model ID、别名、元数据和值来源；它只提供默认元数据，不能替代 Source 的实际协议能力。
- `source_models` 以 `(source_id, upstream_model_id)` 唯一，分别保存确认状态 `pending/confirmed`、可用状态 `unknown/available/unavailable`、原始发现快照、解析后元数据和每个字段的来源。
- `logical_models` 保存对外公开名及 `pending/confirmed/unavailable` 状态；上游模型 ID 与逻辑模型名不要求相同。
- `source_model_capabilities` 以 `(source_id, upstream_model_id, protocol)` 唯一，协议模式为 `unknown/native/adapter/unsupported`。`unknown` 和 `unsupported` 都不是可路由能力；Adapter 仍只允许一次直接转换，并要求其来源协议已确认原生可用。
- `model_bindings` 显式关联 LogicalModel、Source、Account、upstream model 和入口协议。Binding 初始为 `pending`；只有逻辑模型、Source、Account、SourceModel 和对应协议能力都已确认且可用时，数据库才允许转为 `confirmed`。

模型元数据字段第一版包括逻辑名、显示名、上下文窗口、最大输入/输出 Token、输入/输出模态、Tools、Thinking、Web Search、Structured Output、Streaming 和 Usage。每个字段来源只能是 `user/preset/upstream/unknown`，合并优先级固定为：

```text
user > preset > upstream > unknown
```

重复刷新同一 `(source_id, upstream_model_id)` 只更新原始发现快照、最近发现时间和可用状态，不创建重复记录。待确认记录会重新应用预设和上游元数据，但保留用户覆盖；已确认记录的元数据和匹配预设均保持不变。发现中消失的模型只标记 `unavailable`，不删除 LogicalModel、Binding 或 Route。

本阶段只提供 migration、领域类型和仓储查询，不改变现有 Route，也不把发现结果自动写入 `logical_models`、`model_bindings` 或 `routes`。`/v1/models` 和运行时路由切换到 PostgreSQL 模型目录属于后续 PostgreSQL-backed 控制面任务。

开发期 `GATEWAY_CONFIG_JSON` 导入会为尚不存在的 Provider ID 创建一次 `custom@1` Source 快照，并让 Account 显式引用该 Source；后续启动同步不会覆盖已经存在的 Source 快照或用户编辑，PostgreSQL 仍是模型目录事实来源。

### 3.6 自定义渠道与跨 Provider Fallback（待实施）

现状：`fallback_accounts` 只允许同 Provider 账号（`src/main.rs` 按 `provider_id` 过滤），请求体 `model` 原样透传，无法把"同一模型的其他渠道"作为备用。目标：fallback 账号可以是任意 Provider 的账号，用于接入 b.ai、硅基流动等 OpenAI 兼容渠道做兜底。

规则：

- fallback 转发使用候选账号自己 Provider 的 `base_url`/`endpoints`/凭据；attempt 与最终 usage 事件记录实际的 `provider_id`/`account_id`/`upstream_model_id`。
- 跨 Provider fallback 仅限 `native` 链路：候选 Provider 必须对该入站协议声明 `native` 且配置了非空 endpoint；`adapter` 路由不允许跨 Provider fallback（配置校验拒绝，不做静默降级）。
- `AccountConfig` 新增可选 `model_map`（逻辑模型 → 上游模型 ID）：转发前重写请求体顶层 `model` 字段；未命中映射则原样透传；重写结果记入 attempt 的 `upstream_model_id`。
- 配置校验：fallback 账号/Provider 必须存在；跨 Provider 且候选 Provider 未声明该模型时，要求候选账号 `model_map` 存在对应映射；错误信息带配置路径。
- 首选账号冷却中或被禁用时进入 fallback 候选选择，无可用候选才返回 503（修正现状直接 503 的行为）。
- 仍为单次 fallback 重试（首选 1 次 + fallback 1 次），不引入多轮循环；fallback 候选选择对 429/5xx/传输错误路径统一执行 enabled + 健康 + 权重过滤（修正传输错误路径不过滤的现状）。

渠道资料（2026-08 确认，凭据只通过环境变量注入，禁止写入配置或文档）：

- **b.ai**：`https://api.b.ai/v1`，OpenAI 兼容（Chat Completions）。免费模型：`deepseek-v4-flash`（tool_use、thinking）、`deepseek-v4-flash-vision-exp`（另含 image_in）、`glm-5.3-flash`（tool_use、always_thinking、image_in、video_in）、`qwen3.8-flash`。凭据环境变量 `B_AI_API_KEY`。
- **硅基流动（SiliconFlow）**：`https://api.siliconflow.cn`，OpenAI 兼容。凭据环境变量 `SILICONFLOW_API_KEY`。
- 跨渠道重叠模型（用于 fallback 测试）：`deepseek-v4-flash`（DeepSeek 官方 ↔ b.ai）、MiniMax-M2 系列（MiniMax 官方 ↔ 硅基流动）。

配置示例：

```json
{
  "providers": [
    {
      "id": "bai",
      "name": "b.ai",
      "base_url": "https://api.b.ai/v1",
      "models": ["deepseek-v4-flash", "glm-5.3-flash"],
      "native_protocols": ["openai_chat_completions"],
      "endpoints": {"openai_chat_completions": "/chat/completions"},
      "capabilities": {"streaming": "native", "tools": "native", "thinking": "native", "usage": "native"},
      "protocol_capabilities": {
        "openai_chat_completions": {"mode": "native"},
        "openai_responses": {"mode": "unsupported"},
        "anthropic_messages": {"mode": "unsupported"}
      },
      "model_overrides": {}
    }
  ],
  "accounts": [
    {"id": "bai-main", "provider_id": "bai", "display_name": "b.ai 免费渠道",
     "credential_env": "B_AI_API_KEY", "enabled": true, "weight": 50,
     "model_map": {"deepseek-chat": "deepseek-v4-flash"}}
  ],
  "routes": [
    {
      "id": "deepseek-all-native",
      "model": "deepseek-*",
      "provider_id": "deepseek",
      "protocols": ["openai_chat_completions"],
      "primary_account_id": "deepseek-main",
      "fallback_accounts": ["bai-main"],
      "mode": "native"
    }
  ]
}
```

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
- 账号失败会进入 30 秒内存冷却窗口，服务重启后状态会丢失。

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
    "streaming": "native",
    "tools": "native",
    "thinking": "native",
    "web_search": "native",
    "usage": "native"
  },
  "protocol_capabilities": {
    "openai_chat_completions": {"mode": "native"},
    "openai_responses": {"mode": "native"},
    "anthropic_messages": {"mode": "native"}
  },
  "model_overrides": {}
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
  "weight": 100,
  "capabilities": {"tools": "native", "thinking": "translated"},
  "protocol_capabilities": {},
  "model_overrides": {
    "model-a": {"protocol_capabilities": {"openai_responses": {"mode": "unsupported"}}}
  }
}
```

生产环境使用 `credential_env`，不建议在 JSON 中直接写 `credential`。账号可选 `model_map`（逻辑模型 → 上游模型 ID）用于跨 Provider fallback 时重写模型名，规则见 3.6。

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
- 启动时按 migration 初始化 `usage_events`、attempt、Virtual Key 和查询索引；
- 请求结束后写入基础请求事件；
- `request_id` 唯一防重复。

### Virtual Key 与统计 API

- `POST /admin/keys` 创建 Virtual Key；
- `GET /admin/keys` 查询 Key；
- `POST /admin/keys/:id/revoke` 撤销 Key；
- Key 使用 SHA-256 哈希存储，原始值只在创建时返回；
- 支持 `allowed_models` 模型白名单；
- 成功鉴权后更新 `last_used_at`；
- `GET /admin/usage/summary` 返回逻辑请求、上游尝试、重试、成功/失败、延迟和 Token 汇总。
- `GET /admin/usage/timeseries?granularity=hour|day` 返回 UTC 小时/日时间桶。
- `GET /admin/usage/breakdown?breakdown=...` 支持 `logical_model`、`upstream_model`、`provider`、`source`、`account`、`protocol_in`、`protocol_upstream`、`virtual_key`、`status` 和 `usage_source`。
- `GET /admin/usage/events?limit=100&cursor=...` 使用 `(created_at DESC, request_id DESC)` 的确定性 keyset 游标，`limit` 范围为 `1..500`。
- `GET /admin/usage/export?format=csv|json` 按与 events 相同的筛选和排序导出全部匹配事件；不包含 prompt/response 正文。
- `GET /admin/usage/aggregate` 保留为 summary、timeseries 和单一 breakdown 的组合入口，响应与独立入口共享 `version: v1` 契约。

所有 Usage 查询共享组合筛选参数：`from`、`to`、`logical_model`、`upstream_model`、`provider`、`source`、`account`、`protocol_in`、`protocol_upstream`、`virtual_key`、`status`、`status_code` 和 `usage_source`。`from`/`to` 接受带 offset 的 RFC3339，服务端转换为 UTC，并以半开区间 `[from,to)` 解释；响应桶固定为 UTC，UI 只在展示层换算本地时区。`source` 来自可选的下游 `X-Client-Source` 请求头，缺省为 `unknown`；该字段仅用于统计维度，不改变路由或认证。

v1 响应 envelope 固定如下：summary 为 `{version, timezone, range, data}`；timeseries 额外返回 `granularity`，每个 `data` 元素包含 UTC `bucket`；breakdown 额外返回 `dimension`，每个元素使用可空 `key` 表示分组值；events 返回 `{data, page:{limit, has_more, next_cursor}}`。聚合指标统一包含 `logical_requests`、`upstream_attempts`、`retries`、`successes`、`failures`、`success_rate`、`average_latency_ms`、`p95_latency_ms` 和五类 Token；breakdown 另含 `logical_request_share`、`total_token_share`。客户端应把 `next_cursor` 视作不透明值并原样传回。

聚合中的 `logical_requests`、成功/失败、延迟和 Token 来自筛选后的 `usage_events`，因此每个逻辑请求和最终 Usage 只累计一次。`upstream_attempts` 来自这些逻辑请求关联的 `usage_event_attempts`；`retries` 来自逻辑事件的重试计数。Provider、Source、Account、协议等筛选先选择逻辑请求，再统计其关联 attempt，避免把失败 fallback 的 Token 当成已确认 Usage。`usage_source=missing` 的请求保留请求数但 Token 为零。

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

当前已经创建 `usage_events`、`usage_event_attempts`、`virtual_keys`、`providers`、`accounts`、`routes`，以及 Provider/Model preset、Source、SourceModel、LogicalModel、ModelBinding、SourceModelCapability 模型目录表。现有配置会前进回填为 `custom` ProviderPreset 的独立 Source 快照，但运行时仍继续使用当前配置路径，直到 PostgreSQL-backed 控制面任务完成。`request_id` 表示一次北向逻辑请求并保持唯一；重试尝试写入 `usage_event_attempts(request_id, attempt_no)`，同一尝试幂等。`usage_events.logical_model` 保存客户端模型，`upstream_model_id` 在路由能明确提供时填充，否则为空；Virtual Key 鉴权成功时写入 `virtual_key_id`，静态入口 Key 保持为空。时间统一按 PostgreSQL `TIMESTAMPTZ` 以 UTC 存储，展示层负责本地时区转换。

模型目录数据库回归测试只连接显式的 `TEST_DATABASE_URL`，不会复用运行时 `DATABASE_URL`；未设置时普通单元测试跳过 PostgreSQL 集成部分。

- `health_snapshots`；
- `audit_logs`。

### 7.3 Token 统计

已完成非流式 JSON usage 提取、SSE 末事件解析和异步落库，支持 OpenAI Chat/Responses、Anthropic Messages 字段；usage 缺失时使用 `tiktoken-rs` 的 `cl100k_base` 估算并标记为 `estimated`。仍待完成：

- TTFT 和真实流式完成时间记录。

2026-08-31 真实联调发现的 usage 提取缺口（并入 #25、#26 处理）：

- Adapter（Responses → Anthropic）非流式响应的 usage 未落库：上游转换后的响应体含 usage，事件却记为 `missing` 且 0/0；
- Kimi `/v1/messages` 非流式响应只提取到 output_tokens，input_tokens 为 0；
- MiniMax/Kimi 流式无 usage 末事件时 tiktoken 估算值明显膨胀（如实际短回复估算出上千 output tokens）；
- `reasoning_tokens` 恒为 0：MiniMax Responses 的 reasoning item、Anthropic thinking 的 token 均未提取；
- 失败请求（如上游 401）也做了 token 估算并标记 `estimated`，语义待确认。

### 7.4 统计接口和页面

已完成稳定 v1 `/admin/usage/summary`、`timeseries`、`breakdown`、`events`、`export` 查询契约、组合筛选、确定性游标分页、CSV/JSON 导出，以及 Provider/Account/Route 管理查询 API；同时已 vendor Keeper React 前端、构建静态资源（访问 `/admin/`）。仍待完成：

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

已完成账号健康的内存冷却和基础 usage events 查询；仍待持久化健康状态、完整筛选和生产监控。

### 7.6 测试

- 已增加 Kimi 内置 Adapter 的 mock 上游端到端测试；
- 已覆盖非流式 thinking/web search 转换；
- 已覆盖流式 Anthropic SSE → Responses SSE；
- 已增加 OpenAI/Anthropic usage JSON 和 SSE 提取单测。

### 7.7 真实联调基线（2026-08-31）

已用真实上游（MiniMax、DeepSeek、Kimi Code）跑通并落库验证：

- 三协议原生透传全部可用，包括 MiniMax `/v1/responses`、DeepSeek `/v1/responses` 与 `/anthropic/v1/messages`（两家 Anthropic 兼容端点均为 `{base_url}/anthropic/v1/messages`，`config.example.json` 已修正）；
- Kimi Responses → Anthropic adapter 非流式/流式可用，SSE 事件序列完整；
- usage 落库、`/admin/usage/*` 聚合、Admin 控制台 Overview/Analysis 展示与 DB 一致；`protocol_in → protocol_upstream → mode` 链路记录正确；
- 失败请求（上游 401）透传并记录 `success=false`，且不触发 fallback——符合 `is_retryable` 仅认 408/429/5xx 的语义；429/5xx 触发 fallback 的完整链路尚无真实环境验证手段，需要 mock 级 e2e 补齐；
- `upstream_model_id` 恒为空、TTFT 未实现、fallback 最多一次重试且首选冷却时直接 503——均属已知缺口；
- `web/dist` 需随前端源码重建，旧构建仍会调 CPA Keeper 的 `/api/v1/auth/*`（网关无此端点）。

## 8. 验收标准

### 原生 Provider

- MiniMax 三种协议均可透传；
- DeepSeek 三种协议均可透传；
- Web Search、Tools、Thinking 字段不被网关修改；
- 上游 SSE 可被客户端持续读取；
- 429/5xx 可触发 fallback。

### Fallback 渠道

- 429/5xx/传输错误时可 fallback 到不同 Provider 的账号，响应来自候选渠道；
- 转发 body 的 `model` 按候选账号 `model_map` 重写，attempt 记录实际 provider 与 upstream_model_id；
- adapter 路由配置跨 Provider fallback 时被校验拒绝；
- 首选账号冷却/禁用时自动进入 fallback 候选；
- 上述链路有 mock 级端到端测试（非流式 + 流式）。

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
