# RC2 SSE 用量零值修复复验

2026-09-09，关联 [#188](https://github.com/jianyun8023/my-ai-gateway/issues/188)。
修复提交 `4870b32a493c096cc5a427f1617232fc26ade257`，基于 RC2 `89528ea`。
本机使用 Rust 1.97.1 release 构建、生产 Provider 凭据、专用本地 PostgreSQL 测试库，
每个 case 独立 schema、网关进程及数据库 Virtual Key，所有临时 schema 均已清理。

## 本地验证

新增 5 组用量回归在旧实现上全部失败；修复后用量测试 39/39 通过。
覆盖显式零值、缺失字段、全零 usage、缓存拆分、嵌套详情，以及分段 total 推导。

| 命令 | 结果 |
| --- | --- |
| `cargo +1.97.1 fmt --all -- --check` | 通过 |
| `cargo +1.97.1 clippy --locked --all-targets --features test-support -- -D warnings` | 通过 |
| `cargo +1.97.1 test --locked --workspace --features test-support -- --test-threads=1` | lib 224、Contract 90、Mock 35 通过；10 项数据库测试单独执行 |
| 专用 `TEST_DATABASE_URL` + `cargo +1.97.1 test --locked --workspace -- --ignored --test-threads=1` | PostgreSQL 10/10 通过 |
| `cargo +1.97.1 build --release --locked` | 通过 |

## 真实 Provider

复用 RC2 协议矩阵驱动及当前 smoke runner 的隔离/用量 helper，使用普通 HTTP 压缩协商，
对 DeepSeek、MiniMax、Kimi 分别发送三协议 JSON/SSE 短请求。
完整 [metadata-only 报告](protocol-matrix.json) 保留准确提交、时间、事件类型与 usage 字段。
三家各 6/6，合计 **18/18 通过**，没有重试覆盖失败结果。

Kimi Messages SSE 实际覆盖了原始故障：

| 阶段 | input | output | cache read | total |
| --- | --- | --- | --- | --- |
| 上游 message_start | 91 | 0 | 0 | 未提供 |
| 上游 message_delta | 0 | 40 | 91 | 未提供 |
| 网关持久化 | **0** | **40** | **91** | **40** |

`usage_source=parsed`，请求 ID `6ef7c99d-9e47-417e-8ab0-c658ab755afc`；全部既有 Token 字段精确对账。
修复前 RC2 两次复验都错误保留 input=91，原失败报告仍保存在 RC2 Release 中。

未访问生产数据库、未部署生产，未改动 RC2 标签。高成本搜索/signature、真实 Codex E2E
与 Provider 性能不在本轮范围；本地未重跑未修改的 Web/Node 部分，由关联 PR CI 验证。
