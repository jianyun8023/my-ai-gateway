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
| `src/pages/`、`src/features/usage/` | 页面组合、用量筛选、图表、事件与查询生命周期 |
| `src/features/control-plane/` | 管理页面及所属表单、详情、元数据编辑 |
| `src/admin-api/`、`src/gateway-usage/` | 共享 Admin 传输、资源 API、用量适配与筛选模型 |
| `src/hooks/`、`src/lib/`、`src/utils/` | 共享查询、协议、偏好存储、格式化与下载 |
| `src/test/fixtures/` | 仅供测试使用的固定数据 |
| `src/i18n/console` | 中英文文案 |

用量页面请求 `/admin/usage/*`，管理页面请求对应 `/admin/*` 资源。Admin Key 保存在当前标签页的 `sessionStorage`，用于 Admin API 鉴权。

用量时间范围默认“今天”，也可以选择“昨天”、最近 24 小时 / 7 天 / 30 天或自定义。今天与昨天按浏览器本地自然日计算，查询转为 UTC；刷新时重新计算相对范围，同轮分页与导出保持同一窗口。有效的已保存筛选会在下次打开时恢复。

`lint` 同时执行 ESLint 分层与网络边界规则、Knip 全量和生产入口死代码检查；可单独运行 `mise exec -- npm --prefix web run check:dead-code`。`typecheck` 覆盖应用与测试代码。

系统设置支持 Virtual Key 创建、查看/复制、轮换与撤销。轮换时可调整新密钥的模型白名单，并为客户端切换设置最多 24 小时的重叠期；旧密钥原有的到期时间仍然生效。

分层、调用链与治理规则见 [前端架构](../docs/frontend-architecture.md)，设计与组件规则见 [design.md](../design.md)，接口见 [Admin API](../docs/admin-api.md)，测试见 [testing.md](../docs/testing.md)。CPA Usage Keeper 的 MIT 来源与许可保留在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
