# Direct vs Gateway 差分测试

Issue: [#119](https://github.com/jianyun8023/my-ai-gateway/issues/119)

## 目标

验证 **同一 Provider 直连** 与 **经 Gateway 代理** 的响应在协议语义上是否一致。
只比较结构和行为，不比较自然语言文本是否逐字相同。

## 目录结构

```text
scripts/differential/
├── run.mjs                  # 主 runner — 双路请求 + 比较 + 报告
├── compare.mjs              # 比较引擎 — 断言执行 + 结果分类
├── normalize.mjs            # 显式归一化器 — 每个动态字段都有文档说明
└── differential.test.mjs    # 离线单元测试（node:test，无真实 Provider）

tests/differential/
├── cases.json               # 测试用例定义
└── README.md                # 本文件
```

## 快速上手

### 离线自测（不消耗 Token）

```bash
# 运行所有离线单元测试
node --test scripts/differential/differential.test.mjs

# 通过 mise 运行（无 DIFFERENTIAL_TESTS=1 时自动走离线路径）
mise run test-differential
```

### 真实 Provider 测试（需要凭据）

```bash
# 1. 配置 .env.live（参考 .env.live.example）
# 2. 启动 Gateway 实例
# 3. 显式 opt-in 运行

DIFFERENTIAL_TESTS=1 \
  node scripts/differential/run.mjs \
    --provider deepseek \
    --case text.basic
```

### 命令行选项

| 选项 | 说明 |
|------|------|
| `--provider <id>` | 只运行指定 Provider 的用例 |
| `--model <id>` | 覆盖所有用例的模型 |
| `--case <id>` | 只运行指定用例（可重复；显式点名会同时解除默认排除） |
| `--include-error` | 默认排除 `error_expected` 用例，此开关重新纳入 |
| `--include-high-cost` | 默认排除 `cost=high` 用例，此开关重新纳入 |
| `--env-file <path>` | 环境文件路径（默认 `.env.live`） |
| `--list` | 列出匹配的用例，不执行 |
| `--timeout <ms>` | 单请求超时（默认 30000） |

> 默认排除策略（#119 安全要求）：`error_expected` 和高成本（`cost=high`）用例
> 不随默认集合运行，需显式 `--case` 点名或对应 `--include-*` 开关。

### 环境变量

| 变量 | 说明 |
|------|------|
| `DIFFERENTIAL_TESTS=1` | 必须显式设置才能运行真实测试 |
| `DIFFERENTIAL_GATEWAY_URL` | Gateway 实例的 base URL |
| `GATEWAY_API_KEY` | Gateway 侧入口凭据（Gateway 鉴权只认它或 DB Virtual Key；不能用 Provider key） |
| `DEEPSEEK_API_KEY` | DeepSeek API 密钥（仅用于直连路径） |
| `DEEPSEEK_BASE_URL` | DeepSeek API base URL |
| `DEEPSEEK_MODEL` | 可选，覆盖默认模型 |

## 用例定义（cases.json）

第一版覆盖 6 类核心场景：

| case_id | feature | 说明 |
|---------|---------|------|
| `text.basic` | text | 非流式文本完成 |
| `stream.basic` | streaming | 流式文本完成 |
| `tool.single` | tools | 单次工具调用 |
| `tool.roundtrip` | tool_result | 工具调用 → 结果 → 最终回复 |
| `usage.basic` | usage | Token 使用量字段结构 |
| `error.rate_limit_429` | error_envelope | 限流错误信封 |

## 归一化器

每个被忽略或归一化的动态字段都在 `normalize.mjs` 中显式列出并注明理由：

| 归一化器 | 影响字段 | 理由 |
|----------|----------|------|
| `strip_dynamic_ids` | `id`, `system_fingerprint`, `tool_calls[].id` | Provider 生成的不透明标识符 |
| `strip_timestamps` | `created` | 两次请求的创建时间必然不同 |
| `normalize_finish_reason` | `choices[].finish_reason` | Provider 对同一语义使用不同命名 |
| `normalize_usage_shape` | `usage.*` | Token 计数可能因 Gateway 开销微有差异 |
| `normalize_tool_call` | `tool_calls[].function.arguments` | JSON 格式/键序差异 |
| `normalize_stream_events` | `(stream chunks)` | SSE chunk 边界是传输层细节 |

**原则：禁止靠"删大量字段"让差分恒过。**

## 结果分类

| 场景 | 结果 | failure_class |
|------|------|---------------|
| Direct FAIL + Gateway FAIL，同类错误 | FAIL | `UPSTREAM_LIMITATION` |
| Direct PASS + Gateway FAIL | FAIL | `GATEWAY_BUG` |
| 双 PASS，Gateway 丢已声明字段 | FAIL | `GATEWAY_BUG` / `DEGRADED` |
| 模型随机行为 | FAIL | `MODEL_BEHAVIOR` |
| 网络偶发 | FAIL | `FLAKY` |
| 双 PASS，所有断言通过 | PASS | — |

## 报告格式

输出符合 `tests/coverage/test-result.schema.json`（#114 统一结果模型）。

报告写入 `target/test-reports/differential/differential-report.json`。

**安全策略：** 报告仅保存 metadata/assertion，不保存完整 prompt/response/thinking/API Key。

## 非目标

- 不做模型质量比较
- 不要求 Direct/Gateway 文本逐字一致
- 不做高并发测试
- 不替代 #62 Live Provider Smoke
