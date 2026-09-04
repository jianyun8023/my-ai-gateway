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
>   单次运行探测端点的全部 surface（chat / responses / messages / models 等），
>   没有 `--spec`/`--filter`/`--base-url` CLI 参数；实际调用方式为
>   `npx llmprobe@<version> <base-url> -k <key> -m <model> --quick --no-bench --json --no-save --no-color`。
>   runner 侧的 `--spec`/`--filter` 是扫描后对归一化结果的过滤，不传给 llmprobe。
>   `--quick` 深度只跑 surface 探测 + core conformance；capability/agentic/eval 相位
>   为 not-run，记录在归一化报告的 `phases` 元数据中。
>   已知过度断言：0.6.1 对 Responses 流 MUST 断言 `data: [DONE]` 终止符，而真实
>   OpenAI Responses API 以 `response.completed` 结束、不发 `[DONE]`；该断言在
>   `tests/conformance/config.json` 登记为断言级 known gap（按失败断言 id 匹配，
>   不会掩盖同 case 的其他 MUST 失败）。
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

### `test-contract` 运行方式

`mise run test-contract` 依次运行两个 cargo test target：

1. `cargo test --test mock_provider_tests`：MockProvider 基础设施自测（#115）。
2. `cargo test --test contract_tests --features test-support`：三协议 Contract 测试
   （#116 + #117），包含：
   - #116 非流式基础：Chat Completions 11 case、Responses 9 case、Messages 8 case、
     跨协议 error envelope 和 #84 回归 4 case；
   - #117 高风险协议语义：Streaming Chat 6 case（含 #83 [DONE] 回归）、
     Streaming Responses 7 case（event 顺序断言）、Streaming Messages 6 case
     （content block 状态机）、Tool Calling 17 case（single/parallel/required/named/
     none/result_roundtrip/stream_arguments × 三协议）、Reasoning/Thinking 8 case
     （separate_from_text/stream/multi_turn #88 回归）；
   - 合计 81 个 case，全部离线、确定性、无 Provider Secret。

所有 case 完全离线（无 Provider Secret、无公网访问），使用进程内 MockProvider
作为确定性上游，通过 `test-support` cargo feature 暴露的 `test_gateway_router()`
构造无数据库 Gateway Router。

`cargo test --workspace` 也会自动包含 `contract_tests`（当 `test-support` feature
启用时），因此 `mise run test` 已隐式覆盖这些测试。若只跑 Rust 部分，可直接执行
`cargo test --workspace --features test-support -- --test-threads=1`。

### `test-conformance` 运行方式

`mise run test-conformance` 依次运行 llmprobe 和 CompatCanary（#118）。

两者都默认对本地 conformance-target 运行：

1. 自动编译并启动 `cargo run --example conformance-target --features test-support`，
   进程内同时启动 MockProvider 和 Gateway（无 PostgreSQL），绑定随机端口。
2. Node runner 解析 stdout 获取 `CONFORMANCE_GATEWAY_URL`，等待就绪后执行扫描。
3. 扫描完成后自动清理 conformance-target 进程。

**工具版本**来自 `tests/tooling/versions.json`（唯一事实来源）。

**结果产物**：
- 原始报告：`target/test-reports/conformance/llmprobe/` 和
  `target/test-reports/conformance/compatcanary/`
- 归一化摘要：`*-normalized.json`，符合 `tests/coverage/test-result.schema.json`

**可选参数**：
- `--target external`：对外部 Gateway 运行（需设 `CONFORMANCE_GATEWAY_URL` 等 env）
- `--spec`：仅保留指定协议的归一化结果（chat-completions / responses /
  anthropic-messages；models 为共享 surface，所有 spec 均保留）。llmprobe 本体
  不支持按协议选择，runner 在一次完整扫描后过滤结果
- `--profile`：CompatCanary 仅扫描指定 profile（chat / modern）
- `--filter`：按 case_id 子串过滤归一化结果

**llmprobe 通道的失败语义**：JSON 解析失败或零有效用例视为工具故障（exit 2），
绝不允许 0 case 假通过；归一化 FAIL 计数是唯一判定依据（llmprobe 原始退出码
对已登记 known gap 的 MUST 失败也会非零，仅作诊断）。

### `test-sdk-smoke` 运行方式

`mise run test-sdk-smoke` 运行 OpenAI 和 Anthropic 官方 SDK smoke 测试（#118）。

- **OpenAI SDK**（6 case）：Chat basic、Chat stream、Responses basic、Responses
  stream、Tool basic、Usage。
- **Anthropic SDK**（4 case）：Messages basic、Messages stream、Tool basic、Usage。

SDK 版本固定在 `scripts/conformance/sdk-smoke/package.json`（devDependency 精确版本）。
首次运行自动 `npm install`。测试同样使用 conformance-target 本地 Gateway。

### `test-differential` 运行方式

`mise run test-differential` 运行 Direct vs Gateway 差分测试（#119）。

**两种运行模式：**

1. **离线自测**（默认，`DIFFERENTIAL_TESTS` 未设置）：运行
   `scripts/differential/differential.test.mjs` 中的 49 个 node:test 用例，
   覆盖归一化器、比较器、分类器、参数解析和用例加载。不消耗真实 Token、不访问网络。

2. **真实 Provider**（`DIFFERENTIAL_TESTS=1`）：对同一 Provider 分别发起直连请求和
   经 Gateway 代理请求，比较归一化后的协议语义。需要 `.env.live` 凭据和运行中的
   Gateway 实例。

**第一阶段 Provider**：DeepSeek（openai_chat_completions 原生协议，当前最稳定）。

**用例定义**（`tests/differential/cases.json`，6 类）：
- `text.basic`：非流式文本完成
- `stream.basic`：流式文本完成
- `tool.single`：单次工具调用
- `tool.roundtrip`：工具调用往返
- `usage.basic`：Token 使用量字段
- `error.rate_limit_429`：限流错误信封

**归一化器**（`scripts/differential/normalize.mjs`）显式列出每个被忽略的动态字段及理由：
`strip_dynamic_ids`、`strip_timestamps`、`normalize_finish_reason`、
`normalize_usage_shape`、`normalize_tool_call`、`normalize_stream_events`。

**结果分类**遵循 #119 矩阵：
- 双侧失败同类 → `UPSTREAM_LIMITATION`
- Direct PASS + Gateway FAIL → `GATEWAY_BUG`
- 双 PASS 但 Gateway 丢字段 → `GATEWAY_BUG`
- 模型随机性 → `MODEL_BEHAVIOR`
- 网络偶发 → `FLAKY`

**结果产物**：`target/test-reports/differential/differential-report.json`，
符合 `tests/coverage/test-result.schema.json`。报告仅保存 metadata/assertion。

**可选参数**：
- `--provider <id>`：只运行指定 Provider 的用例
- `--model <id>`：覆盖模型
- `--case <id>`：只运行指定用例
- `--list`：列出匹配用例

**失败语义**：零有效用例 exit 2（绝不允许假通过）；至少一个 FAIL exit 1。

**安全**：不提交密钥，复用 `.env.live`，报告不存完整 prompt/response/thinking。

### AI Ping — 可选手工排障入口

[AI Ping](https://github.com/thinkall/ai-ping) 支持 OpenAI / Anthropic 协议的
快速连通性检查，适合手工排障或提供第二意见。

**不进入任何自动门禁**，原因：与 llmprobe/CompatCanary 功能重叠，维护两套门禁
增加复杂度但不增加覆盖面。

手工使用：

```bash
# 安装（一次性）
npx ai-ping --help

# 对本地 Gateway 快速检查
npx ai-ping --provider openai \
  --base-url http://127.0.0.1:8787/v1 \
  --api-key sk-test \
  --model conformance-test-model

# 对真实 Provider 检查
npx ai-ping --provider anthropic \
  --base-url https://api.anthropic.com \
  --api-key $ANTHROPIC_API_KEY \
  --model claude-sonnet-4-20250514
```

### `verify` 策略

第一版保持现有 `verify` 快速稳定，不立即把第三方扫描和 load 塞进去。

### CI 策略（#118 决策）

- **Rust Contract**（`test-contract`）：默认 CI 必跑。
- **CompatCanary against local Gateway**（`test-conformance` 中 CompatCanary 部分）：
  可进入默认 CI——运行时间短、确定性强、无外部依赖。
- **llmprobe**：因运行时间较长且依赖 npx 下载，建议作为独立 CI job 或
  release/manual gate；待稳定后可合入默认 CI。
- **SDK Smoke**（`test-sdk-smoke`）：可进入默认或独立 CI job。
- **AI Ping**：不进 CI。
- **真实 Provider**：仍由 #62 opt-in。

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
