# Mantine 控制台迁移清单（#166）

基线：2026-09-08，`main c99455e`（PR #168 已合并）。实施分支：`codex/166-mantine-console`。关联 [Issue #166](https://github.com/jianyun8023/my-ai-gateway/issues/166)，本清单随每批实现更新；未完成全部验收前不关闭总任务。

Issue 中 `de708e8` 的链接是历史调查依据。当前事件详情已位于 `features/usage/UsageEventDetails.tsx` 并复用公共 Modal；旧 Select、PortalTooltip、QuestionMarkHelp 等无调用实现已在 #168 删除，不再列为线上迁移对象。

## 职责与批次

1. Mantine 基础组件负责通用控件、Portal、焦点、滚动锁、定位和动画。
2. `components/ui` 负责品牌主题对接及确有项目契约的组合：Modal/Drawer 的统一标题、底部操作区、关闭禁用和退出回调；字段的 label/hint/error 关联；主次操作和状态语义。无需契约的 Mantine 组件从公共入口直接导出，不机械封装。
3. `features` 负责业务数据、字段、校验、提交、筛选和领域展示；不重新实现浮层基础交互。

首批：完整清单、主题接入、公共 Modal/Drawer、移动导航、事件列设置、退出生命周期与组合回归。后续批次继续控件、反馈、表格、图表和八页完整验收。本清单中的“接入”表示受公共基础覆盖，不等于该页完成验收。

## 页面与流程覆盖

路径相对于 `web/src/`。PR 列在外部写入获得授权后填写；本轮仅本地实施。

| 页面/流程 | 当前组件与实际入口 | 目标与保留项 | 迁移批次/PR | 验证证据与剩余工作 |
| --- | --- | --- | --- | --- |
| 应用壳 | `components/gateway/GatewayConsoleShell`：桌面侧栏、手写移动遮罩、连接、主题、语言、刷新 | 桌面导航保留 hash 语义；移动导航使用公共 Drawer；主题状态唯一 | 首批，本地 | 验证关闭/返回焦点、窄屏和双主题；后续收敛工具栏控件 |
| 总览 | `features/usage/UsageOverview`、`UsageFilters`、`charts.ts`：时间/模型过滤、KPI、Chart.js | 保留时间/Token 语义；字段、图例、轴与 canvas 主题接入 | 全局 Provider 首批；页面后续 | 后续验证有数据、空态、部分失败、刷新、主题和图表尺寸 |
| 用量分析 | `UsageAnalysis`、`UsageFilters`：趋势、分布、价格与导出 | 保留 Chart.js 与格式化工具；统一图表容器和表格 | 全局 Provider 首批；页面后续 | 后续验证筛选/导出、missing 与 unknown、长文本 |
| 请求事件 | `UsageEvents`、`UsageEventDetails`：TanStack Virtual、列设置、详情/重试、导出 | details → Popover；详情 → Mantine Drawer；保留虚拟化与查询取消 | 浮层首批；列表后续 | 首批列切换、详情关闭；后续密集数据滚动、刷新与列测量 |
| 来源 | `SourcesPage`、`sources/SourceForm`、`AccountForm`、`SourceDetailDrawer` | 编辑/确认 → Modal，详情 → Drawer；保留接入、预设比较、连接测试 | 浮层首批；控件后续 | 首批代表性编辑/确认/详情关闭；后续表单全状态、实际提交与连接测试 |
| 模型发现 | `ModelDiscoveryPage`、`discovery/*`：发现、筛选、批量选择、确认、元数据/能力编辑 | 编辑与确认使用 Modal；保留确认状态与能力语义 | 浮层首批；控件后续 | 后续发现/确认完整流程、禁用/失败/选择保持 |
| 模型与路由 | `ModelsRoutesPage`、`models/*`：三类实体表格、编辑、详情、确认 | Modal/Drawer；保留 Binding、Route、逻辑模型区别 | 浮层首批；控件后续 | 后续三类实体各自核心流程、长 ID 与宽表 |
| 能力矩阵 | `CapabilitiesPage`：筛选、三协议矩阵、详情 | 详情 → Drawer；保留 unknown/unsupported/degraded | 浮层首批；控件后续 | 后续多协议状态、过滤空态、详情长内容 |
| 设置 | `SettingsPage`、`VirtualKeyForm`、`VirtualKeyRotationForm`：Key、导入/导出、运行信息 | Modal/确认；敏感值关闭立即清除，不为动画延长保存 | 浮层首批；控件后续 | 保留已有轮换测试；后续读取/复制/轮换/撤销和导入导出完整验收 |

## 组件处置清单

| 类别 | 处置 |
| --- | --- |
| 活跃 Modal | 首批替换其手写焦点、滚动、计时和动画；来源/实体/能力详情的 380ms 外部计时改用 Mantine 退出回调 |
| 移动侧栏 | 首批替换遮罩、手写 body overflow 和 Esc/焦点计时器；桌面导航继续使用同一内容 |
| 活跃列设置 details | 首批迁移 Popover；复选项使用 Mantine Checkbox |
| Button、IconButton、FormField、SegmentedTabs、LanguageSwitcher | 保留调用契约，后续逐类改用 Mantine；原生 select 仍在页面/字段中，首批不宣称全部下拉完成 |
| Notice、LoadingState、LoadingSpinner、EmptyState、StatusPill | 后续统一通知/持久错误、加载/空态/状态色；保留部分失败与重试信息 |
| Card、TableScroll、FormGrid、FilterBar、PageActions、DrawerSection | 保留有价值的页面组合；后续去除重复控件样式，公共布局归入 UI 层 |
| Chart.js、TanStack Virtual、格式工具 | 保留专业实现；后续统一主题、容器、数值/缺失值展示并验证性能 |
| 已删除无调用组件 | #168 已清理旧 Input、Select、MainActionButton、PortalTooltip、QuestionMarkHelp、QuestionMarkHelpButton，不重新引入 |

## 基线与验证记录

基线浏览器为本地 Vite、无后端连接（错误态）；不能据此声称复现了生产遮挡或跳顶。代表性截图：[桌面浅色](evidence/166/before-desktop-light.png)、[390px 深色侧栏](evidence/166/before-mobile-dark.png)。源码已确认关闭的移动侧栏仍留在可访问树；后续以 Drawer 关闭后卸载验证。

基线构建使用 `mise exec -- npm --prefix web run build`，Node 24.11.1，Vite 8.0.16：JS 合计 673.79 kB（gzip 205.11 kB），CSS 合计 86.20 kB（gzip 16.31 kB）；主入口 JS 309.35 kB（gzip 97.45 kB）。按相同命令记录首批变化。

最终验证命令、浏览器结果与未覆盖项在本批实现完成后补充。DOM 回归只证明组件契约，定位、滚动、焦点恢复和动画须由真实浏览器补充。八页均需浅色/深色、桌面/窄屏、至少一条核心流程与适用的加载/错误/空态证据；清单未填项继续由 #166 跟踪。
