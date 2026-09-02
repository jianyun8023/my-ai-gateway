# my-ai-gateway Kubernetes 清单

这里是当前仓库提供的脱敏 Kustomize 基线，适用于 Kubernetes 和 K3s。清单只部署 Gateway，不创建 PostgreSQL；运行时控制面必须连接已有的 PostgreSQL 16 或托管 PostgreSQL。

应用依赖以下外部 Secret，清单不会创建或覆盖它们：

- `my-ai-gateway-secret`：`DATABASE_URL`、`GATEWAY_ADMIN_KEY`、`GATEWAY_CREDENTIAL_MASTER_KEY`；
- `my-ai-gateway-provider-secret`：Provider 凭据和可选的过渡 `GATEWAY_API_KEY`；
- `ghcr-my-ai-gateway`：访问私有 GHCR 包的 `docker-registry` Secret。

先按 [`docs/kubernetes.md`](../../docs/kubernetes.md) 准备数据库、Secret 和 Ingress 的域名/TLS，再执行：

```bash
kubectl apply -k deploy/kubernetes
kubectl -n apps rollout status deployment/my-ai-gateway
kubectl -n apps get service,pod,ingress -l app=my-ai-gateway
```

默认镜像为 `ghcr.io/jianyun8023/my-ai-gateway:main`。生产环境应在受控 overlay 或 Argo CD 配置中改为经过审核的 digest；不要把凭据、`DATABASE_URL` 或真实域名写回本目录。
