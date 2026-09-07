# Gateway 本地性能基线

时间：2026-09-07T04:48:58.977Z；目标：mock；profile：release/test-support/no-database

TTFT 为首个非空文本 delta；非流式 TTFT 不适用。差值为 Gateway 分位数减 Direct 分位数，保留负值，不代表逐请求配对差值。

| Case | 并发 | 结果 | Direct/Gateway 请求数 | Gateway 成功率 | Δ latency p50/p95/p99 ms | Δ TTFT p50/p95/p99 ms |
| --- | ---: | --- | --- | ---: | --- | --- |
| chat.json | 1 | PASS | 100 / 100 | 100.00% | 0.07 / 0.04 / 0.02 | — |
| chat.sse | 1 | PASS | 100 / 100 | 100.00% | -1.00 / 1.05 / 6.95 | 1.00 / 1.00 / -0.05 |
| responses.json | 1 | PASS | 100 / 100 | 100.00% | 0.09 / 0.06 / 0.05 | — |
| responses.sse | 1 | PASS | 100 / 100 | 100.00% | 0.00 / 1.05 / 4.99 | 0.00 / 1.00 / 5.01 |
| chat.json | 5 | PASS | 500 / 500 | 100.00% | 0.15 / 0.29 / 0.33 | — |
| chat.sse | 5 | PASS | 500 / 500 | 100.00% | 3.00 / 3.00 / 3.05 | 2.00 / 2.00 / 1.00 |
| responses.json | 5 | PASS | 500 / 500 | 100.00% | 0.15 / 0.18 / 0.20 | — |
| responses.sse | 5 | PASS | 500 / 500 | 100.00% | 0.00 / 4.00 / 9.02 | 0.00 / 1.00 / 0.01 |
| chat.json | 10 | PASS | 1000 / 1000 | 100.00% | 0.06 / 0.24 / 0.35 | — |
| chat.sse | 10 | PASS | 1000 / 1000 | 100.00% | -1.00 / -2.00 / -5.00 | 0.00 / 0.00 / -2.00 |
| responses.json | 10 | PASS | 1000 / 1000 | 100.00% | 0.05 / 0.07 / 0.18 | — |
| responses.sse | 10 | PASS | 1000 / 1000 | 100.00% | 2.00 / 0.00 / -1.01 | 2.00 / -0.05 / -4.98 |
| chat.json | 25 | PASS | 2500 / 2500 | 100.00% | 0.47 / 1.01 / 1.79 | — |
| chat.sse | 25 | PASS | 2500 / 2500 | 100.00% | 2.00 / 0.00 / -21.00 | 1.00 / 0.00 / -2.00 |
| responses.json | 25 | PASS | 2500 / 2500 | 100.00% | 0.46 / 0.89 / 1.36 | — |
| responses.sse | 25 | PASS | 2500 / 2500 | 100.00% | 1.00 / 0.00 / -5.97 | 1.00 / 1.00 / -7.00 |
| chat.json | 50 | PASS | 5000 / 5000 | 100.00% | 0.85 / -0.09 / -0.89 | — |
| chat.sse | 50 | PASS | 5000 / 5000 | 100.00% | 1.00 / 2.00 / 0.00 | 1.00 / 2.00 / 2.00 |
| responses.json | 50 | PASS | 5000 / 5000 | 100.00% | 0.82 / 0.09 / -0.70 | — |
| responses.sse | 50 | PASS | 5000 / 5000 | 100.00% | 2.00 / 1.00 / -1.00 | 2.00 / 2.00 / 2.00 |
| chat.json | 100 | PASS | 10000 / 10000 | 100.00% | 1.80 / 3.01 / 4.16 | — |
| chat.sse | 100 | PASS | 10000 / 10000 | 100.00% | 0.00 / 4.00 / 8.00 | 1.00 / 1.00 / 9.00 |
| responses.json | 100 | PASS | 10000 / 10000 | 100.00% | 1.75 / 2.69 / 4.25 | — |
| responses.sse | 100 | PASS | 10000 / 10000 | 100.00% | 0.00 / -1.00 / 0.00 | 1.00 / 0.00 / 0.00 |

## 环境与边界

```json
{
  "os": "darwin",
  "arch": "arm64",
  "release": "27.0.0",
  "cpu": "Apple M3 Max",
  "logical_cpus": 16,
  "memory_bytes": 68719476736,
  "node": "v24.11.1",
  "gateway_commit": "c4da71703117a566a34eac09a2099d41f95c1c7f",
  "working_tree_dirty": true,
  "network": "loopback, Mock and Gateway share a process",
  "iterations_per_vu": 100,
  "concurrency": [
    1,
    5,
    10,
    25,
    50,
    100
  ]
}
```

- 本地 Mock 使用 release/test-support，不包含 PostgreSQL Usage 持久化、真实 Provider 或生产网络开销。
- 每个 case/并发组合使用独立 Mock/Gateway 进程和端口，两路径共享同一进程；各路径先执行 5 次未计入指标的预热；Direct/Gateway 按档位交替执行先后顺序。
- TTFT/SSE 总时长使用客户端毫秒时钟；JSON 总时长采用 k6 HTTP duration（不含 DNS/连接建立）；吞吐分母为 k6 testRunDurationMs。
- 本地 SSE 单路径达到 4,000 请求时，执行前等待 31 秒回收前序连接；此间隔不计入指标。
- 短样本的 p95/p99 仅用于探索；负差值表示测量噪声/调度差异，不截为零。
- 未达到请求数或完成率门限判 FAIL，工具失败不会记为 SKIPPED/PASS；失败后的更高并发标为 SKIPPED/PRIOR_FAILURE。
