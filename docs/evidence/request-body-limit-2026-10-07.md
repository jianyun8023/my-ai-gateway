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

## 发布与生产验收

- 用户授权后，将修复 `0f0ba1268abbc8298d4b9d8a6ffc4dd7f8a5823f` 快进推送 GitHub `main`；[镜像发布 37577253107](https://github.com/jianyun8023/my-ai-gateway/actions/runs/37577253107) 成功。多架构 digest 为 `sha256:3732e893fdbcfe4c0aaaabc13e7b447a1a1ee51752063c8b0d28576680fffff7`；linux/amd64 与 linux/arm64 的 revision 标签均匹配修复源码。
- GitOps `main` 发布提交 `b9453ab44db246bbcee5d0c7eaf001c01ecd6b7d`，包含 32 MiB 配置；Deployment 与 Argo CD Application 的 Kustomize 镜像覆盖项同时固定到新 digest。此前 Application 的 `main` 覆盖项会覆盖 Deployment 中的 digest，本次已对齐。
- 收尾观察到 Image Updater v1.1.1 的 `latest` 策略在 05:45:38 UTC 将固定 digest 改回 `main`，随后 root-app self-heal 恢复，Deployment generation 43 → 45。GitOps 提交 `23c75b6` 将本应用改为 `digest` 策略；只修改网关 Application，不改变共享 ImageUpdater 或其他应用规则。
- 05:50:40–05:50:46 UTC 下一轮自动协调结果为 `images_updated=0, errors=0`；网关 digest 未被改写，Deployment generation 保持 45，同一 Pod Ready、重启 0，未再触发重复 rollout。
- `apps/my-ai-gateway` rollout 完成；新 Pod `my-ai-gateway-578cd86f4c-6zppr` Ready、重启 0、实际 imageID 匹配发布 digest。启动日志记录 `max_request_body_bytes=33554432`。网关与 root-app 均 `Synced / Healthy`，HTTPS `/healthz` 正常。
- 滚动切换期间 HTTPS 健康检查出现一次 502，5 秒后复查恢复；完成以下边界验证时只有新 Pod 存活，未再出现 502。
- 直连新 Pod（port-forward）与 HTTPS 分别完成相同的无凭据有效 JSON 请求测试，响应均为协议 JSON，且 `request_id` 与 `x-request-id` 一致：

| 请求 | 字节数 | Pod / HTTPS 结果 |
| --- | ---: | --- |
| Chat、Responses、Messages | 各 6,291,456 | 401 `unauthorized`，已进入正常鉴权 |
| Responses，精确默认边界 | 33,554,432 | 401 `unauthorized` |
| Responses，超限一字节 | 33,554,433 | 413 `request_too_large` |

原始验收元数据：[Pod](request-body-limit-pod-2026-10-07.json)、[HTTPS](request-body-limit-https-2026-10-07.json)。任务与发布进展见 [#249](https://github.com/jianyun8023/my-ai-gateway/issues/249)。这些测试未携带凭据、未发送真实 Provider 推理请求；真实图片识别、生产 SSE、模型 context 与 PostgreSQL 集成不属于此次传输边界复验结果。
