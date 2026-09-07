# AI Gateway 控制台

React + TypeScript + Vite 应用，构建后由网关挂载在 `/admin/`。

- **监控**：总览、用量分析、请求事件。
- **配置**：来源管理、模型发现、模型与路由、能力矩阵。
- **系统**：系统设置与 Virtual Key 管理。

## 开发

在仓库根目录运行，工具版本由 `mise.toml` 固定：

```bash
mise run install
mise run dev
mise exec -- npm --prefix web run lint
mise exec -- npm --prefix web run typecheck
mise exec -- npm --prefix web test
mise exec -- npm --prefix web run build
```

## 代码入口

| 路径 | 职责 |
| --- | --- |
| `src/App.tsx`、`src/lib/consoleNavigation.ts` | 页面与 hash 导航 |
| `src/components/gateway`、`src/components/ui` | 控制台外壳与公共组件 |
| `src/pages/GatewayUsagePage.tsx`、`src/gateway-usage` | 用量页面、API 适配与测试夹具 |
| `src/features/control-plane`、`src/admin-api` | 管理页面与资源 API |
| `src/i18n/console` | 中英文文案 |

用量页面请求 `/admin/usage/*`，管理页面请求对应 `/admin/*` 资源。Admin Key 保存在当前标签页的 `sessionStorage`，用于 Admin API 鉴权。

系统设置支持 Virtual Key 创建、查看/复制、轮换与撤销。轮换时可调整新密钥的模型白名单，并为客户端切换设置最多 24 小时的重叠期；旧密钥原有的到期时间仍然生效。

设计与组件规则见 [design.md](../design.md)，接口见 [Admin API](../docs/admin-api.md)，测试见 [testing.md](../docs/testing.md)。CPA Usage Keeper 的 MIT 来源与许可保留在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
