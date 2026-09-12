# AI Gateway 需求与实现状态

本文档是项目当前唯一的总体设计与进度基线，记录产品需求、架构决策、已经完成的实现、当前限制和后续工作。

当前详细 TODO 见 [todo.md](todo.md)。

后端模块职责、事务边界、公共能力和治理验证记录见 [后端分层与维护约束](backend-architecture.md)。

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

上游响应的 HTTP 压缩由网关协商：代理不转发客户端的 `Accept-Encoding`，共享 HTTP client 按已启用的解码能力协商 gzip，并在 JSON usage 解析、SSE 事件跟踪与转发前解压。解码后的响应移除原 `Content-Encoding` / `Content-Length`，保留解码后的正文内容及协议字段；gzip SSE 逐块解码，继续遵守首事件、空闲、总时限和取消约束。压缩体读取/校验失败按已有上游错误路径处理，不将压缩字节估算成成功请求 Token（#185）。

SSE 用量按字段是否由上游提供进行合并：显式 `0` 覆盖旧值，缺失字段保留先前值；已知数值字段全部为零仍属于 `parsed`，不回退估算。缓存读写先分别合并，再计算 cached；input/output 发生更新且本帧未提供 total 时，使用合并后的 input + output，避免沿用较早事件的 total。当前帧明确提供的 total 保持上游值，缓存及 reasoning 不重复计入推导 total（#188）。

### 3.3 Kimi Code

Kimi Code 官方于 2026-09 原生支持 OpenAI Responses（`POST /coding/v1/responses`，2026-09-06 实测返回标准 Responses 语义：reasoning item、流式 function_call 事件、`usage.input_tokens_details.cached_tokens`、`incomplete_details`），三协议全部按原生透传处理：

```text
Chat Completions       → 原生透传
Anthropic Messages     → 原生透传
OpenAI Responses       → 原生透传（/v1/responses，preset kimi_code@4 起）
```

原内置 `kimi-responses-adapter`（Responses → Anthropic Messages）已随 #157 移除：workspace crate、运行时 dispatch 和 `kimi_code@1`-`@3` 的 adapter 声明退役，存量 Source 由 migration 0023 迁移到 native。Adapter 框架（注册表校验、mode 管线、degraded 语义）保留给后续 Provider 接入；当前生产注册表为空，声明任何 adapter 都会在配置与 snapshot 校验时 fail closed。

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

当前实现内置版本化 ProviderPreset：`deepseek@1`、`minimax@1`、`kimi_code@1` 保留为历史不可变快照；DeepSeek/MiniMax 最新为 `@3`，Kimi Code 最新为 `@4`。`@2` 记录已验证的 Responses `web_search` 与 Kimi Responses Adapter 的 `tool_streaming`；`@3` 进一步显式记录 Responses 的 `web_search_citations` 与 `web_search_sources`：DeepSeek/MiniMax 当前声明为 `unsupported`，Kimi（当时走 Adapter）声明为 `supported`。`kimi_code@4`（#157）将 Responses 切换为官方原生 `/v1/responses`：`web_search` 与 `tool_streaming` 声明为 `supported`，原生端点的 citation/source 可见性未验证、保持 `unknown`，不做猜测。启动注册新版本不会改写已经创建的 Source 快照。ProviderPreset 完整声明默认 Base URL、三协议 endpoint/模式、认证和 Header 模板、最小连接测试请求、默认能力及发现规则；Kimi Code 因官方未提供已认证模型列表 endpoint，明确声明 `discovery.support=unsupported`，不会猜测接口。首批 ModelPreset 包括 DeepSeek V4、MiniMax M3/M2.7、Kimi K3/K2.7 Code Model。

管理 API 流程为：

```text
GET  /admin/provider-presets
POST /admin/sources
POST /admin/sources/:source_id/connection-tests
POST /admin/sources/:source_id/discoveries
GET  /admin/sources/:source_id/discoveries/latest
GET  /admin/sources/:source_id/models?confirmation_status=pending
PATCH /admin/sources/:source_id/models
POST /admin/sources/:source_id/models/confirm
```

连接测试按入口协议执行；Adapter 模式同时返回实际 `upstream_protocol`。测试与发现只从关联且启用的 Account 的 `credential_env` 取凭据，请求日志和持久化记录不包含 Authorization/API Key 或完整失败响应正文。失败记录仅保存稳定错误码、脱敏消息、HTTP 状态、耗时、操作者、Account、预设版本和 UTC 时间。成功 discovery 保存有大小上限的完整模型列表 snapshot；解析和数据库更新位于明确的失败边界，失败不会修改既有 SourceModel。

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
- `logical_models.request_timeout_ms` 与 `max_retries` 保存模型请求设置。超时覆盖整次请求与 SSE，null 继承全局总超时；重试数限制首次之后的实际尝试数，null 遍历所有可用线路。
- `source_model_capabilities` 以 `(source_id, upstream_model_id, protocol)` 唯一，协议模式为 `unknown/native/adapter/unsupported`。`unknown` 和 `unsupported` 都不是可路由能力；Adapter 仍只允许一次直接转换，并要求其来源协议已确认原生可用。能力行可由配置导入创建，也可由管理端逐协议 upsert（`PUT /admin/sources/:source_id/models/:model/capabilities/:protocol`）；feature 能力缺省时从 SourceModel metadata 推导，非法值保持 unknown 而不猜测。
- `model_bindings` 显式关联 LogicalModel、Source、Account、upstream model 和入口协议。Binding 初始为 `pending`；只有逻辑模型、Source、Account、SourceModel 和对应协议能力都已确认且可用时，数据库才允许转为 `confirmed`。

模型元数据字段第一版包括逻辑名、显示名、上下文窗口、最大输入/输出 Token、输入/输出模态、Tools、Thinking、Web Search、Structured Output、Streaming 和 Usage。每个字段来源只能是 `user/preset/upstream/unknown`，合并优先级固定为：

```text
user > preset > upstream > unknown
```

重复刷新同一 `(source_id, upstream_model_id)` 只更新原始发现快照、最近发现时间和可用状态，不创建重复记录。待确认记录会重新应用预设和上游元数据，但保留用户覆盖；已确认记录的元数据和匹配预设均保持不变。发现中消失的模型只标记 `unavailable`，不删除 LogicalModel、Binding 或 Route。

ProviderPreset 与发现确认阶段不改变 Route，也不把发现结果自动写入 `logical_models`、`model_bindings` 或 `routes`。新模型保持 `pending`；用户确认只将 SourceModel 变为 `confirmed`，协议能力需在确认 Binding 之前显式声明并确认，LogicalModel/Binding/Route 仍由独立控制面流程显式创建。运行时 snapshot 只消费 enabled 且 confirmed/available 的 LogicalModel、SourceModel、SourceModelCapability、ModelBinding、Source、Account 和 Route；`logical_models.enabled` 与 `model_bindings.enabled` 是独立运行期开关，不改变确认/可用状态历史。

每次成功刷新在单个 PostgreSQL 事务中按 Source 加锁，保存原始 snapshot、更新模型并生成按模型 ID 排序的 `added/changed/missing` diff。重复相同刷新得到空 diff；confirmed 元数据保持不变，pending 记录重算 upstream/preset 字段但保留 user 字段；missing 仅改为 `unavailable`。连接测试与 discovery run 分别保留审计历史，最近一次 discovery 可由 API 读取。

开发期 `GATEWAY_CONFIG_JSON` 只在控制面为空时做一次 `custom@1` 初始化，或在显式设置 `GATEWAY_CONFIG_IMPORT=true` 时事务化替换开发控制面。控制面非空的普通启动不会解析该 JSON，更不会覆盖 Source 快照或用户编辑；启动直接从数据库一致性事务构建 snapshot。

### 3.6 自定义渠道与跨 Source/Provider Fallback

跨 Source fallback 已接入 DB-first runtime snapshot。Route 只声明逻辑模型、入口协议、策略和是否允许有损转换；实际首选与 fallback 候选来自已确认、可用的 ModelBinding。每个候选 Binding 独立携带 Source、Account、上游模型、endpoint 和协议链，运行时不会再把首选 Source 的连接信息套到 fallback 请求上。

当前规则（#195 在既有加权策略之外新增 `ordered_fallback`）：

- 首选 Binding 固定优先；HTTP 408、429、5xx、传输错误，以及首选账号禁用或处于 PostgreSQL 冷却时，才进入 fallback 候选池。
- fallback 候选统一执行协议、enabled 和健康过滤；HTTP 响应错误与传输错误使用同一资格规则。
- `primary_then_weighted_fallback` 保持固定首选和一次按权重选择的 fallback（首选 1 次 + fallback 1 次）；无可用备用时仍沿用短 `Retry-After` 的一次同账号重试。
- `ordered_fallback` 按 Binding priority 降序对应的线路顺序逐一尝试，跳过协议不匹配、停用或冷却线路，每条最多一次；不按权重重排。`max_retries=0` 只允许一次实际尝试，null 遍历全部可用线路。所有尝试共享同一请求总超时；SSE 开始后由同一总时限终止流，不再重放到备用线路。
- 候选请求使用自己的 Source Base URL、协议 endpoint 和 Account 凭据；请求体顶层 `model` 按实际 Binding 的 `upstream_model_id` 重写。开发期初始化配置中的账号 `model_map` 也会在三类协议主路径和 fallback 路径生效。
- 跨 Source/Provider fallback 只允许原生协议链。Adapter 路由不能跨 Provider fallback；非法方向、多段转换和缺失 endpoint 会在配置或控制面事务中被拒绝。
- 响应已经开始向下游发送后不能再切换账号。
- 每次上游尝试写入独立 UsageAttempt；逻辑 UsageEvent 成功时归因最终成功 attempt，全部失败时归因最后一次实际 attempt，并保留固化的 `provider_preset_id` 作为 `provider_id`，以及实际 `source_id`、`account_id` 和 `upstream_model_id`。

账号健康状态以 PostgreSQL `accounts` 行为事实来源，并在 `account_health_events` 保存无正文的转换历史。被动失败和 ProviderPreset 连接探测使用指数退避；成功会清零连续失败和 cooldown。`health_updated_at` 超过 stale 阈值时状态转为 `stale`，旧 cooldown 不会永久屏蔽账号；人工启停会在同一控制面事务中重置健康状态。Provider 与 Source 已独立归因：同一 ProviderPreset 的多个 Source 使用相同 `provider_id`，但各自保留实际 `source_id`。具体 Source 与 Binding 初始化示例见 [`config.example.json`](../config.example.json)，凭据只允许通过服务端 Secret 配置。

## 4. 请求处理流程

```text
客户端请求
  ↓
入口协议识别
  ↓
PostgreSQL Virtual Key 鉴权（静态 GATEWAY_API_KEY 仅过渡兼容）
  ↓
读取 model
  ↓
读取原子发布的不可变 PostgreSQL snapshot
  ↓
匹配 enabled Route + confirmed/available Binding
  ↓
读取 SourceModelCapability 完整协议链
  ├─ native → 原生透传
  └─ adapter → 调用内置 Adapter
  ↓
主账号请求
  ├─ 成功 → 返回客户端
  └─ 408/429/5xx/网络错误 → fallback
  ↓
记录 usage_events 与 usage_event_attempts
```

当前 fallback 规则：

- 首选账号固定优先；
- HTTP 408、429、5xx 和网络错误允许 fallback；
- fallback Binding 必须启用、可用且通过账号健康过滤；原生链允许跨 Source/Provider；
- 响应已经开始流式输出后不能切换账号；
- HTTP 408、429、5xx 与网络错误使用同一加权候选选择；其他 4xx 不触发 fallback；
- 请求模型按候选 Binding/账号映射改写，attempt 记录实际 Source、账号和上游模型；
- 当前最多执行一次 fallback；账号失败会进入持久化指数冷却窗口，服务重启后从 PostgreSQL 恢复绝对 UTC 时间戳。

## 5. 当前配置模型

PostgreSQL 是运行时配置入口和事实来源，`DATABASE_URL` 为必填项。`GATEWAY_CONFIG_JSON` 仅是空控制面初始化/显式导入格式，完整示例见 [`config.example.json`](../config.example.json)。监听地址由 `GATEWAY_LISTEN_ADDR` 独立覆盖。

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

Kimi（#157 起三协议全部原生，单一 Route 即可覆盖）：

```json
{
  "id": "kimi-native",
  "model": "kimi-for-coding-highspeed",
  "provider_id": "kimi_code",
  "protocols": ["openai_chat_completions", "openai_responses", "anthropic_messages"],
  "primary_account_id": "kimi-main",
  "mode": "native"
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
- PostgreSQL Virtual Key 鉴权；`GATEWAY_API_KEY` 仅保留为过渡兼容。

### 路由

- Provider、Account、Route 配置模型；
- 精确模型匹配；
- `*` 前缀模型匹配；
- DB-first Route + Binding snapshot；
- 首选 Binding 固定优先；
- native 链跨 Source/Provider fallback；
- HTTP/传输错误统一加权候选选择；
- 三协议请求模型按实际 Binding/账号映射重写；
- Adapter/native 模式区分和单段转换校验；
- 逐 attempt 记录实际 Source、账号和上游模型。

### ProviderPreset 与模型发现

- DeepSeek、MiniMax、Kimi Code 内置版本化 ProviderPreset；
- 首批版本化 ModelPreset，字段来源严格为 `preset/unknown`；
- Source 创建时复制完整预设快照，最新预设仅用于差异预览；
- Source 独立 Base URL、endpoint、认证和协议能力快照用于后续连接测试；
- DeepSeek/MiniMax 模型列表发现；Kimi Code 显式报告不支持发现；
- discovery 原始 snapshot、脱敏失败、Account/操作者/耗时/预设版本审计；
- 稳定 added/changed/missing、待确认列表、用户编辑和事务化批量确认；
- 不自动创建 LogicalModel、Binding 或 Route，不改变 `/v1/models` 和运行时路由。

### Kimi Adapter（已于 #157 移除）

- Kimi Code 官方原生支持 OpenAI Responses 后，内置 `kimi-responses-adapter` workspace crate、进程内 dispatch 和相关回归已删除；
- Adapter 框架（注册表、校验、degraded 管线）保留，生产注册表为空；
- 存量数据由 migration 0023 迁移到 `kimi_code@4` native。

### PostgreSQL 基础

- `DATABASE_URL` 是 DB-first 运行时必填项；
- 启动时按 migration 初始化用量、控制面和模型目录表，并在 `REPEATABLE READ READ ONLY` 事务中构建运行时 snapshot；控制面写入在 `SERIALIZABLE` 事务中递增 `snapshot_revision`，内存发布拒绝旧 revision 覆盖新 revision；
- 请求结束后写入逻辑 UsageEvent 和逐次 UsageAttempt；
- `request_id` 与 `(request_id, attempt_no)` 分别保证逻辑请求和上游尝试幂等。

### Virtual Key 与统计 API

- `POST /admin/keys` 创建 Virtual Key；
- `GET /admin/keys` 查询 Key 列表；
- `GET /admin/keys/:id` 查询单个 Key 详情；
- `GET /admin/keys/:id/value` 由 Admin 显式查看可恢复 Key；
- `POST /admin/keys/:id/rotate` 轮换 Key，支持 `overlap_secs` 平滑过渡、`scopes` 权限和 `key_group` 分组；
- `POST /admin/keys/:id/revoke` 或 `DELETE /admin/keys/:id` 撤销 Key；
- Key 使用 SHA-256 哈希执行数据面鉴权；原始值另以 AES-256-GCM envelope 保存，列表和普通详情不返回，只有显式 value 接口解密；
- 支持 `allowed_models` 模型白名单；
- 成功鉴权后更新 `last_used_at`；
- `GET /admin/usage/summary` 返回逻辑请求、上游尝试、重试、成功/失败、延迟和 Token 汇总。
- `GET /admin/usage/timeseries?granularity=hour|day` 返回 UTC 小时/日时间桶。
- `GET /admin/usage/breakdown?breakdown=...` 支持 `logical_model`、`upstream_model`、`provider`、`source_id`、`client_source`、`account`、`protocol_in`、`protocol_upstream`、`virtual_key`、`status` 和 `usage_source`。
- `GET /admin/usage/events?limit=100&cursor=...` 使用 `(created_at DESC, request_id DESC)` 的确定性 keyset 游标，`limit` 范围为 `1..500`。
- `GET /admin/usage/export?format=csv|json` 按与 events 相同的筛选和排序导出全部匹配事件；不包含 prompt/response 正文。
- `GET /admin/usage/aggregate` 保留为 summary、timeseries 和单一 breakdown 的组合入口，响应与独立入口共享 `version: v1` 契约。

所有 Usage 查询共享组合筛选参数：`from`、`to`、`logical_model`、`upstream_model`、`provider`、`source_id`、`client_source`、`account`、`protocol_in`、`protocol_upstream`、`virtual_key`、`status`、`status_code` 和 `usage_source`。`from`/`to` 接受带 offset 的 RFC3339，服务端转换为 UTC，并以半开区间 `[from,to)` 解释；响应桶固定为 UTC，UI 只在展示层换算本地时区。`provider` 来自 Source 固化的 `provider_preset_id`，`source_id` 来自 DB-first RuntimeRoute/Binding 的最终实际 attempt；可选下游 `X-Client-Source` 只写入 `client_source`，缺省为 `unknown`，不参与路由或认证。

v1 响应 envelope 固定如下：summary 为 `{version, timezone, range, data}`；timeseries 额外返回 `granularity`，每个 `data` 元素包含 UTC `bucket`；breakdown 额外返回 `dimension`，每个元素使用可空 `key` 表示分组值；events 返回 `{data, page:{limit, has_more, next_cursor}}`。聚合指标统一包含 `logical_requests`、`upstream_attempts`、`retries`、`successes`、`failures`、`success_rate`、`average_latency_ms`、`p95_latency_ms` 和五类 Token；breakdown 另含 `logical_request_share`、`total_token_share`。客户端应把 `next_cursor` 视作不透明值并原样传回。

聚合中的 `logical_requests`、成功/失败、延迟和 Token 来自筛选后的 `usage_events`，因此每个逻辑请求和最终 Usage 只累计一次。`upstream_attempts` 来自这些逻辑请求关联的 `usage_event_attempts`；`retries` 来自逻辑事件的重试计数。Provider、Source、Client Source、Account、协议等筛选先选择逻辑请求，再统计其关联 attempt，避免把失败 fallback 的 Token 当成已确认 Usage。每个 attempt 独立保存 `provider_id` 与 `source_id`，使同 Provider 多 Source 和跨 Provider fallback 都可审计；逻辑事件成功时归因成功 attempt，全部失败时归因最终实际 attempt。`usage_source=missing` 的请求保留请求数但 Token 为零。

### Secret Resolver 与凭据加密

- AES-256-GCM 信封加密，格式 `gwenc:v1:key_version:nonce:ciphertext`；
- 多版本 keyring 支持渐进式轮换，旧版本仍可解密；
- 环境变量引用（`credential_env`）和加密密文（`credential_ciphertext`）二选一，同时配置则拒绝；
- `POST /admin/credentials/encrypt` 加密原始凭据；
- `POST /admin/accounts/:id/credentials/rotate` 以当前活跃密钥版本重加密账号凭据；
- 启动时从 `GATEWAY_CREDENTIAL_MASTER_KEY` 或 `GATEWAY_CREDENTIAL_MASTER_KEYS` 加载 keyring；
- `SecretLease` 使用 `Zeroizing<Vec<u8>>` 内存保护，Debug/Display 输出脱敏。

### Admin 审计日志

- 请求级 `AuditContext`，通过 `tokio::task_local` 在事务中共享；
- `audit_logs` 表记录 `operation_id`、`action`、`actor`、`status`、`result`、`diff`、`resource_type/id`；
- 成功写入在控制面事务内原子记录，失败写入在事务回滚后独立记录；
- diff 只保留字段名和类型，不保留标量值；敏感字段（credential、token、prompt 等 14 类）自动标记并排除；
- Admin 路由中间件已接入，自动从请求方法、路径和 payload 推导 `action`、`resource_type` 和 `resource_id`。

### 运行事件中心与恢复语义（#110）

事件中心采用方案 C（窄系统表 + 统一读侧），不复制既有事实，也不建立第二套状态机。`migrations/0024_system_events.sql` 新增 `system_events`，只保存原本仅存在于进程日志的生命周期、配置、数据库和安全事件；账号当前健康仍以 `accounts` 为准，请求/attempt、健康转换、Admin/运维审计和模型发现仍分别以原表为准：

| 事件类别 | 权威存储 | 统一时间线范围 |
| --- | --- | --- |
| 进程启动/优雅关闭、config reload、runtime snapshot 构建/切换 | `system_events` | 全部 |
| 数据库连接异常/恢复、master key 或账号凭据解析失败 | `system_events` | 全部；仅记录组件、主体和稳定错误码 |
| Admin 写入与后台运维生命周期 | `audit_logs` | 全部 |
| 账号健康转换 | `account_health_events` | 全部；当前状态仍读取 `accounts` |
| 模型发现运行 | `source_discovery_runs` | 全部 |
| 数据面请求 | `usage_events` | 只投影失败、fallback 或 degraded 请求；普通成功请求留在 Request Events |

`system_events` 固定字段为 UTC `occurred_at`、`category`、`event_type`、`level`、`subject_type/id`、`correlation_id`、短 `message` 和 metadata-only `details`。写入器与统一查询返回边界共用脱敏规则，限制长度、层级和字段数，递归移除 credential、token、Authorization、prompt/response、thinking/signature 等敏感内容。既有 audit actor、discovery requested_by 属于非必要且任意调用方可控的文本，在统一投影中无条件替换为 `[REDACTED]`，不能依赖供应商 Key 前缀识别。通用 Admin 请求可自带的 `X-Request-ID` 只保留为服务端筛选键：audit 的主体/关联输出和复用该值的配置类 system event 关联输出固定脱敏，避免普通事件响应反射外部关联值；专用运维任务的 `operation_id` 仍是可返回、可轮询的领域资源标识。事件落库失败只告警，不能把本来成功的网关操作改成失败。

连接事件使用共享 SQLx 分类：I/O、TLS、连接池不可用/获取超时、SQLSTATE `08` 和 PostgreSQL 停机/尚未可连接状态会打开 incident；缺表、权限、约束、解析等查询错误保持原业务错误，不伪装成连接异常。用量读写（写入含 SSE 结算）、Virtual Key 鉴权、健康状态与探测 metadata 读写、普通 Admin 控制面读取、控制面事务/snapshot、Admin 审计和事件查询接入该记录器。运行期数据库对象的克隆共享按组件区分的 incident 集合：每个组件只保留首个未恢复 incident，并由该组件后续成功按原发生时间补写失败事件、写入同一 `correlation_id` 的恢复事件并独立关闭；其他组件的失败不会被吞掉，成功或仅发布内存 snapshot 也不会错误关闭它。失败路径只在短内存临界区登记，不同步重试已经不可用的连接池；恢复路径在不持有 incident 集合锁时补写。此记录不依赖周期探测开启。

初始 PostgreSQL 连接/迁移尚未成功时无可用事件仓储；进程终止前仍未恢复的 incident 也无法持久化。这些失败仍由启动错误/日志报告，内存补写不提供跨重启可靠性。

`GET /admin/events` 在上述五张历史表上构建只读 `UNION ALL` 投影，支持 `from`、互斥的 `since`、`to`、`category`、`level`、`event_type`、`subject_type/id`、`correlation_id`（`operation_id` 为查询别名）、`source`、`limit` 和不透明 keyset `cursor`。`from` 使用闭边界、`since` 使用开边界、`to` 使用开边界；结果按 `(occurred_at DESC,event_id DESC)` 稳定分页。该 API 与“运行事件”页面用于关注项回看和跨表关联，不能替代各事实表的专用详情接口。

恢复/继续时机维持现有成功驱动模型，不引入通知总线、进程内 broadcast 或 PostgreSQL signal：

| 场景 | 谁继续 | 恢复/继续条件 | 观察方式 |
| --- | --- | --- | --- |
| 账号健康恢复 | 路由候选与健康状态机 | cooldown 到期只变为可尝试的 `unhealthy`；下一次真实请求或默认 60 秒周期探测成功后才写回 `healthy` | `accounts` 当前状态 + `account_health_events` |
| fallback 回主 | 下一次逻辑请求 | 主账号只有在上述成功将其恢复为 `healthy` 后才重新固定优先；既有 fallback 请求不会重放 | `usage_events.fallback_reason` + 健康事实 |
| 后台任务完成/重试 | Admin 客户端或调度器 | 继续轮询任务 GET；需要关注跨表事实时以 `operation_id` + 上次时间作为 `/admin/events?since=...` 增量查询，取消/retry 仍调用任务专用端点 | 任务资源 + `audit_logs` 统一投影 |

Prometheus 保持实时、可聚合、低基数职责：请求量、耗时、Token、活跃流、冷却次数和 snapshot revision 不逐条复制进 `system_events`。落库事件负责逐条回看、主体与 `correlation_id` 关联和脱敏 metadata；高吞吐普通成功请求只进入 Usage 事实与指标，不进入运行事件时间线。

### Prometheus 与 OpenTelemetry 可观测性

- `GET /metrics` 返回 Prometheus 文本格式指标；
- 请求级：`gateway_requests_total`（protocol/model/status/mode）、`gateway_request_duration_seconds`；
- 上游级：`gateway_upstream_attempts_total`（protocol/source/account/status/fallback）；
- Token 级：`gateway_tokens_total`（model/direction）；
- 流式：`gateway_time_to_first_token_seconds`、`gateway_active_streams`；
- 运维：`gateway_health_cooldowns_total`、`gateway_snapshot_revision`；
- 非流式和流式路径均已接入 `record_proxy_request`；5 秒 upkeep 周期。
- OpenTelemetry OTLP/gRPC tracing 已由 PR #80 实现；设置 `OTEL_EXPORTER_OTLP_ENDPOINT` 启用，`OTEL_SERVICE_NAME` 默认 `my-ai-gateway`。未配置时仅本地日志；初始化失败会告警并回退本地日志。生产 collector 接收情况需另行验证。

### 部署

- [Dockerfile](../Dockerfile)；
- [docker-compose.yml](../docker-compose.yml)；
- [.env.compose.example](../.env.compose.example)；
- [Docker Compose 部署说明](deployment.md)；
- [Kubernetes / K3s 部署参考](kubernetes.md)；
- 多阶段前端/Rust 构建和精简运行时镜像，运行时使用非 root 用户并提供 `/healthz` 容器健康检查；
- PostgreSQL 16；
- Gateway + PostgreSQL 单机部署结构，PostgreSQL 使用命名卷持久化，默认不向宿主机发布数据库端口。

## 7. 分领域现状与剩余验收

### 7.1 Virtual Key 正式系统

已完成数据库-backed Key 创建、列表、查询、受控查看、撤销、轮换和模型白名单鉴权。Key 轮换支持 `overlap_secs` 平滑窗口、scopes 权限更新和 `key_group` 分组；migration 0015 新增生命周期字段，0016 增加绑定 Key prefix 的 AES-GCM recovery envelope。Admin Session 使用独立的 `GATEWAY_ADMIN_KEY` fail closed 保护；旧 hash-only Key 保持可鉴权，但必须轮换后才可查看。

### 7.2 PostgreSQL 持久化剩余项

当前已经创建 `usage_events`、`usage_event_attempts`、`virtual_keys`、`providers`、`accounts`、`routes`，Provider/Model preset、Source、SourceModel、LogicalModel、ModelBinding、SourceModelCapability 模型目录表，以及 `source_connection_tests`、`source_discovery_runs` 记录表。内置预设以不可变 `(id, version)` 启动注册。运行时从 `sources`、`accounts`、`logical_models`、`model_bindings`、`source_models`、`source_model_capabilities` 和 `routes` 构建完整 `protocol_in → protocol_upstream → endpoint/Adapter` 链，旧 `providers` 行不再是运行时事实来源。`request_id` 表示一次北向逻辑请求并保持唯一；重试尝试写入 `usage_event_attempts(request_id, attempt_no)`，同一尝试幂等。`usage_events.logical_model` 保存客户端模型，`upstream_model_id` 保存实际 Binding 的上游模型，`provider_id` 保存 Source 固化的 ProviderPreset ID，`source_id` 保存最终实际 Source，`client_source` 独立保存客户端自报来源；attempt 逐次保存自己的 Provider 与 Source。历史旧 `source` 值迁入 `client_source`，历史 `source_id` 保持 `NULL`；0010 migration 只把 `source_id IS NOT NULL AND provider_id = source_id` 的 DB-first 错误归因回填为 ProviderPreset ID，Source 已删除且无法可靠映射时显式写为 `unknown`。Usage 历史不对控制面 `sources` 设置外键，因此删除 Source 不会删除历史归因。Virtual Key 鉴权成功时写入 `virtual_key_id`，静态入口 Key 保持为空。时间统一按 PostgreSQL `TIMESTAMPTZ` 以 UTC 存储，展示层负责本地时区转换。

控制面写入采用 `SERIALIZABLE` 事务：先写候选变更，再校验引用、endpoint、Adapter 注册表与方向、单段转换、能力链和 Binding 可路由性，随后在同一事务读取并构建下一版不可变 snapshot；任一步失败都回滚。提交成功后一次写锁替换整个 snapshot，并发请求只会持有旧版或新版的完整 `Arc`。手工 reload 使用一致性只读事务；失败不替换当前有效 snapshot。

Admin 资源为 `/admin/sources`、`/admin/accounts`、`/admin/logical-models`、`/admin/model-bindings` 和 `/admin/routes`，支持集合 `GET/POST`、单资源 `GET/PUT/DELETE` 与 `PUT /{id}/enabled`。`GET /admin/capabilities` 读取与 proxy 相同的不可变 runtime snapshot，按 Route、Source、Account、logical/upstream model 输出三协议完整矩阵、primary/fallback Binding、直接转换链、degraded 状态和结构化不可路由错误；它不会回退到初始化配置。错误固定为 `{error:{code,message}}`；Account 与能力矩阵响应均不返回 `credential_ciphertext`、`credential_env` 或明文凭据。

#195 的常规模型配置通过 `GET/PUT /admin/logical-models/{id}/routing` 聚合接口承载模型名、启停、有序 Source/Account/upstream model 线路以及超时/重试。`POST /admin/model-routings` 原子创建并由服务端生成模型 ID，名称重复返回冲突；PUT 只更新已有模型。两者在同一个既有控制面事务中根据已确认、可用的模型协议能力生成各协议 Binding 和 Route，策略固定为 `ordered_fallback`；编辑保留已有模型 metadata、field_sources 与 ID。完整校验、snapshot 构建和事务提交均成功后一次发布，失败整体回滚。界面无需串行调用模型/Binding/Route 写接口。普通界面仅为模型列表与编辑抽屉，协议能力只读，底层 CRUD 能力保留；对旧加权配置的修改通过显式保存转为顺序线路。模型列表读取请求设置，在重试上限限制回退时明确展示最多尝试的可用线路数。

控制面写入契约以 Source/Binding 为中心：Source 创建时复制 `provider_preset_id@version` 快照，后续 `PUT` 不允许更换该引用；Account 直接引用 `source_id`，凭据只能提交 `credential_env` 或 `credential_ciphertext`；LogicalModel 的 `status` 与 `enabled` 分离；ModelBinding 明确携带 `logical_model_id/source_id/account_id/upstream_model_id/protocol/status/enabled/priority`；Route 只声明 `logical_model_id/protocols/strategy/allow_lossy_conversion/enabled`，上游 Source、账号、模型、模式和 Adapter 全部由 Binding + SourceModelCapability 解析，Route 不再复制这些字段。ProviderPreset 与 SourceModel 的发现/确认 API 由 #13 负责，不在本控制面重复实现。

PostgreSQL 回归测试只连接显式的 `TEST_DATABASE_URL`，不会复用运行时 `DATABASE_URL`。完整控制面测试为 ignored test，并在实际执行时创建/清理独立 schema；验收必须显式运行，不能把缺少数据库导致的跳过作为通过。

ProviderPreset/模型发现回归使用真实 PostgreSQL 与 mock 上游，覆盖 DeepSeek、MiniMax、Kimi Code 的成功、失败、空列表、重复刷新、模型消失、confirmed/user 覆盖保留、批量确认、版本差异和日志脱敏。非流式 JSON 响应与流式 SSE 都必须把上游 usage 映射到统一 `UsageReport`；缺失 usage 才按既有 `estimated/missing` 规则处理。

账号健康持久化、主动探测和无正文转换历史已由 #52 完成；统一 Admin 写操作审计日志（#48）已接入。#53 已补齐运维操作自身的审计记录，不把普通应用日志当作审计事实；#110 只为无既有事实表的系统事件补窄表，并在读侧聚合这些事实。

### 7.2.1 数据保留、清理、备份与恢复（#53）

`migrations/0011_retention_backup.sql` 新增 `retention_policies`、`retention_cleanup_runs`、`audit_logs`、`backup_runs`、`gateway_schema_migrations` 和 `gateway_schema_metadata`；`migrations/0012_health_persistence.sql` 追加健康状态字段与 `account_health_events`；`migrations/0024_system_events.sql` 增加窄系统事件表和对应清理计数。五类策略（logical UsageEvent、UsageAttempt、连接测试/健康/运维 audit、discovery run、system event）分别按 UTC `retention_days` 管理。`POST /admin/retention/cleanup` 在运行开始时固定策略和 cut-off，每个批次独立提交并记录 scanned/deleted/progress；同一 `operation_id` 可重复提交、取消和 retry。逻辑事件只有在不会级联删除仍在保留期内的 attempt 时才删除。

控制面可通过 `GET /admin/control-plane/export` 导出脱敏 JSON，包含恢复路由所需的 Source/Account/模型/Binding/Route、schema/migration 版本和 runtime fingerprint，不包含 usage 正文、Authorization、API Key、Virtual Key hash/recovery ciphertext 或账号凭据 ciphertext。`POST /admin/control-plane/import` 在显式 `replace=true` 时按 FK 顺序恢复到新库，重置序列并重新构建 snapshot；fingerprint 不一致时标记恢复失败。完整 pg_dump、Compose 和本地 CLI 步骤见 [`operations.md`](operations.md)。

### 7.3 Token 统计

已完成 OpenAI Chat/Responses、Anthropic Messages 的非流式 JSON usage 提取、SSE 末事件解析、reasoning/cached token 映射和异步落库。成功响应缺少已确认 usage 时可以使用 `tiktoken-rs` 的 `cl100k_base` 估算并标记为 `estimated`；失败 JSON/SSE 不再估算，固定记录 `missing` 和 0 Token。

UsageEvent 已记录实际 `upstream_model_id`、`route_id`、`streamed`、脱敏 `error_summary`、最终 `source_id`、独立 `client_source` 和 `ttft_ms`。流式 TTFT 从逻辑请求开始计到首个非空 Provider body chunk（网关心跳和纯 SSE comment 不计入），不预取、不缓冲，也不改变 SSE 顺序或背压；空流和无法观察首块的失败保持 `NULL`。每个 fallback attempt 另存实际 Source、账号、上游模型、状态和耗时。migration 0019 新增 `usage_events.fallback_reason`：主账号在前置不可用（`account_disabled` / `account_cooling_down` / `account_unhealthy` / `account_unavailable`）或主路径尝试失败（retryable HTTP 状态 `upstream_http_<status>` 如 429、`upstream_transport_error`）时，事件记录该白名单原因码，使 Request Events 能解释“为什么响应模型不是请求的逻辑模型”；未发生 fallback 保持 NULL。

Provider 与 Source 已使用独立运行时身份：Provider 按 Source 固化的 ProviderPreset 聚合，Source 保留每次实际 Binding/attempt 的具体来源；组合筛选不会重复逻辑请求或 Token。

### 7.4 控制台

控制台提供总览、用量分析、请求事件、运行事件、来源管理、模型与路由、能力矩阵、系统设置八个入口；“模型更新审核”作为来源生命周期内的子页面（`#sources/<id>/review`）接入，不再是独立一级导航。前三项继续由 `/admin/usage/*` 提供用量统计与请求明细；“运行事件”通过 `/admin/events` 展示跨子系统关注项；其他管理页面通过对应 `/admin/*` 资源完成接入、模型确认、路由和 Virtual Key 管理。

`GatewayConsoleShell` 统一导航与 Admin Key 连接。用量视图与管理视图分别由 `GatewayUsagePage`、`GatewayManagementPage` 接入，公共组件位于 `web/src/components/ui`。视觉与交互规范见 [design.md](../design.md)，目录与开发命令见 [前端 README](../web/README.md)，分层、共享 Admin 传输与时间窗口规则见 [前端架构](frontend-architecture.md)。

### 7.5 账号健康和生产化

账号健康状态由 PostgreSQL `accounts` 行提供事实来源，`account_health_events` 保存不含请求/响应正文的审计历史。状态机明确区分：

- `unknown`：没有观测或人工重新启用；
- `healthy`：路由请求或 ProviderPreset 连接探测成功；
- `cooling_down`：失败窗口达到阈值后触发指数退避；
- `unhealthy`：窗口内尚未达到阈值，或 cooldown 到期但尚未成功恢复；
- `stale`：`health_updated_at` 达到阈值，旧 cooldown 只作提示而不会永久屏蔽账号；
- `disabled`：Account 或 Source 被人工停用。

所有转换以 UTC 观测时间写入，行级锁保证并发失败计数不丢失。默认在 60 秒内累计 3 次可重试失败才开启冷却，第三次采用 base cooldown；活跃 cooldown 内完成的并发请求不会继续放大退避。成功会清零失败窗口、连续失败和 cooldown，新的 `HealthRegistry` 直接读取数据库恢复重启前状态。固定首选保持优先，只有冷却、不可用或人工停用才进入 fallback；无 fallback 时只对带短 `Retry-After` 的 429 做一次同账号重试。`POST /admin/accounts/:account_id/probe`、`POST /admin/health/probe` 和批量接口复用 ProviderPreset 最小连接测试，不调用 discovery；探测沿用 Source URL allowlist、DNS、重定向和凭据策略，并在 cooldown 内按正常周期作为半开恢复探测。周期任务默认每 60 秒运行，可由环境变量关闭或调整。

生产化剩余范围均有独立 Issue：

- 健康状态持久化与主动探测（#52）已完成；
- SSE 心跳、取消和流式超时契约（#54）已完成：三协议原生透传共享可配置心跳、连接/首事件/空闲/总时限和取消清理；
- Prometheus 指标采集与 `/metrics` 端点（#50，PR #77）已完成：`gateway_requests_total`、`gateway_upstream_attempts_total`、`gateway_tokens_total`、`gateway_request_duration_seconds`、`gateway_time_to_first_token_seconds`、`gateway_health_cooldowns_total`、`gateway_snapshot_revision`、`gateway_active_streams`；OpenTelemetry OTLP/gRPC tracing 已由 PR #80 完成；
- Secret Resolver 与凭据信封加密（#47，PR #73）已完成：AES-256-GCM 信封加密、多版本 keyring、运行时凭据路径和 Admin 加密/轮换端点已集成；
- Admin 写操作审计日志（#48，PR #74）已完成：请求级 AuditContext、diff 脱敏、事务内/独立审计记录和 Admin 路由中间件已接入；
- 数据保留、清理、备份和恢复（#53，第一版已完成；后续仅按运行反馈加固）。

Provider URL allowlist、解析后 IP 校验、重定向限制和 SSRF 防护（#46）已经完成。Admin API 只接受独立的 `GATEWAY_ADMIN_KEY`，未配置时请求级 fail closed 返回 `401`，不会回退到数据面 Key。

### 7.5.1 SSE 流式契约（#54）

SSE 心跳和超时是进程级运行参数，不属于 PostgreSQL Source/Binding。网关使用
`GATEWAY_SSE_HEARTBEAT_INTERVAL_MS`、`GATEWAY_SSE_CONNECTION_TIMEOUT_MS`、
`GATEWAY_SSE_FIRST_EVENT_TIMEOUT_MS`、`GATEWAY_SSE_IDLE_TIMEOUT_MS` 和
`GATEWAY_SSE_TOTAL_TIMEOUT_MS`。默认值分别为 `15000`、`120000`、`30000`、`60000` 和 `300000`，`0`
禁用单项限制。也接受带单位的环境值（如 `2s`、`500ms`）。

连接时限覆盖流式和非流式请求等待上游响应头的阶段，默认 120 秒；总时限也用于非流式响应体读取。
首事件时限从响应头开始，空闲时限在每个完整 Provider
SSE 事件后重置，总时限从逻辑请求开始计算。心跳固定为 `: gateway-heartbeat` SSE
comment，单独作为下游 Body chunk 发送，不进入 Provider 事件、序列号、Usage 捕获或 TTFT。
已发出响应头后不能 fallback：正常 EOF 保留原始顺序并结束；Chat 仅在所有已出现的 choice 都报告非空 `finish_reason` 时补缺失的 `[DONE]`，缺少完成证据则报告上游错误，后续 usage chunk 不提前截断。空流发送
`gateway_empty_stream`，上游读取错误发送 `gateway_upstream_error`，首事件/空闲/总时限
分别发送对应的 `gateway_*_timeout` 错误帧后关闭。下游 Body 被丢弃时立即 drop Reqwest
上游流，Usage 以 `499` 和 `client disconnected` 记录；这些错误摘要只含稳定脱敏文本，
不保存 prompt/response 正文。每次流结束还通过 tracing 输出低基数的终止原因、TTFT 和
转发字节数，并已接入 Prometheus/OpenTelemetry（#50）；不使用 request id、模型全文
或凭据作为指标标签。

SSE usage 解析失败日志只记录 request_id、成功状态、字节长度、行数、JSON 解析失败计数、无 usage 计数及最终 usage_source；不输出正文预览、Base64/hex 编码片段或正文指纹（2026-09-07 收尾修复）。

### 7.6 测试

- Kimi 内置 Adapter 及其转换测试已随 #157 移除，当前测试以三协议原生契约为主；保留通用 Adapter 框架校验与 degraded 语义回归；
- 原生 thinking/web search/tool calling 与 usage 由 JSON/SSE Contract 覆盖；
- 已增加 OpenAI/Anthropic usage JSON 和 SSE 提取单测；
- 已覆盖主账号和 fallback 的三协议模型重写、408/429/5xx/传输错误、首选账号不可用、全部失败和流式 TTFT；
- 已覆盖失败请求 `missing/0`、真实 upstream model、逻辑事件/attempt 归因及跨 Source fallback；
- 已增加隔离 PostgreSQL schema 的控制面集成测试，覆盖 DB-first 一次性导入、全资源 CRUD/启停、事务回滚、native/adapter Binding 解析、并发 snapshot 切换、刷新失败保留旧 snapshot、凭据脱敏和 `/v1/models` 健康过滤。
- 已增加隔离 PostgreSQL schema 的 #53 运维回归，覆盖 UTC 保留 cut-off、dry-run、分批续跑、attempt/logical event 引用保护、audit/discovery 清理、脱敏控制面导出和新库 snapshot fingerprint 校验；HTTP 运维入口同步覆盖策略、版本、导出和恢复契约。
- 已增加健康状态单元、HTTP/Admin 和 PostgreSQL 回归，覆盖指数退避、并发计数、成功恢复、stale/过期放行、重启恢复、Source/Account 人工启停、ProviderPreset 探测复用、发现失败隔离和凭据/正文不泄露。

### 7.7 真实联调基线（2026-08-31）

历史真实上游联调已验证：

- 三协议原生透传全部可用，包括 MiniMax `/v1/responses`、DeepSeek `/v1/responses` 与 `/anthropic/v1/messages`（两家 Anthropic 兼容端点均为 `{base_url}/anthropic/v1/messages`，`config.example.json` 已修正）；
- Kimi Responses → Anthropic adapter 非流式/流式可用，SSE 事件序列完整；
- usage 落库、`/admin/usage/*` 聚合和 Admin 控制台 Overview/Analysis 展示与数据库一致；`protocol_in → protocol_upstream → mode` 链路记录正确；
- 上游 401 原样透传并记录 `success=false`，且不触发 fallback，符合仅对 408、429、5xx 和传输错误重试的语义。

上述真实环境基线之后，主线已通过 mock 上游和真实 PostgreSQL 回归补齐实际 `upstream_model_id`、流式 TTFT、首选账号不可用时的 early fallback、HTTP/传输错误统一加权选择、全部失败归因和跨 Source attempt 审计。三家真实 Provider 尚未在这些修复后完整重跑，因此这是待复验项，不再作为“功能未实现”记录。

Usage 的 `provider_id` 与 `source_id` 已在 DB-first snapshot、主路径、early fallback、HTTP/传输 fallback、全失败与流式路径中分离，并有真实 PostgreSQL 回归。用量页面请求 `/admin/usage/*`；`web/dist` 是构建产物，必须由当前源码生成，不能复用历史 Keeper 构建。

## 8. 验收标准

### 原生 Provider

- MiniMax 三种协议均可透传；
- DeepSeek 三种协议均可透传；
- Web Search、Tools、Thinking 字段不被网关修改；
- 上游 SSE 可被客户端持续读取；
- 429/5xx 可触发 fallback。

### Fallback 渠道

- 429/5xx/传输错误时可 fallback 到不同 Provider 的账号，响应来自候选渠道；
- 转发 body 的 `model` 按候选 Binding/账号映射重写，attempt 记录实际 Source、账号与 upstream_model_id；
- adapter 路由配置跨 Provider fallback 时被校验拒绝；
- 首选账号冷却/禁用时自动进入 fallback 候选；
- 上述链路有 mock 级端到端测试（非流式 + 流式）。

### Kimi

- Chat Completions 原生透传；
- Anthropic Messages 原生透传；
- Responses 原生透传至 `/v1/responses`；
- thinking/signature 保留；
- function call 保留；
- web search 保留；
- 非流式和流式均可工作。

### 统计

- 每个请求有唯一 request_id；
- 成功和失败请求都可查询；
- 能按协议、模型、Provider、Source、账号和 Key 独立聚合并组合筛选；
- 上游 usage 优先；
- 缺失 usage 时明确标记估算或缺失。

## 9. 后续工作

开放任务、依赖与发布验收统一维护在 [todo.md](todo.md)。
