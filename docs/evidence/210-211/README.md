# 请求事件用量展示（#210 / #211）

核对日期：2026-09-14。实现基于最新 `main` `2a2c371`，对应 [Issue #210](https://github.com/jianyun8023/my-ai-gateway/issues/210) 与 [Issue #211](https://github.com/jianyun8023/my-ai-gateway/issues/211)，同一 PR 交付。

## 修改内容

| 验收点 | 实现 |
| --- | --- |
| `usage_source` 不再仅显示“解析” | 完整文案改为“上游返回（完整响应）/ 上游返回（流式响应）/ 本地估算 / 未获取”，Badge 短文案为“上游 / 上游流式 / 估算 / 未获取” |
| Tooltip 说明 SSE 合并语义 | Badge 悬浮说明区分数据可信来源与采集方式，明确 `parsed` 来自上游 SSE `usage` 字段按事件合并，不是本地估算 |
| 列表按行展示 Token 与缓存 | 事件表新增 Tokens / Cache 两列，数值右对齐，大数统一 K / M / B，表头附说明图标 |
| 悬浮查看完整分解 | Token 悬浮展示输入、输出、推理、缓存读取、缓存创建、总计与用量来源；缓存悬浮展示命中率、读取、创建与计算说明 |
| 命中率口径 | `cache_read_tokens / input_tokens`，`input <= 0` 显示 `—`，缓存创建不计入命中，保留 1 位小数并截断到 100% |
| 详情抽屉紧凑行式布局 | 按基本信息 / 路由 / Token / 缓存分组为 label-value 行，替代独立小卡片 |
| 上游尝试压缩 | 单次尝试压缩为一行，Retry / Fallback 时逐 attempt 展开 |
| i18n 同步 | 中英文文案同步调整，展示层口径一致 |

## 验证

- `mise exec -- npm --prefix web run lint`：通过，包含 ESLint 和 Knip。
- `mise exec -- npm --prefix web run typecheck`：通过。
- `mise exec -- npm --prefix web test`：38 个测试文件、276 项测试通过，含新增 `usageQuality` 命中率边界、`UsageBadge` 悬浮说明与事件表列测试。
- `mise exec -- npm --prefix web run build`：通过；Vite 仍提示部分 bundle 大于 500 kB。
- 浏览器连接只读模拟 API（[visual-fixture.mjs](visual-fixture.mjs)，覆盖各类 `usage_source`、缓存命中率与单/多次尝试），验证 1800px 桌面浅色 / 深色 / 英文：事件表两列与悬浮明细、详情抽屉单次压缩与回退展开均符合预期。

本地验证范围为前端展示层。未修改 `usage_source` 枚举、持久化结构、API 契约与 Token 采集口径；模拟数据不代表真实 Provider 行为，未执行真实 Provider 或生产验收。

## 视觉证据

| 场景 | 截图 |
| --- | --- |
| 事件表，中文浅色 | [events-desktop-light.png](events-desktop-light.png) |
| 事件表，中文深色 | [events-desktop-dark.png](events-desktop-dark.png) |
| 事件表，英文浅色 | [events-desktop-en.png](events-desktop-en.png) |
| Token 悬浮明细 | [events-token-hover.png](events-token-hover.png) |
| 缓存悬浮明细 | [events-cache-hover.png](events-cache-hover.png) |
| 详情抽屉，单次尝试 | [event-drawer-single.png](event-drawer-single.png) |
| 详情抽屉，缓存与尝试区 | [event-drawer-single-bottom.png](event-drawer-single-bottom.png) |
| 详情抽屉，回退多次尝试 | [event-drawer-fallback.png](event-drawer-fallback.png) |
