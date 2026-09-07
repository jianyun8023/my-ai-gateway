# 性能基线与故障测试（#120）

本地性能测试比较同一 Mock 的 Direct 与 Gateway 路径；故障行为由 Rust Contract 验证，两者分开执行。测试不评价模型回答质量，不将公网 Provider 延迟直接当作 Gateway SLA。

## 固定工具链

版本事实来源为 [`../tooling/versions.json`](../tooling/versions.json)：k6 **2.2.0**、xk6 **1.4.12**、xk6-sse **0.1.13-0.20260818094211-37cc4724690d**。

原先锁定的 xk6-sse 0.1.12 使用 `go.k6.io/k6`，与 k6 2.x 的 `go.k6.io/k6/v2` 不兼容：构建会警告扩展未激活，自动解析也无法识别。改为锁定上游 [k6 v2 适配提交 37cc472](https://github.com/phymbert/xk6-sse/commit/37cc4724690d8486853de0df7ec325d04df92c5e)，不追踪浮动 main/latest。

本机准备 Go 后运行（首次会下载构建依赖）：

```bash
mise run build-load-tools
```

输出 `target/tools/k6`，测试前检查 k6 版本和已激活的 SSE 扩展版本。可以用 `K6_BIN=/absolute/path/to/k6` 指定同版本已有二进制。不满足版本要求直接失败，不在测试时自动下载/升级扩展。

## 本地 Mock 基线

```bash
mise run test-load -- --list
mise run test-load -- --target mock
# 小范围验证 runner
mise run test-load -- --target mock --concurrency 1,5 --iterations 10 --case chat.sse,responses.sse
```

配置在 [`scenarios.json`](scenarios.json)，默认覆盖：

- Chat / Responses × JSON / SSE 四类请求；
- `1 / 5 / 10 / 25 / 50 / 100` 并发，每 VU 100 次；
- 每个 case/并发组合启动独立的 release conformance-target 进程与随机端口，同一组 Direct/Gateway 使用相同进程中的 Gateway 与 Mock；
- 每次 k6 运行先做 5 次不计入自定义指标的预热；相邻并发档位交替 Direct/Gateway 的执行先后顺序。

报告写到 `target/test-reports/load/<UTC时间>/summary.json` 与 `summary.md`，每条路径保留仅含指标的 k6 JSON 摘要。包含请求数、成功率、TTFT/总延迟 p50/p95/p99、请求吞吐、流完成率和 Gateway 增量延迟，以及 OS/CPU/内存/工具版本/代码基线/并发档位。

JSON 总时长采用 k6 高精度 `response.timings.duration`（发送+等待+接收，不含 DNS/连接建立）；SSE 总时长采用客户端毫秒时钟。TTFT 测的是客户端收到首个非空文本 delta 的时刻（毫秒精度），不将角色事件、心跳或响应头当成首 Token；非流式 TTFT 为 null。Gateway 增量是两组分位数相减，不是逐请求配对延迟；保留负值以反映测量噪声。吞吐分母是 k6 的 `testRunDurationMs`。Mock/网关共享进程，test-support 不包含 PostgreSQL Usage 持久化与生产健康状态，不能将这组结果当作生产端到端开销。

xk6-sse 每次请求创建独立 HTTP Transport，连续 SSE 负载可能耗尽客户端临时端口。每组进程隔离之外，本地 SSE 单路径达到 4,000 请求时，runner 会在执行前等待 31 秒（不计入指标）；本机 TCP MSL 为 15 秒，TIME_WAIT 回收约需 30 秒。此安排用于清除前序组合的连接压力，不代表修改 Gateway 的并发或限流策略。单次超大请求量、不同系统端口限制仍可能导致工具资源不足，需通过报告中的 failure_counts 区分。

零请求、指标缺失、请求数不足、失败请求或未正常结束的流均不能通过；工具异常记为 FAIL，后续未执行组合记为 SKIPPED/PRIOR_FAILURE，不伪造成功。停止原因需先查看失败组合。默认每组 k6 scenario 最大 60 秒，runner 为子进程设置总时间上限；不要在其他测试或构建争抢 CPU 时采集基线。

## 已保存的本地基线

[2026-09-07 Apple M3 Max 基线与复验记录](baselines/2026-09-07-m3-max/README.md)：四类请求 × 六档并发全部通过，共 152,800 次请求。保存了首次连续 SSE 资源不足失败、单独复验以及最终完整结果；不使用失败样本估算 Gateway 容量。

## 真实 Provider 差值（显式执行）

`--target live` 需要 `LOAD_TESTS=1`，使用现有 Gateway 与同一实际 Provider：

```bash
# 从本机 Secret 环境注入下列变量；不要把真实值写进仓库或命令行参数。
# LOAD_DIRECT_URL / LOAD_GATEWAY_URL：不含协议路径的 base URL，无 userinfo/query/hash
# LOAD_DIRECT_MODEL / LOAD_GATEWAY_MODEL：同一个实际模型的上游名 / 逻辑名
# LOAD_DIRECT_KEY / LOAD_GATEWAY_KEY：Provider Key / 数据面 Virtual Key，分别设置
LOAD_TESTS=1 mise run test-load -- --target live --concurrency 1 --iterations 10 --case chat.sse
```

该模式会消耗 Token。本轮只执行本地 Mock 基线，未执行真实 Provider 压测。外部部署的硬件、网络、Gateway 配置与 Provider 限流仍须单独记录。报告不保存 Key、完整 URL 或 prompt/response；k6 stderr 不持久化，故障信息只保留工具退出状态及 HTTP、流未完成、JSON 解析、流错误、传输、疑似资源耗尽等分类计数，不保存错误原文。

## 故障验证

```bash
mise run test-faults
```

入口运行三组现有/新增测试：

1. `tests/contract_tests/faults.rs`：三协议 400/401/403/404/429/500/502/503、损坏/空错误响应、单账号短 Retry-After 重试、备用账号模型映射、连接关闭、响应头前超时、首事件/idle 超时、空/损坏/截断 SSE、多 choice 截断、下游断开后上游 TCP 连接结束。
2. `src/proxy/stream.rs`：心跳、首事件/idle/total timeout、上游错误与 drop 清理。
3. `src/api/runtime_usage_tests.rs`：请求与 attempt 的实际模型/来源及 fallback 归因；跨 Source 的 PostgreSQL 持久化回归由 `mise run test-postgres` 另行执行。

重试遵守当前实现：无 fallback 且 429 的 Retry-After 不超过 2 秒才同账号重试一次；500 不会无条件重试，不为符合旧 Issue 示例增加隐式重试。流式响应已发出 HTTP 200 后用协议错误事件表达失败；Chat 错误帧后可以有 `[DONE]`，但不能只有正常结束标记。

本次修正 Chat EOF：只有所有已出现的 choice 都给出非空 `finish_reason`，才补缺失的 `[DONE]`；缺少结束证据时输出 `gateway_upstream_error`。正常 finish 后到达的 usage chunk 保留。非流式 native 上游的错误 HTTP 状态与字节仍原样透传。

`mise run verify` 包含新增 Contract 与 runner 单测，不包含性能负载。`test-all-offline` 包含完整本地负载，因此需要预先构建 k6；它强制使用 Mock 与离线差分，不会因继承 live 开关而调用真实模型 API。外部扫描工具安装需要网络，准备好依赖后的测试目标仅为本机。
