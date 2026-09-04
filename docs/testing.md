# 测试体系

本文档是 AI Gateway 测试分层、工具选型和运行入口的唯一文档化基线。

---

## 1. 测试目标

验证**网关本身**的协议正确性、转换保真性、故障行为和运行时语义，**不**评估模型回答质量。

| 维度 | 涵盖 | 不涵盖 |
|------|------|--------|
| 协议覆盖 | OpenAI Chat Completions / Responses / Anthropic Messages | 非北向协议 |
| 转换正确性 | native/convert 路径字段映射、事件顺序、Usage、Error | 模型质量 Benchmark |
| 真实上游可用性 | 最小稳定 Smoke | Prompt/RAG/Agent 评分 |
| 稳定性 | 并发、SSE、超时、429/5xx、Fallback、取消 | 自然语言质量评估 |

## 2. 分层测试模型

```text
┌──────────────────────────────────────────────────────────────┐
│  Layer 7: Live Provider Smoke            (真实 Provider)     │
├──────────────────────────────────────────────────────────────┤
│  Layer 6: Performance & Fault Injection  (k6/xk6-sse)       │
├──────────────────────────────────────────────────────────────┤
│  Layer 5: Direct vs Gateway Differential (Node runner)       │
├──────────────────────────────────────────────────────────────┤
│  Layer 4: External Conformance & SDK     (llmprobe/CC/SDK)   │
├──────────────────────────────────────────────────────────────┤
│  Layer 3: Streaming / Tools / Reasoning  (Rust Contract)     │
├──────────────────────────────────────────────────────────────┤
│  Layer 2: Basic Protocol Contract        (Rust Contract)     │
├──────────────────────────────────────────────────────────────┤
│  Layer 1: Mock Provider Infrastructure   (Axum Mock)         │
├──────────────────────────────────────────────────────────────┤
│  Layer 0: Tooling & Coverage Baseline    (本 Issue)          │
└──────────────────────────────────────────────────────────────┘
```

## 3. 工具选型定稿

### 3.1 选型总览

| 测试层 | 主工具 | 角色 | 基线状态 |
|--------|--------|------|----------|
| Rust 单元/Contract | `cargo test` + Tokio + Tower/Axum | 协议事实真源 | **是** |
| Mock Provider | 自建 Axum Mock | 固定 JSON/SSE/Tool/Error/Fault | **是** |
| 外部 Conformance | llmprobe (npm) | 主外部协议扫描（三协议） | **是** |
| 轻量 Canary | CompatCanary (npm) | OpenAI-compatible 快速探针 | **是** |
| SDK Smoke | OpenAI/Anthropic 官方 SDK | SDK 解析兼容性 | **是** |
| 多协议交叉检查 | AI Ping | 手工排障/第二意见 | **可选**，不进入门禁 |
| HTTP Load | k6 | 延迟/并发/吞吐 | **是** |
| SSE Load | k6 + xk6-sse | 并发流/TTFT | **是** |
| 模型 Eval | Promptfoo | Prompt/模型质量 | **排除**，不纳入当前基线 |

### 3.2 选型依据

**Rust Contract = 事实真源**：Gateway 关键正确性（字段映射、事件顺序、tool id
关联、error envelope、usage、fallback）是确定性语义，第三方扫描器无法完整覆盖，
Rust Contract 是最终 gate。

**Axum Mock = 确定性上游**：仓库已有 ephemeral Axum mock upstream 先例，可以
精确控制 SSE event 顺序、TCP/body chunk、`[DONE]`/completed/message_stop 缺失、
tool arguments delta、延迟、malformed JSON/SSE、429/5xx 和半流断开。不额外引入
wiremock/httpmock，复杂流式状态机仍需自建 Mock。

**llmprobe = 主外部扫描**：ddalcu/responses-chat-messages-validator，支持
OpenAI Responses / Chat Completions / Anthropic Messages 三协议合规测试，
CLI 可通过 `npx llmprobe` 运行。只使用 conformance surface 能力，不纳入 model
capability benchmark 结果。

**CompatCanary = 快速 Canary**：CognizenOrg/compatcanary，7 项确定性探针
覆盖 models/chat/stream/tool/structured output/Responses。价值在于外部独立
检查和 CI 快速反馈，不代替完整协议证明。

**Official SDK Smoke = 客户端事实**：覆盖"curl 看起来可用但 OpenAI/Anthropic
SDK 解析失败"的情况。

**AI Ping = 可选**：与 llmprobe/CompatCanary 重叠较多，不值得再设一套主门禁；
保留排障入口即可。

**Promptfoo = 当前排除**：本阶段测试 Gateway 协议/运行时，不测自然语言质量。
以后若做路由质量或模型行为回归，单独建 Issue 引入。

### 3.3 第三方工具版本固定

版本事实来源为 `tests/tooling/versions.json`（见下文）。

规则：

- 禁止长期使用 `latest`；
- PR 中记录本次固定版本和选择理由；
- 升级工具必须单独显式修改版本；
- 工具升级导致测试结果变化时，不能静默改 expected baseline；
- 原始报告中记录 tool version。

## 4. 版本固定：`tests/tooling/versions.json`

```json
{
  "llmprobe": "0.6.1",
  "compatcanary": "0.2.2",
  "k6": "2.2.0",
  "xk6_sse": "0.1.12"
}
```

> 以上版本基于 2026-09-04 各工具的最新稳定版本。
> - **llmprobe 0.6.1**：npm `llmprobe`，ddalcu/responses-chat-messages-validator。
>   支持 `--spec responses|chat-completions|anthropic-messages` 三协议和 `--filter` 粒度。
> - **CompatCanary 0.2.2**：npm `compatcanary`，CognizenOrg/compatcanary。
>   支持 `--profile chat|modern` 和 `--format json|markdown`。
> - **k6 2.2.0**：Grafana k6，2026-08-10 发布。
> - **xk6-sse 0.1.12**：phymbert/xk6-sse，k6 community extension。
>   k6 ≥ 2.x 可直接 `import sse from "k6/x/sse"` 自动解析，无需自定义构建。
>   已知限制：`sse.open()` 阻塞事件循环，不支持单 VU 并行 SSE 连接。

## 5. Mise 命令设计

所有测试命令通过 `mise.toml` 注册，遵循现有风格。

| 命令 | 职责 | Issue |
|------|------|-------|
| `mise run test-contract` | Rust 基础 + 高风险 Contract | #116, #117 |
| `mise run test-conformance` | llmprobe + CompatCanary 外部合规 | #118 |
| `mise run test-sdk-smoke` | OpenAI/Anthropic 官方 SDK Smoke | #118 |
| `mise run test-differential` | Direct vs Gateway 差分 | #119 |
| `mise run test-faults` | 故障注入 + 异常行为 | #120 |
| `mise run test-load` | k6/xk6-sse 性能基线 | #120 |
| `mise run test-live` | 真实 Provider Smoke（已有） | #62 |
| `mise run test-all-offline` | 聚合离线门禁（无 Secret/无公网） | — |
| `mise run test` | 既有 Rust + 前端 + Node 单测 | — |
| `mise run verify` | 部署前完整门禁（保持现状） | — |

### `test-all-offline` 约束

- 不读取真实 Provider Secret；
- 不访问公网模型 API；
- 包含 contract + faults + 本地可运行的 conformance/sdk smoke；
- 可作为未来 CI 的完整协议门禁。

### `verify` 策略

第一版保持现有 `verify` 快速稳定，不立即把第三方扫描和 load 塞进去。
后续 #118 决定哪些 conformance job 适合进入默认 CI。

## 6. Issue 到命令/工具的映射

| Issue | 标题 | 对应命令 | 主工具 |
|-------|------|----------|--------|
| #114 | 能力矩阵与测试结果模型 | `config-check`（JSON 校验） | JSON Schema |
| #115 | Mock Provider 基础设施 | `test-contract`（间接） | Axum Mock |
| #116 | 三协议基础 Contract | `test-contract` | cargo test |
| #117 | Streaming/Tools/Reasoning | `test-contract` | cargo test |
| #118 | 外部合规 + SDK Smoke | `test-conformance`, `test-sdk-smoke` | llmprobe, CompatCanary, SDK |
| #119 | Direct vs Gateway 差分 | `test-differential` | Node runner |
| #62  | Live Provider Smoke | `test-live` | Node runner（已有） |
| #120 | 性能 + 故障注入 | `test-load`, `test-faults` | k6/xk6-sse, Axum Mock |

## 7. 回归样本来源

以下已关闭 Issue 的修复应作为后续 Contract/Conformance 测试的回归样本：

| Issue | 描述 | 目标测试层 |
|-------|------|-----------|
| #54 | SSE 心跳、取消与流式超时契约 | #117 Streaming Contract |
| #62 | Provider 工具/搜索 Live Smoke（已有） | `test-live` |
| #83 | MiniMax Chat SSE `[DONE]` | #116/#117 Contract |
| #84 | Anthropic Messages 错误包 | #116 Contract |
| #85 | Responses Web Search citations | #117 Contract |
| #88 | Thinking 多轮兼容问题 | #117 Contract |

这些 Issue 均已关闭并修复，角色是后续 #116/#117 的回归输入，不是待修复缺陷。

## 8. 测试结果模型

详见 [`tests/coverage/README.md`](../tests/coverage/README.md)（#114）。

- 能力矩阵：[`tests/coverage/capability-matrix.json`](../tests/coverage/capability-matrix.json)
- 结果 Schema：[`tests/coverage/test-result.schema.json`](../tests/coverage/test-result.schema.json)

### 8.1 Result 状态

| 状态 | 含义 |
|------|------|
| `PASS` | 断言全部通过 |
| `FAIL` | 至少一个断言失败 |
| `UNSUPPORTED` | 协议/能力明确不支持 |
| `DEGRADED` | 可运行但有已声明语义损失 |
| `SKIPPED` | 未执行（前置条件不满足或手动跳过） |

### 8.2 失败分类

| 分类 | 含义 |
|------|------|
| `GATEWAY_BUG` | 网关自身缺陷 |
| `UPSTREAM_LIMITATION` | 上游 Provider 限制 |
| `MODEL_BEHAVIOR` | 模型行为差异（非网关问题） |
| `FLAKY` | 不稳定/间歇性失败 |
| `TOOL_LIMITATION` | 测试工具自身限制 |

### 8.3 Mode 定义

| 模式 | 含义 |
|------|------|
| `native` | Provider 原生协议直通 |
| `convert` | Gateway/Adapter 做一次明确转换 |
| `degraded` | 可运行但有已声明语义损失 |
| `unsupported` | 明确不支持 |

### 8.4 判定原则

- `HTTP 200` 不能单独判定 PASS；
- `unknown`/`unsupported` 不会被自动猜成 PASS；
- native 优先于 adapter，精确模型优先于通配；
- 允许损失时必须显式标记 `DEGRADED`。
