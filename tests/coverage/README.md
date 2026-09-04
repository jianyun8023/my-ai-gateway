# 测试覆盖基线 — `tests/coverage/`

本目录定义 AI Gateway 的协议能力矩阵、统一测试结果模型和 case ID 命名规则。
它是后续所有 Contract / Conformance / Live / Load 测试的元数据基线。

> **本阶段只建立测试事实模型，不实现具体协议测试。**

---

## 文件说明

| 文件 | 用途 |
|------|------|
| `capability-matrix.json` | Protocol × Provider × Feature × Mode 能力矩阵 |
| `test-result.schema.json` | 统一测试结果报告 JSON Schema |
| `README.md` | 本文件 |

## 能力矩阵 (`capability-matrix.json`)

### 维度

矩阵按以下四个维度交叉描述：

1. **Protocol**（三类北向协议）：
   - `openai_chat_completions`
   - `openai_responses`
   - `anthropic_messages`

2. **Provider**（上游提供商 + 网关通用行为）：
   - `deepseek`
   - `minimax`
   - `kimi_code`
   - `gateway_common`（跨 Provider 的网关自身行为）

3. **Feature**（被测能力）：
   - `text` / `multi_turn` / `streaming` / `usage`
   - `tools` / `tool_choice` / `parallel_tools` / `tool_result`
   - `structured_output` / `reasoning` / `thinking`
   - `error_envelope` / `cancel` / `timeout`
   - `web_search` / `citations`

4. **Mode**（协议路径模式）：
   - `native`：Provider 原生协议直通
   - `convert`：Gateway/Adapter 做一次明确转换
   - `degraded`：可运行但有已声明语义损失
   - `unsupported`：明确不支持

### 状态值

每个 Feature 条目的 `status` 使用以下值：

| 状态 | 含义 |
|------|------|
| `unknown` | 未测试或未确认 |
| `supported` | 已验证支持（native 或 convert） |
| `unsupported` | 明确不支持 |
| `degraded` | 可运行但有已声明语义损失 |
| `planned` | 计划支持但尚未实现 |

### 第一版说明

- 当前矩阵为**计划/声明状态**，大部分 feature 标记为 `unknown`
- **禁止伪造测试结果**：只有跑过测试并通过的才能标记 `supported`
- `unknown` 不等于 `supported`，不会被自动猜成 PASS
- 后续 #116/#117/#118/#62 跑完后逐项回填 evidence

## 统一测试结果 (`test-result.schema.json`)

### 必填字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `case_id` | string | 稳定 case ID |
| `protocol_in` | string | 北向入口协议 |
| `result` | string | PASS/FAIL/UNSUPPORTED/DEGRADED/SKIPPED |
| `feature` | string | 被测能力 |
| `stream` | boolean | 是否流式 |
| `duration_ms` | number | 执行耗时 |
| `tool_name` | string | 执行工具名 |
| `tool_version` | string | 执行工具版本 |
| `timestamp` | string | ISO 8601 UTC 时间 |

### 结果判定原则

- **PASS**：全部断言通过。`HTTP 200` 不能单独判定 PASS。
- **FAIL**：至少一个断言失败。必须同时填写 `failure_class`。
- **UNSUPPORTED**：协议/能力明确不支持，不是错误。
- **DEGRADED**：可运行但有已声明语义损失。
- **SKIPPED**：未执行（前置条件不满足或手动跳过）。

### 失败分类 (`failure_class`)

| 分类 | 含义 |
|------|------|
| `GATEWAY_BUG` | 网关自身缺陷 |
| `UPSTREAM_LIMITATION` | 上游 Provider 限制 |
| `MODEL_BEHAVIOR` | 模型行为差异（非网关问题） |
| `FLAKY` | 不稳定/间歇性失败 |
| `TOOL_LIMITATION` | 测试工具自身限制 |

### `evidence` 字段

只允许保存 metadata 和 assertion 摘要。**禁止**保存：
- 完整 prompt / response 正文
- thinking / reasoning 内容
- Authorization header 或 API Key

## Case ID 命名规则

### 格式

```
{surface}.{feature}.{scenario}
```

### 规则

1. 全部小写，使用下划线 `_` 分隔单词
2. `surface` 对应协议简称：`chat` / `responses` / `messages` / `common`
3. `feature` 对应能力名
4. `scenario` 描述具体场景
5. **Case ID 一旦进入报告，不因函数名或文件移动随意修改**

### 示例

```text
# OpenAI Chat Completions
chat.text.basic
chat.text.multi_turn
chat.stream.done
chat.stream.event_order
chat.tool.required
chat.tool.parallel
chat.tool.roundtrip
chat.usage.non_stream
chat.usage.stream
chat.error.401
chat.error.429
chat.structured_output.json_schema

# OpenAI Responses
responses.text.basic
responses.stream.event_order
responses.stream.sequence_numbers
responses.tool.roundtrip
responses.tool.output
responses.thinking.basic
responses.web_search.basic
responses.usage.non_stream
responses.usage.stream

# Anthropic Messages
messages.text.basic
messages.stream.event_order
messages.tool.roundtrip
messages.thinking.basic
messages.usage.non_stream
messages.usage.stream
messages.error.401

# 跨协议通用
common.error.401
common.error.429
common.error.5xx
common.cancel.client_disconnect
common.timeout.idle
common.timeout.total
common.fallback.429
common.fallback.5xx
common.fallback.transport_error
```

## 后续消费

| Issue | 如何使用本目录 |
|-------|---------------|
| #115 | Mock Provider fixture 对齐矩阵中的 feature 列表 |
| #116 | Contract test case ID 使用本规则；结果按 `test-result.schema.json` 输出 |
| #117 | 同上，覆盖 streaming/tools/reasoning 高风险 feature |
| #118 | llmprobe/CompatCanary 结果映射到统一 schema；回填矩阵 evidence |
| #119 | 差分测试结果按统一 schema 输出 |
| #62  | Live Provider 结果回填矩阵 |
| #120 | 性能/故障测试结果按统一 schema 输出 |
