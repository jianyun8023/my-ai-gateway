# 2026-09-07 Apple M3 Max 本地基线

最终结果见 [summary.md](summary.md) 和 [summary.json](summary.json)：**24/24 组合 PASS，Direct/Gateway 合计 152,800 次请求，成功率与流完成率均为 100%**。执行命令为 `mise run test-load -- --target mock`（本次通过同一 Node runner 直接执行）；固定工具、预热、测量定义和限制见 [运行说明](../../README.md)。

代码基线是 main `c4da717` 加本验收分支工作区改动。机器为 Apple M3 Max / 16 逻辑 CPU / 64 GiB / darwin arm64；release/test-support，Mock 与 Gateway 共享进程，没有 PostgreSQL usage 持久化和真实 Provider 网络。

| 100 并发 | Gateway 新增总延迟 p50 / p95 / p99 ms | Gateway 新增 TTFT p50 / p95 / p99 ms |
| --- | --- | --- |
| Chat JSON | 1.80 / 3.01 / 4.16 | 不适用 |
| Chat SSE | 0 / 4 / 8 | 1 / 1 / 9 |
| Responses JSON | 1.75 / 2.69 / 4.25 | 不适用 |
| Responses SSE | 0 / -1 / 0 | 1 / 0 / 0 |

以上为两条路径分位数之差，负数反映调度/测量噪声；短 JSON 测量不足以给出长期容量保证。

## 保留的失败与复验记录

- [initial-continuous-failed.json](initial-continuous-failed.json)：最初连续使用同一 target 的矩阵，在 50 并发 Chat SSE 出现 Direct/Gateway 同时失败，成功率为 82.58% / 18.82%；后续组合跳过。该版尚无失败分类计数。
- [standalone-50-sse-passed.json](standalone-50-sse-passed.json)：单独重跑相同 50 并发 Chat SSE，5,000/5,000 次均成功。
- [isolated-without-cooldown-failed.json](isolated-without-cooldown-failed.json)：仅隔离进程/端口仍失败，Direct 89 次、Gateway 3,933 次错误归为 resource_exhaustion。
- 最终 runner 对本地单路径至少 4,000 次请求的 SSE 组增加执行前 31 秒间隔，再执行全矩阵全部通过。等待不计入测量，不减少并发或请求数。

xk6-sse 锁定版本每次请求创建独立 HTTP Transport，并调用 CloseIdleConnections；上游源码已标注缺少跨迭代 transport 复用。本机只读 sysctl 查询得到临时端口 49152–65535、TCP MSL 15,000 ms。分类错误、独立复验与回收间隔后的结果共同支持“前序 SSE 连接累积造成客户端端口压力”的解释；未将首次失败认定为 Gateway 容量上限。所有报告只保存指标和环境元数据，不含 Key、完整服务 URL 或请求/响应正文。
