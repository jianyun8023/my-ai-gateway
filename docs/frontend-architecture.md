# 前端架构与治理

控制台是 React + TypeScript + Vite 应用，通过 `/admin/` 提供九个导航入口。本文约定代码职责、共享能力与检查门禁；领域与协议基线仍以 [设计文档](ai-gateway-design.md) 为准，视觉与交互遵循 [design.md](../design.md)。

本轮沿用后端治理的分层、共享能力收敛、死代码清理和门禁方式，实现位于 `codex/frontend-architecture-governance`。起点为 2026-09-08 的 `main` `de708e8`，完成后整合已合并后端治理的 `main` `efeb445`。

关联任务：[前端分层、公共能力与死代码治理 #167](https://github.com/jianyun8023/my-ai-gateway/issues/167)。与 Mantine 全局 UI 治理 #166 的衔接见下文；CI 和合并状态以 GitHub 实时记录为准。

## 分层与调用链

| 层 | 入口与职责 |
| --- | --- |
| 应用装配 | `main.tsx` 初始化与挂载；`Root.tsx` 同步主题；`App.tsx` 接入连接状态、导航和页面 |
| 导航与页面 | `lib/consoleNavigation.ts` 定义九个 hash 入口；`pages/` 组合功能与刷新状态 |
| 业务功能 | `features/usage/` 组织筛选、总览、分析、请求事件与详情；`features/events/` 组织统一运行事件时间线；`features/control-plane/` 组织来源、发现、模型路由、能力矩阵与设置 |
| 资源与数据 | `admin-api/resources.ts` 封装控制面资源；`gateway-usage/` 封装用量查询、过滤、游标合并及响应适配 |
| HTTP 传输 | `admin-api/client.ts` 是管理请求的统一入口；`admin-api/errors.ts` 提供错误归一化 |
| 共享能力 | `hooks/` 提供查询生命周期；`lib/` 管理导航、协议与偏好存储；`utils/` 提供格式化和下载 |
| 视图基础 | `components/ui/` 提供通用表单、状态、表格和弹窗；`components/gateway/` 提供控制台外壳 |

两条主要读取链路：

```text
App → GatewayUsagePage → features/usage → GatewayUsageClient
    → AdminClient → /admin/usage/* → adapter → ViewModel → 视图

App → GatewayManagementPage → features/control-plane → useAdminQuery / 资源操作
    → AdminApi 资源 → AdminClient → /admin/*

App → GatewayManagementPage → features/events → useAdminQuery / 游标合并
    → GatewayAdminResources → AdminClient → /admin/events
```

页面负责组合，表单、详情和局部展示放到所属功能目录。来源与账号（来源列表/详情/编辑）、模型更新审核、逻辑模型与 Binding/Route 的职责保持分离；拆组件不能改变提交字段或能力判断。

模型与路由按 [#195 V3](design/model-routing-ui/implementation-v3.md) 以逻辑模型为唯一列表对象。`routingPresentation` 将配置与运行时能力聚合为线路、协议和健康摘要，`ModelRoutePath` 展示请求路径；不同协议的实际顺序不同时保留差异，旧加权备用池不能显示为确定顺序。模型列表同时读取请求设置；当重试上限限制回退时，显示最多尝试的可用线路数并省去无条件的失败箭头，保留全部候选线路以表达冷却跳过不消耗次数。`ModelRoutingEditor` 用现有字段与抽屉维护有序线路，通过 `GatewayAdminResources.createModelRouting` 创建或 `saveModelRouting` 更新，一次保存到模型级事务接口，不在浏览器顺序调用多种资源写接口。新增模型 ID 由服务端生成，不用公开名称命中更新路径。Binding / Route 的领域职责和 Admin API 保留，独立 CRUD 表单与详情页已移除；实现信息仅在抽屉底部 Accordion 只读展示。

`features` 不反向依赖页面，也不跨功能引用内部实现。`admin-api`、`gateway-usage`、`lib`、`utils`、`i18n` 不依赖 UI 层。通用 UI 不依赖业务 API，跨页面共享逻辑放到下层模块。纯数据与格式化函数不混在 React 组件文件中，以保持 Fast Refresh 边界。

## Admin 请求与错误

- `AdminClient` 同时支持 JSON 与 Blob，统一动态读取当前 Admin Key、构建鉴权头、禁止缓存、拒绝重定向、处理结构化错误与取消信号。
- 请求路径限制在同源 `/admin/` 下。调用方不能通过自带 Authorization 覆盖当前连接身份，不能另建页面级 `fetch` 或 XMLHttpRequest 通道。
- `GatewayUsageClient` 只负责端点与数据适配；事件详情在适配层转成 attempt ViewModel，视图不解析原始 JSON。
- Admin Key 仍由连接模块保存在当前标签页的 `sessionStorage`。非敏感筛选与列偏好使用安全的本地存储辅助函数；存储不可用时当前页面操作仍可继续。
- 查询错误归一化后由页面选择文案；取消请求不显示错误。详情失败明确展示重试，不伪装成“没有 attempt”。

## 时间范围与查询生命周期

没有有效的已保存筛选时，默认选中“今天”。支持“昨天”“最近 24 小时”“最近 7 天”“最近 30 天”和自定义起止时间。

| 选项 | 查询范围 |
| --- | --- |
| 今天 | 浏览器本地当天零点到下一天零点 |
| 昨天 | 浏览器本地前一天零点到当天零点 |
| 最近 24 小时 / 7 天 / 30 天 | 查询发起时刻向前滚动相应时长 |
| 自定义 | 用户指定的本地起止时间 |

请求将时间转成 UTC ISO 字符串，服务端按 `[from, to)` 查询。自然日分别计算两个零点，支持夏令时切换日的 23 / 25 小时。自定义范围必须有效且结束晚于开始；空值或损坏的持久化内容不会导致页面崩溃。

相对时间只保存预设，不保存过期的绝对窗口。每轮首次查询、刷新或重试重新解析时间，同一轮的统计、分页和导出使用固定窗口，避免滚动时间导致页面间边界漂移。页面跨日后在刷新时切换自然日窗口。

`useUsageData` 为每轮查询持有 AbortController、有效筛选与游标。切换筛选、页面、粒度或刷新时取消旧查询；即使底层在取消后返回，也不发布旧结果。分页在请求发起前同步加锁，按事件 ID 去重，避免滚动回调重复请求同一游标。`useAdminQuery` 为控制面、运行事件首屏和事件详情复用取消、加载、失败与重试流程；运行事件的后续页另外绑定首屏响应身份、取消旧请求并按 namespaced `event_id` 去重，筛选切换后不会拼入旧页。

## 组件与布局

用量筛选与运行事件类型复用 `RemoteFilterField`（#193）。该通用组件只接收异步候选加载函数，不依赖业务 API；端点与时间范围由 `GatewayUsageClient` / `GatewayAdminResources` 和功能组件提供。展开时按需请求、搜索防抖 250ms，关闭/卸载或变更搜索/时间/身份时取消旧请求；响应发布前检查取消信号，渲染时按查询上下文隐藏旧候选。输入与候选数据独立，失败可重试或手输，清空代表全部，选择只修改草稿。候选不写入持久化存储。Mantine Combobox 负责 Portal、键盘选项导航和滚动，固定枚举继续使用 `SelectField`。

请求事件与运行事件详情复用 `Modal` drawer，统一关闭、Escape、焦点约束与恢复。宽表通过 `TableScroll` 或事件列表自己的滚动容器滚动；控制台 flex / grid 子项必须允许收缩，不能让表格撑宽整页。日期预设在窄屏换行。

删除没有生产调用方的旧组件、图标、路由辅助函数与独占样式。测试夹具统一放在 `src/test/fixtures/`，不混入生产数据层。保留 CPA Usage Keeper 的 MIT 许可与来源说明。

## 与 Mantine 全局治理 #166 的衔接

截至 2026-09-09，[#166](https://github.com/jianyun8023/my-ai-gateway/issues/166) 的 54 项原始范围与验收项已完成并关闭，PR [#169](https://github.com/jianyun8023/my-ai-gateway/pull/169)–[#182](https://github.com/jianyun8023/my-ai-gateway/pull/182) 均已合并。当前组件职责与规则以 [design.md](../design.md) 和本文为准；逐批迁移、构建体积、实际 App + 合成 API 浏览器矩阵、截图及未执行边界保留在 [迁移清单](mantine-migration.md)。

最终证据索引：

- PR #169–#173：主题、浮层、字段、筛选、反馈与图表基础；
- PR #174–#179：控制面表格、虚拟请求表、应用壳、通知、KPI 与页面组合；
- PR #180–#182：[查询生命周期](mantine-migration.md#第十二批页面状态与查询生命周期验收)、[最终组件/交互审计](mantine-migration.md#第十三批最终组件审计与适用交互收尾)与[逐页业务状态闭环](mantine-migration.md#第十四批逐页业务状态证据闭环)；
- 54 项的最终归并依据见[原始未勾项映射](mantine-migration.md#对-166-原始未勾项的最终映射)。

#166 覆盖当时的八个页面。#110 后续新增的第九个“运行事件”入口复用相同公共组件、布局和查询取消约束，但必须使用自己的测试与浏览器记录验收；不能把 #166 的八页证据外推为第九页证据，也不能外推为真实 Provider、生产配置、屏幕阅读器人工朗读或帧耗时基准。

PR #168 交接时关于 MantineProvider、公共 Modal/Drawer、字段、Popover、图表、虚拟列表和旧样式的“后续接入”描述只是首批历史快照，相关代码迁移与适用交互已经由上述后续批次收敛，不再作为当前待办。新增页面继续从 `components/ui` 与既有功能组合复用，不恢复已删除的手写 Portal、浮层、字段或重复主题状态。

## 持续检查

在仓库根目录使用 `mise.toml` 锁定的 Node 版本：

```bash
mise exec -- npm --prefix web run lint
mise exec -- npm --prefix web run typecheck
mise exec -- npm --prefix web test
mise exec -- npm --prefix web run build
TZ=America/New_York mise exec -- npm --prefix web test -- src/gateway-usage/filterState.test.ts
```

- ESLint 对未使用符号、React Hooks 和 Fast Refresh 报错；本地架构规则检查 alias、相对路径与静态动态导入的分层约束，网络规则约束统一传输入口。
- `lint` 包含 `check:dead-code`：Knip 全量检查未用文件、依赖及导出等问题，再以生产入口检查文件与依赖，识别“只有测试引用”的遗留实现。Knip 配置包含 SCSS，以检查孤立样式文件；不扫描单个 CSS 选择器或类方法的可达性。
- `typecheck` 分别覆盖应用和测试代码，开启未使用局部变量、参数与 switch fallthrough 检查。
- 既有 `mise run lint`、`mise run test` 与 PR CI 已调用这些 Web 脚本，无需第二套门禁入口。

2026-09-08 本轮本地验证记录：

| 验证 | 结果 |
| --- | --- |
| 应用与测试 TypeScript 检查 | 通过 |
| ESLint、Knip 全量与生产入口检查 | 通过，无 warning |
| Web Vitest | 29 个文件，136 项通过，0 失败、0 跳过 |
| 纽约时区时间筛选回归 | 11 项通过，覆盖自然日 23 / 25 小时边界 |
| Web 生产构建 | 通过 |
| 浏览器 | 生产构建与本地 Mock Admin API：日期切换、来源表单、事件详情及 Escape 关闭、390px 窄屏筛选换行与表格内部滚动；控制台无 warning / error |

源码复核确认 11 个拆出的控制面表单与展示组件实现保持一致；来源详情的预设比较改为共享查询生命周期。记录只描述这次执行结果，后续变更应重新选择适用检查。

这些检查不等于真实 Provider、PostgreSQL 集成或部署验收。本轮未改 Rust、HTTP API 或数据库 Schema，未运行真实模型请求与生产复验。
