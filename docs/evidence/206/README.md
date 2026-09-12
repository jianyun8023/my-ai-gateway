# #206 模型有效能力详情验证

关联 [Issue #206](https://github.com/jianyun8023/my-ai-gateway/issues/206)。2026-09-12 在 `codex/206-model-capabilities` 上完成，基于 `main` `82b0953`。

## 本地检查

在仓库根目录执行：

| 命令 | 结果 |
| --- | --- |
| `mise run config-check` | 通过 |
| `mise exec -- npm --prefix web run lint` | ESLint、knip（常规及 production）通过 |
| `mise exec -- npm --prefix web run typecheck` | 应用及测试 TypeScript 检查通过 |
| `mise exec -- npm --prefix web test` | 36 个测试文件、257 项测试通过 |
| `mise exec -- npm --prefix web run build` | 通过；Vite 提示主包超过 500 kB |
| `git diff --check` | 通过 |

新增/调整的行为回归覆盖：移除独立导航与旧 hash；协议入口打开只读详情、关闭后恢复焦点且不产生写请求；模型级入口默认选择可用协议；三个独立协议路由的占位单元格过滤；实际不可路由和缺失快照；附于其他路由行的协议解析错误去重且不误归因账号；禁用、待确认、冷却线路；协议间主备顺序及转换/降级信息。已有多线路、加权策略及重试限制回归继续通过。

## Chrome 界面检查

使用本地只读合成 API，桌面视口 1468 × 902，窄屏视口 390 × 844。检查模型列表、长模型名、协议切换、抽屉关闭、有效功能表、展开的运行时诊断，以及原生、转换降级、待确认状态。窄屏页面无横向溢出；模型表格保留区域内横向滚动，协议入口至少 44 px 高。检查后恢复浏览器视口。

| 截图 | 内容 |
| --- | --- |
| [模型列表：桌面](models-desktop.png) | 七项导航、紧凑单线路与协议差异 |
| [能力详情：桌面](capabilities-desktop.png) | Responses 实际线路与八项功能能力 |
| [能力详情：降级](capabilities-degraded.png) | Responses 主备顺序与模拟转换/Thinking 降级 |
| [能力详情：待确认](capabilities-pending.png) | 未发布线路不推断有效能力 |
| [模型列表：窄屏](models-mobile.png) | 长模型名、协议按钮与表格区域滚动 |
| [能力详情：窄屏](capabilities-mobile.png) | 单列能力项与可见关闭按钮 |
| [运行时诊断：窄屏](diagnostics-mobile.png) | ID、端点、转换链换行 |

复现界面数据：

```sh
mise exec -- node docs/evidence/206/visual-fixture.mjs
```

在另一个终端启动前端，再通过 Chrome 打开 `http://127.0.0.1:5176/#models`：

```sh
VITE_API_PROXY_TARGET=http://127.0.0.1:8796 mise exec -- npm --prefix web run dev -- --host 127.0.0.1 --port 5176
```

使用默认同源代理地址即可，fixture 不校验 Admin Key。API 仅监听回环地址并对非 GET 请求返回 405。示例域名、账号、能力与 `synthetic-responses-to-messages` Adapter 均为合成数据；模型名称只用于布局检查，不代表这些 Provider 的实际能力。当前生产 Adapter 注册表仍为空。

## 验证边界

本次只改前端及文档，保留 `/admin/capabilities` 和后端路由/协议契约。未在本地重跑 Rust、PostgreSQL 或真实 Provider 请求；完整 PR CI 结果以关联 PR 的 Checks 为准。未修改生产配置或执行部署，截图不作为真实 Provider、生产性能或部署验收证据。
