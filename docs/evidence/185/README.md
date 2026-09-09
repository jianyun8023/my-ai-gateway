# RC1 gzip / Kimi smoke 修复复验

2026-09-09，关联 [#185](https://github.com/jianyun8023/my-ai-gateway/issues/185) 与
[#186](https://github.com/jianyun8023/my-ai-gateway/issues/186)。实现提交为
`d69724dfae41005c8ec145856224332b5999d54f`，基于 RC1 的 `68b849d`。
使用本机构建的网关、生产 Provider 凭据、专用本地 PostgreSQL 测试库和数据库 Virtual Key。
没有访问生产数据库；临时 schema 均已清理。

## 本地验证

| 检查 | 结果 |
| --- | --- |
| `mise run config-check` | 通过 |
| `cargo +1.97.1 fmt --all -- --check` | 通过 |
| `cargo +1.97.1 clippy --locked --all-targets --features test-support -- -D warnings` | 通过 |
| `cargo +1.97.1 test --locked --workspace --features test-support -- --test-threads=1` | lib 219、Contract 90、Mock 35 通过；10 项数据库测试单独运行 |
| 配置专用 `TEST_DATABASE_URL` 后，`cargo +1.97.1 test --locked --workspace -- --ignored --test-threads=1` | PostgreSQL 10/10 通过 |
| `cargo +1.97.1 build --locked` | 通过 |
| `mise run test` 中的五组 Node runner 测试 | 182/182 通过 |

Rust 使用固定的 1.97.1 工具链及 `/tmp` 下的 Cargo cache/target；Node 为 24.11.1。
本轮未在本地重跑未修改的 Web 部分，完整门禁由关联 PR CI 验证。
新增 gzip 测试覆盖三协议 JSON/SSE、增量首事件、编码/长度头与截断压缩错误；
修复前 JSON 回归会因 `estimated` 而失败，修复后通过。

## 真实 Provider

正式 runner 命令为 `node scripts/live-provider-smoke.mjs --strict-known-issues`，
通过本机 ignored `.env.live` 注入配置，并将 `GATEWAY_BIN` 指向修复后的构建产物。
默认低成本选集保持不变，未设置 `Accept-Encoding: identity`。

| Case | 结果与用量证据 |
| --- | --- |
| `deepseek.function` | 工具往返通过，两阶段 `upstream` |
| `minimax.function` | 工具往返通过，两阶段 `upstream` |
| `kimi.function` | `low + auto` 工具往返通过，273 / 437 Token，两阶段均为 `upstream` 且精确对账 |
| `kimi.function_stream` | 工具名、参数 delta/done、事件顺序与终止均通过，253 Token，`parsed` 且精确对账 |
| `fallback.deepseek_bai` | 本机故障 Source 503 → 真实 b.ai 200，两次 attempt 归因正确 |

完整 [live smoke 元数据](live-smoke.json)：run ID `2026-09-09T02-58-07-307Z-becc2a00`，
5 passed，0 failed / unavailable / known-issue failure，全部首次执行通过。

另复用同一 runner 的隔离网关与 Usage helper，发送普通非流式短请求，逐字段对照上游
usage 与持久化事件。Kimi Chat / Responses / Messages 三项均 HTTP 200，正常完成、
`usage_source=upstream`，解码后的响应没有残留 `Content-Encoding`。
Chat 为 118 Token，Responses 为 122；Messages 按既有契约记录 input=0、output=36、
total=36、cache_read=91（缓存不重复加到 total）。证据见 [Chat/Responses](kimi-json.json)
和 [Messages](kimi-messages.json)。

这三项短请求在提交前执行，原始 artifact 如实保留 `68b849d` + `dirty=true`；
当时被测的源码随后原样提交为 `d69724d`。正式 smoke 完成时已提交，记录 `d69724d`。
文件仅包含协议与用量元数据，不保存请求/响应正文或凭据。

## 验收边界

这是修复提交的本机真实上游复验，未部署生产、未改变已发布的 `v0.1.0-rc.1` 标签或镜像。
高成本搜索/signature、真实 Codex E2E、真实 Provider 性能以及历史 #96/#97/#98 生产数据
复验没有在本轮执行，不能据此标记为完成。
