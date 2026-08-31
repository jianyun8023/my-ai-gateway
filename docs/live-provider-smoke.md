# 真实 Provider Smoke 测试

真实 Provider Smoke 用于验证上游当前契约，补充默认 CI 中的 mock 回归。它会产生真实外部请求和 Token 消耗，因此默认关闭，也不由 Pull Request CI 自动运行。

问题与验收范围跟踪在 [Issue #62](https://github.com/jianyun8023/my-ai-gateway/issues/62)。

## 两层测试

| 层级 | 入口 | 外部调用 | 用途 |
| --- | --- | --- | --- |
| 确定性回归 | `mise run verify` | 无 | 请求/响应转换、SSE 顺序、Usage、fallback、数据库契约 |
| Live Smoke | `mise run test-live` | 有 | Provider 当前协议、工具、服务端搜索和真实 fallback |

默认 CI 必须保持无 Provider Secret、无外部模型调用。Live 结果不能代替 mock 回归，Provider 暂时不可用也不能破坏普通 PR 门禁。

## 环境

创建本机 `.env.live`，字段参考 [`.env.live.example`](../.env.live.example)。文件必须保持 Git ignored，建议权限为 `600`。

必须显式设置：

```dotenv
LIVE_PROVIDER_TESTS=1
LIVE_TEST_DATABASE_URL=postgres://gateway:gateway@127.0.0.1:5432/gateway_test
```

`LIVE_TEST_DATABASE_URL` 必须指向专用测试数据库，不能等于运行时 `DATABASE_URL`。Runner 会在该数据库创建随机 schema，启动随机本机端口的网关，并在结束时删除 schema。PostgreSQL 用户需要具有创建/删除 schema 的权限。

Fallback case 的故障 Source 使用本机随机端口。Runner 只在该隔离网关子进程中设置
`GATEWAY_SOURCE_URL_ALLOWLIST=127.0.0.1`，以符合 Source URL 安全策略；它会覆盖外部
同名变量，不会给普通 Provider case 或正在运行的开发网关放宽策略。

## 运行

查看稳定 case ID，不发送外部请求：

```bash
mise run test-live -- --list
```

不带参数时只运行 `cost=low` 的函数调用和 fallback 案例：

```bash
mise run test-live
```

选择单个案例：

```bash
mise run test-live -- --case kimi.web_search_stream
mise run test-live -- --case deepseek.function --case minimax.function
```

显式运行包括搜索在内的全部高成本案例：

```bash
mise run test-live -- --include-high-cost
```

其他选项：

- `--provider deepseek|minimax|kimi|fallback`：按 manifest 中的 Provider 筛选；实际值以 `--list` 输出为准；
- `--strict-known-issues`：已关联 Issue 的已知失败也令命令返回非零；
- `--keep-schema`：调试失败时保留隔离 schema，使用后必须手工清理；
- `--env-file <path>`：覆盖默认 `.env.live`；
- `--output-dir <path>`：覆盖结果目录。

## Case 维护

稳定元数据位于 [`tests/live/provider-smoke.cases.json`](../tests/live/provider-smoke.cases.json)，Runner 位于 [`scripts/live-provider-smoke.mjs`](../scripts/live-provider-smoke.mjs)。

新增或修改 case 时：

1. 保持 case ID 稳定，格式为 `provider.behavior`；
2. 在 manifest 声明 `provider`、`kind`、`cost` 和最小 `required_env`；
3. Runner 只断言协议元数据，不把完整正文写入结果；
4. 为参数解析、配置生成或 SSE 解析增加 Node 确定性单测；
5. Provider 行为变化先更新 Issue/ProviderPreset 版本，不静默放宽断言；
6. 搜索、thinking 等高 Token 案例必须标记 `cost=high`；
7. 暂时无法由上游触发的能力使用 `not_triggered`，不能记作通过。

## 结果与安全

结果写入 Git ignored 的 `target/live-provider-smoke/<run-id>.json`，权限为 `600`。Artifact 只包含：

- commit、case ID、耗时和 outcome；
- HTTP/协议状态、工具名、参数是否合法、事件计数与顺序；
- Source/citation 数量；
- Usage 和 request ID；
- metadata-only 的 Usage attempt。

禁止写入 Authorization、API Key、完整 prompt/response、搜索正文、thinking 或 signature。Runner 失败信息也只保留结构化错误码和状态码。

Live 结果应在对应 Issue/PR 留下 run ID、commit、Provider、case、结果摘要和未覆盖项，不能只保存在本机 artifact。若未来接入 GitHub Actions，只允许手工触发并使用受保护 environment；不得把 Provider Secret 加入普通 PR CI。
