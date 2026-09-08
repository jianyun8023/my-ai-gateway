# 前端架构与治理

控制台是 React + TypeScript + Vite 应用，通过 `/admin/` 提供八个导航入口。本文约定代码职责、共享能力与检查门禁；领域与协议基线仍以 [设计文档](ai-gateway-design.md) 为准，视觉与交互遵循 [design.md](../design.md)。

本轮沿用后端治理的分层、共享能力收敛、死代码清理和门禁方式，实现位于 `codex/frontend-architecture-governance`。起点为 2026-09-08 的 `main` `de708e8`，完成后整合已合并后端治理的 `main` `efeb445`。

关联任务：[前端分层、公共能力与死代码治理 #167](https://github.com/jianyun8023/my-ai-gateway/issues/167)。与 Mantine 全局 UI 治理 #166 的衔接见下文；CI 和合并状态以 GitHub 实时记录为准。

## 分层与调用链

| 层 | 入口与职责 |
| --- | --- |
| 应用装配 | `main.tsx` 初始化与挂载；`Root.tsx` 同步主题；`App.tsx` 接入连接状态、导航和页面 |
| 导航与页面 | `lib/consoleNavigation.ts` 定义八个 hash 入口；`pages/` 组合功能与刷新状态 |
| 业务功能 | `features/usage/` 组织筛选、总览、分析、事件与详情；`features/control-plane/` 组织来源、发现、模型路由、能力矩阵与设置 |
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
```

页面负责组合，表单、详情和局部展示放到所属功能目录。来源与账号、模型发现与确认、逻辑模型与 Binding/Route 的职责保持分离；拆组件不能改变提交字段或能力判断。

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

`useUsageData` 为每轮查询持有 AbortController、有效筛选与游标。切换筛选、页面、粒度或刷新时取消旧查询；即使底层在取消后返回，也不发布旧结果。分页在请求发起前同步加锁，按事件 ID 去重，避免滚动回调重复请求同一游标。`useAdminQuery` 为控制面和事件详情复用取消、加载、失败与重试流程。

## 组件与布局

事件详情复用 `Modal` drawer，统一关闭、Escape、焦点约束与恢复。宽表通过 `TableScroll` 或事件列表自己的滚动容器滚动；控制台 flex / grid 子项必须允许收缩，不能让表格撑宽整页。日期预设在窄屏换行。

删除没有生产调用方的旧组件、图标、路由辅助函数与独占样式。测试夹具统一放在 `src/test/fixtures/`，不混入生产数据层。保留 CPA Usage Keeper 的 MIT 许可与来源说明。

## 与 Mantine 全局治理 #166 的衔接

2026-09-08 后续实施：#166 已从 `main c99455e` 开始，首批接入 Mantine 9.6.0 的 Provider、Modal/Drawer、移动导航和列偏好 Popover。以下表格保留 #168 交接时的历史范围；当前组件职责及逐页证据以 [design.md](../design.md) 与 [迁移清单](mantine-migration.md) 为准。首批不代表 #166 全页面治理与验收完成。

[#166](https://github.com/jianyun8023/my-ai-gateway/issues/166) 负责采用 Mantine 统一八个页面的设计系统；本轮架构治理不改变这项决策，也不完成其全页面 UI 迁移与验收。该 Issue 的调查链接固定在旧提交，后续清单应按本轮的新入口更新。

以下路径相对于 `web/src/`：

| #166 涉及范围 | 本轮影响 | 后续接入点与剩余工作 |
| --- | --- | --- |
| 应用入口与主题 | 主题初始化组件从 `main.tsx` 拆到 `Root.tsx`；全局样式加载顺序保持不变 | 在新入口装配 MantineProvider；统一 `useThemeStore`、CSS 变量、Portal 与图表主题，不能假定本轮已经完成主题迁移 |
| 事件详情 | 从 `pages/GatewayUsagePage.tsx` 移到 `features/usage/UsageEventDetails.tsx`，改为复用公共 `Modal` drawer | 与控制面一同迁移公共 Modal/Drawer；当前公共组件仍保留手写焦点、滚动锁和动画逻辑 |
| 控制面表单与详情 | 拆入 `features/control-plane/sources/`、`models/`、`discovery/` 与 `VirtualKeyForm.tsx` | 对新文件迁移控件；来源、实体和能力详情仍有外部关闭计时器，需要随 Mantine 生命周期一并收敛 |
| 列设置、图表与虚拟列表 | 分别位于 `features/usage/UsageEvents.tsx`、`UsageOverview.tsx`、`UsageAnalysis.tsx`、`charts.ts` | 列设置仍是原生 `details`；Popover、图表主题与虚拟列表布局验收仍由 #166 完成 |
| 旧公共组件 | 删除无生产调用的 Input、Select、MainActionButton、PortalTooltip、QuestionMarkHelp、QuestionMarkHelpButton 及独占样式 | 从“待迁移”清单移除这些遗留实现；活跃 FormField、Button、IconButton 等仍需接入体系 |
| 应用壳与时间筛选 | 修正主内容收缩约束；八个实际 hash 入口不变；新增今天/昨天并固定同轮查询窗口 | 保留表格内部滚动、自然日/滚动时间与查询取消契约；移动侧栏的手写浮层行为仍需迁移和组合验收 |
| 门禁与依赖 | 新增开发依赖及分层、Fast Refresh、测试类型和 Knip 检查；没有引入运行时 UI 库 | Mantine 外部导入不被分层规则禁止；共享主题与组合组件应放在公共层，新增示例须有实际入口，移除被替代实现后通过门禁 |

两个任务会共同修改 `main.tsx`、`Root.tsx`、`components/ui/`、页面样式、`package.json` / 锁文件和 `design.md`。建议 #166 从本轮 PR 合并后的主线开始实现；已开始的分支先整合本轮提交，再按新路径迁移，避免重新建立旧页面内实现。依赖合并应保留两边所需的依赖与检查脚本，并通过 npm 重新生成一致的锁文件。

本轮的桌面/窄屏局部检查不能替代 #166 要求的八页面、双主题、嵌套浮层、滚动与性能验收；本轮 PR 只关联 #166，不关闭它。

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
