# MiniMax Code Token Cache 深度调研报告

日期：2026-09-03  
范围：本地 Kimi Code 0.39.0、`ai-gateway.pvelab.top` 生产网关、MiniMax-M3 与 DeepSeek V4 Flash

## 结论

MiniMax 的 Token Cache 实际已经生效。当前“没有缓存”的主要问题发生在网关统计层，而不是 MiniMax 推理层。

在 2026-09-02 13:23:56Z–14:27:47Z 的对齐窗口内：

| 观测位置 | 记录数 | 缓存命中记录 | 缓存读取 Token | 非缓存输入 Token | 缓存读取占输入 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Kimi Code 本地 wire | 529 | 526 | 88,518,137 | 1,012,787 | 98.87% |
| 网关 Usage Events 抽样 | 500 | 4 | 512 | 无法可靠计算 | 无法可靠计算 |

网关的 500 条记录中有 494 条 `usage_source=estimated`。估算逻辑不可能知道上游缓存读取量，因此会把 `cached_tokens` 记为 0。换言之，管理端看到的 0 主要表示“网关没有解析到上游 Usage”，不能解释为“MiniMax 没有命中缓存”。

此外，原路由配置允许 MiniMax 失败后调用 DeepSeek V4 Flash，导致逻辑模型仍显示 MiniMax-M3，但实际执行模型和缓存系统已经变成 DeepSeek。这个归因污染已处理。

## 证据链

### 1. Kimi Code 确实发出了可缓存请求

Kimi Code 的 Anthropic Provider 会在系统提示、最后一个可缓存消息块和最后一个工具定义上附加 `cache_control: {type: "ephemeral"}`。本地 wire 日志同时显示固定的 system/tools hash，并在连续轮次中报告大量 `inputCacheRead`。

截至调查快照，本机共有 1,883 条 `my-gateway/MiniMax-M3` Usage 记录，其中 1,863 条缓存读取大于 0，命中记录占 98.94%；累计缓存读取 245,983,211 Token，非缓存输入 4,085,125 Token。

MiniMax 官方说明自动缓存会复用至少 512 Token 的重复前缀，前缀顺序是 tools → system → messages；Anthropic 兼容接口也支持显式的 ephemeral cache breakpoint。参见 [MiniMax Prompt Caching](https://platform.minimax.io/docs/api-reference/text-prompt-caching) 与 [Anthropic-compatible Cache](https://platform.minimax.io/docs/api-reference/anthropic-api-compatible-cache)。

### 2. 网关流式 Usage 大量退化为估算

生产数据的 500 条 MiniMax 样本中，499 条是流式请求、499 条走 Anthropic Messages；Usage 来源分布为：

- `estimated`: 494
- `parsed`: 5
- `upstream`: 1

代码在无法从 SSE 中提取 Usage 时，会用请求与响应字节做 tokenizer 估算；估算结果天然没有缓存字段，见 [`src/proxy/usage.rs`](../src/proxy/usage.rs)。同一批响应在 Kimi Code 客户端能够形成 `inputCacheRead`，说明问题已收敛到网关的 SSE 捕获/解析/完成时机，而不是上游没有返回缓存数据。

目前还缺少生产原始 SSE 样本，因为系统按设计不保存完整正文。下一步应从已经存在的 `SSE usage extraction failed` 安全预览日志中提取失败事件形状，制作脱敏 fixture，并补齐回归测试。

### 3. 网关把 Cache Read 和 Cache Creation 合并了

当前 `cached_tokens` 的解析会把以下字段相加：

- `cached_tokens`
- `cache_read_input_tokens`
- `cache_creation_input_tokens`

因此即使值大于 0，也不能判断它是缓存读取还是首次写入。管理端将这个合并值理解为缓存命中，会形成第二层误判。正确的数据模型至少需要拆成 `cache_read_tokens` 与 `cache_creation_tokens`，并保留 `usage_source`。

### 4. 跨模型 fallback 污染了 MiniMax 归因

历史请求 `84245e5e-044b-49d2-b3c4-beebac79dc88` 的尝试链为：

1. MiniMax-M3 请求 10,007 ms 后返回 504；
2. 网关 fallback 到 `deepseek-v4-flash`，950 ms 后成功；
3. 最终事件的逻辑模型仍是 MiniMax-M3，但 `source_id=deepseek`、`upstream_model_id=deepseek-v4-flash`。

这种请求不能用于判断 MiniMax 缓存。调查时发现 24 条 MiniMax → DeepSeek 和 9 条 DeepSeek → MiniMax 的已启用 Binding。

## 已执行修复

### Kimi Code 模型元数据

本地配置 [`~/.kimi-code/config.toml`](/Users/zhaojianyun/.kimi-code/config.toml) 已更新并通过 `kimi doctor`：

| 模型 | 上下文 | 最大输出 | Thinking | 其他能力 |
| --- | ---: | ---: | --- | --- |
| MiniMax-M3 | 1,000,000 | 500,000 | adaptive on/off | tools、image、video |
| DeepSeek V4 Flash | 1,000,000 | 384,000 | low/high/max，默认 high | tools |

MiniMax 官方确认 M3 是 1M 上下文、原生图像/视频输入并支持 adaptive thinking；工具会话必须原样回传完整 thinking/text/tool blocks。参见 [MiniMax API Overview](https://platform.minimax.io/docs/api-reference/api-overview)、[Anthropic SDK Compatibility](https://platform.minimax.io/docs/api-reference/text-anthropic-api) 与 [M3 发布说明](https://www.minimax.io/blog/minimax-m3)。

DeepSeek 官方规格为 1M 上下文、384K 最大输出，支持 `low/high/max` 思考档位，默认 `high`。参见 [DeepSeek Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/) 与 [Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)。

新配置已通过一次真实 Kimi Code 调用验证：`deepseek-v4-flash` 返回成功，wire 请求使用 `maxTokens=384000`、`thinkingEffort=high`、`thinkingKeep=all`。

### 网关 fallback

33 条跨家族 Binding 已从控制面彻底删除，不再只是 disabled：

- MiniMax → DeepSeek 的 24 条记录已删除；
- DeepSeek → MiniMax 的 9 条记录已删除；
- 复查 MiniMax ↔ DeepSeek 跨家族 Binding 的总记录数为 0，包括 disabled 残留也不存在；
- `b.ai` 的 Binding 52 保留，因为其逻辑模型和上游模型都是 `deepseek-v4-flash`，属于同模型多来源，不是跨家族 fallback。

运行时路由已验证：MiniMax-M3 Anthropic 路由只解析到 MiniMax，DeepSeek V4 Flash Anthropic 路由只解析到 DeepSeek。

## 当前独立故障

完成 fallback 收紧后，新的 MiniMax Kimi Code 测试返回 503，这是符合预期的显式失败，而不是再静默切到 DeepSeek。`minimax-main` 当时处于 cooling down，已有 13 次连续上游失败；管理探针还返回 `invalid_provider_preset`。

这两个问题与 Cache 是否命中彼此独立，但会阻止新的 MiniMax 端到端复测：

- 数据面：MiniMax 上游请求持续超时/失败；
- 控制面：Source 中保存的 Provider Preset snapshot 无法通过当前探针校验。

## 建议的工程修复顺序

1. **P0：修复 Anthropic 流式 Usage 解析。** 从安全日志提取真实 MiniMax `message_start/message_delta` 事件形状，补充 fixture，要求缓存读取、缓存创建、输入、输出在分块合并后完整保留，禁止成功请求默默退化为 estimated。
2. **P0：拆分缓存字段。** 数据库、领域类型、Admin API 与前端分别展示 `cache_read_tokens`、`cache_creation_tokens`；历史 `cached_tokens` 不应继续被解释为纯命中。
3. **P1：给 fallback 增加显式家族约束。** 当前已修复生产配置，但代码仍允许管理员未来重新创建跨家族 Binding。建议给 Binding/LogicalModel 增加显式 `fallback_group` 或 canonical family，写入时校验；不要用模型名字符串猜测家族。
4. **P1：修复 MiniMax Source Preset 与账号健康。** 先解决 `invalid_provider_preset`，再处理上游 10 秒超时和 cooldown，恢复后执行两轮相同前缀的受控 MiniMax 请求。
5. **P1：识别 Kimi Code 客户端来源。** 500 条样本中 493 条 `client_source=unknown`，建议从稳定的 User-Agent/显式 Header 记录 `kimi-code`，方便后续分客户端比较缓存率。

## 验收标准

- 连续两轮相同静态前缀请求中，第二轮 `cache_read_tokens > 0`；
- Kimi Code wire、网关 UsageEvent、上游账单三处缓存读取量处于同一数量级；
- 成功的 MiniMax Anthropic 流式请求不再大面积出现 `usage_source=estimated`；
- Cache Read 与 Cache Creation 可独立查询和展示；
- MiniMax 上游失败时只尝试 MiniMax 同类账号/Source，没有可用同类 fallback 就返回结构化错误；
- DeepSeek 与 MiniMax 的 Usage、价格和缓存统计不再交叉归因。

## 安全说明

本报告未写入 Admin Key、数据面 Key 或 Provider Key。调查中提供的 Admin Key 已出现在对话记录中，建议完成本次排查后轮换。
