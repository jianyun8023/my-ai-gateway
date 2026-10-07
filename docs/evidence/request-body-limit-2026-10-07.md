# 图片历史请求体 413 修复记录（2026-10-07）

## 生产定位

- 集群 `home-oec`，应用 `apps/my-ai-gateway`，入口 `https://ai-gateway.pvelab.top`。
- 同一规模的 3 MiB 无凭据请求体，经 HTTPS `/v1/responses` 和 Pod 内 `127.0.0.1:8787/v1/responses` 均返回 HTTP 413：`Failed to buffer the request body: length limit exceeded`。诊断未调用真实 Provider。
- 三协议处理器使用 Axum 0.8.9 `Bytes` 提取器；旧版 Router 未设置 `DefaultBodyLimit`，因此使用默认 2 MiB。拒绝发生在 proxy service 之前。
- 生产 Traefik 3.6.7；`my-ai-gateway-ingress` 无 middleware 注解，未挂载 buffering middleware。集群中其他服务的 buffering 不属于网关链路，不修改。
- Argo CD 来源是 `kubernetes_manifests/my-ai-gateway`，开启自动同步与 self-heal；仅改运行中 Deployment 会被 GitOps 覆盖。

## 已准备的修复与验证

- 三协议默认请求体上限 32 MiB，启动读取 `GATEWAY_MAX_REQUEST_BODY_BYTES`，非法/零值拒绝启动；控制面限制不变。
- 数据面超限返回协议 JSON 413，含 `request_too_large`、request ID 和 `x-request-id`；读取失败不回显底层正文/错误。
- 三协议各自的图片字段承载六段约 1 MiB base64 数据，经应用 Router 和真实 HTTP Mock 上游验证原样完整透传。该测试验证传输，不验证图片内容或模型识别。
- 固定长度和无 Content-Length 的分块请求覆盖自定义上限的精确边界、超限一字节；默认 32 MiB 超限验证通过；既有三协议 SSE 回归通过。
- `cargo fmt --all -- --check`、`cargo clippy --all-targets --features test-support -- -D warnings`、`cargo build --workspace`、`mise run config-check` 均通过。
- `cargo test --workspace --features test-support -- --test-threads=1`：243 单元 + 2 架构 + 92 Contract + 35 Mock，共 372 通过，13 ignored。未设置独立 `TEST_DATABASE_URL`，不据此宣称 PostgreSQL 集成验收完成。
- 源码 Kubernetes 基线与实际 GitOps 配置显式设置 `33554432`；两份 Kustomize 渲染通过，实际 GitOps 清单通过 `home-oec` server dry-run。

## 发布边界

本记录为发布前验证：尚未推送/合并源码与 GitOps 变更，尚未构建发布 Linux 镜像或更新生产镜像。旧镜像不识别新变量，必须发布新镜像后核对 digest、启动上限、Pod Ready，再复测 Pod 与 HTTPS 的 6 MiB/32 MiB 边界。真实 Provider、多图片识别、生产 SSE 和模型 context 限制尚未复验。
