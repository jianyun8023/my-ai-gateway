# Kubernetes / K3s 部署参考

本文是当前仓库的脱敏 Kubernetes 部署基线。可复用清单位于 [`deploy/kubernetes/`](../deploy/kubernetes/)，适用于标准 Kubernetes 和 K3s；当前参考部署使用 K3s 自带的 Traefik 作为 Ingress。清单只部署 Gateway，不在集群中重复创建 PostgreSQL。

## 部署边界

- Gateway 监听 `0.0.0.0:8787`，Service 使用 `ClusterIP` 暴露同一端口；
- Deployment 默认 1 个副本、`Recreate` 更新策略，容器以 UID/GID `10001` 的非 root 用户运行；
- `/healthz` 同时用于 startup、readiness 和 liveness probe；
- PostgreSQL 是控制面事实来源，必须使用已有的 PostgreSQL 16 或托管实例；
- Provider、模型、Binding 和 Fallback Route 由 PostgreSQL 控制面保存，不写入公开清单；
- GHCR 镜像包为私有包时，Pod 通过 `imagePullSecrets` 拉取镜像。

如果只是单机开发、测试或需要一并运行 PostgreSQL，请使用 [`docs/deployment.md`](deployment.md) 中的 Docker Compose 方案。

## 前置条件

准备以下资源：

1. 一个可以访问 PostgreSQL 的 Kubernetes/K3s 集群，以及具有创建 `Namespace`、`Deployment`、`Service` 和 `Ingress` 权限的 kubeconfig；
2. 一个专用 PostgreSQL 数据库和登录角色。密码使用 URL-safe 字符，避免未经编码的 `@`、`#` 或 `/` 破坏 `DATABASE_URL`；
3. `apps` 命名空间中的三个外部 Secret：
   - `my-ai-gateway-secret`：`DATABASE_URL`、`GATEWAY_ADMIN_KEY`、`GATEWAY_CREDENTIAL_MASTER_KEY`；
   - `my-ai-gateway-provider-secret`：`GATEWAY_API_KEY`、`KIMI_API_KEY`、`MINIMAX_API_KEY`、`MINIMAX_API_KEY_2`、`DEEPSEEK_API_KEY`、`SILICONFLOW_API_KEY`、`B_AI_API_KEY` 中实际使用的键；
   - `ghcr-my-ai-gateway`：GHCR 的 `docker-registry` Secret；
4. Ingress Controller 和 TLS Secret。K3s 默认通常使用 Traefik；部署前把 `deploy/kubernetes/ingress.yaml` 的示例域名和 `my-ai-gateway-tls` 改成实际值。

不要把这些 Secret 写入 Git、ConfigMap、镜像层、Kustomize 生成文件或 `GATEWAY_CONFIG_JSON`。`GATEWAY_ADMIN_KEY` 只用于 `/admin/*`，不要与 Provider Key、Virtual Key 或过渡静态 Key 复用。`GATEWAY_CREDENTIAL_MASTER_KEY` 必须稳定保存，否则无法解密已保存的可恢复 Virtual Key。

## 准备 PostgreSQL

在数据库管理员连接下创建专用角色和数据库；下面只提供占位符：

```sql
CREATE ROLE my_ai_gateway LOGIN PASSWORD '<url-safe-password>';
CREATE DATABASE my_ai_gateway OWNER my_ai_gateway;
```

如果集群已经有 PostgreSQL Service，`DATABASE_URL` 使用集群 DNS，例如：

```text
postgres://my_ai_gateway:<url-safe-password>@postgresql.database.svc.cluster.local:5432/my_ai_gateway
```

Gateway 首次启动会执行仓库内置 migration。迁移和控制面数据备份、恢复见 [`docs/operations.md`](operations.md)。

## 创建 Secret

推荐在受限的本地目录准备两个临时 env 文件，并设置 `chmod 600`。文件只包含下面列出的键，不要提交：

```bash
chmod 600 control-plane.env provider.env

kubectl -n apps create secret generic my-ai-gateway-secret \
  --from-env-file=control-plane.env \
  --dry-run=client -o yaml | kubectl apply -f -

kubectl -n apps create secret generic my-ai-gateway-provider-secret \
  --from-env-file=provider.env \
  --dry-run=client -o yaml | kubectl apply -f -
```

`control-plane.env` 至少包含：

```dotenv
DATABASE_URL=postgres://my_ai_gateway:<url-safe-password>@<postgres-service>:5432/my_ai_gateway
GATEWAY_ADMIN_KEY=<admin-key>
GATEWAY_CREDENTIAL_MASTER_KEY=<stable-random-master-key>
```

`provider.env` 按实际控制面 Account 的 `credential_env` 放入对应键，例如：

```dotenv
GATEWAY_API_KEY=<optional-legacy-data-plane-key>
KIMI_API_KEY=<provider-key>
MINIMAX_API_KEY=<provider-key>
MINIMAX_API_KEY_2=<optional-provider-key>
DEEPSEEK_API_KEY=<provider-key>
SILICONFLOW_API_KEY=<optional-provider-key>
B_AI_API_KEY=<optional-provider-key>
```

对于私有 GHCR，使用短期 token 或组织规定的最小权限 token 创建拉取 Secret：

```bash
kubectl -n apps create secret docker-registry ghcr-my-ai-gateway \
  --docker-server=ghcr.io \
  --docker-username="$GHCR_USERNAME" \
  --docker-password="$GHCR_TOKEN" \
  --docker-email="$GHCR_EMAIL" \
  --dry-run=client -o yaml | kubectl apply -f -
```

不要在命令行中直接展开 token；执行完清理本地变量和临时文件。

## 应用清单

先检查并修改 [`deploy/kubernetes/ingress.yaml`](../deploy/kubernetes/ingress.yaml) 的域名和 TLS Secret，再渲染、校验和应用：

```bash
kubectl kustomize deploy/kubernetes > /tmp/my-ai-gateway.yaml
kubectl apply --dry-run=client -k deploy/kubernetes
kubectl apply -k deploy/kubernetes
kubectl -n apps rollout status deployment/my-ai-gateway --timeout=5m
```

生产环境建议在 GitOps 仓库中维护这个基线的 overlay，并把镜像从可变 tag 改为已审核的 digest。当前仓库只提供 `main` tag 作为参考，不把某次构建的 digest 固定在源仓库清单中。

## 控制面初始化

清单不会自动把 Provider、Account、模型或 Fallback Route 写入数据库。Gateway Ready 后，通过受控的 Admin API 完成以下步骤：

1. 创建或确认 Source、Account 及其 `credential_env`；
2. 执行模型发现并确认 SourceModel/Capability；
3. 显式创建 LogicalModel、ModelBinding 和 Route；
4. 用 `/admin/capabilities` 和 `/admin/routes` 检查 native/adapter、primary/fallback 以及不可路由状态；
5. 为下游客户端签发 PostgreSQL-backed Virtual Key。

开发阶段也可以在空控制面使用 `GATEWAY_CONFIG_JSON` 做一次性初始化，但不要把真实 JSON 或凭据写入 Kubernetes 清单。已有控制面需要替换时必须显式设置 `GATEWAY_CONFIG_IMPORT=true`，并先确认数据影响；完整契约见 [`docs/ai-gateway-design.md`](ai-gateway-design.md) 和 [`docs/admin-api.md`](admin-api.md)。

## 验证

```bash
kubectl -n apps get deployment my-ai-gateway
kubectl -n apps get pods -l app=my-ai-gateway -o wide
kubectl -n apps describe pod -l app=my-ai-gateway
kubectl -n apps exec deployment/my-ai-gateway -- curl -fsS http://127.0.0.1:8787/healthz
```

预期结果：Deployment 达到 `1/1`，Pod 为 `Ready` 且没有持续重启，`/healthz` 返回成功 JSON。若使用 Ingress，再从集群外验证 TLS、`/healthz`、三类协议入口和 SSE；反向代理必须保留 `text/event-stream`、关闭响应缓冲，并将读取/idle timeout 设为大于网关对应时限。

常见失败定位：

- `ImagePullBackOff`：检查 `ghcr-my-ai-gateway` 的 namespace、token 权限、镜像名和 tag/digest；
- `CrashLoopBackOff`：先看 `kubectl logs`，重点检查 `DATABASE_URL`、Admin Key 和凭据主密钥是否存在；
- Pod Ready 但请求失败：检查 PostgreSQL 连通性、控制面是否已创建可用 Binding/Route，以及 Ingress 的 host/TLS/timeout；
- 私有 Provider URL 被拒绝：按 [`docs/security.md`](security.md) 配置最小范围的 `GATEWAY_SOURCE_URL_ALLOWLIST`，不要通过客户端请求传入任意上游 URL。

## 升级与回滚

升级顺序：备份 PostgreSQL → 在 GitOps overlay 更新镜像 digest → 等待 rollout → 检查 `/healthz`、migration 和控制面能力矩阵。不要用 `latest` 作为生产回滚依据。

如果新镜像启动失败，先停止继续发布并保留 Pod 日志；在 GitOps 仓库把镜像恢复到上一个已验证 digest 后同步。直接执行 `kubectl rollout undo` 只适合临时止血，最终仍应把期望状态写回 Git，避免 Argo CD 或其他控制器再次改回。

## 部署 checklist

- [ ] PostgreSQL 专用数据库、角色和备份策略已准备；
- [ ] `my-ai-gateway-secret`、`my-ai-gateway-provider-secret` 和 `ghcr-my-ai-gateway` 在正确 namespace；
- [ ] Secret 未提交 Git，凭据主密钥已稳定备份；
- [ ] Ingress 域名、TLS Secret 和 SSE 超时已确认；
- [ ] 镜像使用已审核 tag/digest，且架构与 K3s 节点匹配；
- [ ] migration、`/healthz`、Admin API 鉴权和一次真实路由已验证；
- [ ] 升级前已完成 PostgreSQL 备份，回滚 digest 已记录。
