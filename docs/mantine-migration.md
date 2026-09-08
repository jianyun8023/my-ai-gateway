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
| 应用壳 | `components/gateway/GatewayConsoleShell`：桌面侧栏、移动导航、连接、主题、语言、刷新 | 桌面导航保留 hash 语义；移动导航使用公共 Drawer；主题状态唯一 | 首批，本地 | 已验证关闭/返回焦点、窄屏和双主题；后续收敛工具栏控件 |
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

### 首批结果（2026-09-08）

接入 Mantine 9.6.0，主题直接读取既有 `useThemeStore` 与品牌 CSS 变量。公共浮层替换手写焦点、滚动锁、Portal 和动画，Modal/Drawer 共用堆叠上下文。非敏感表单在退出完成后清理，敏感值仍关闭即清除。详情切换编辑器先完成退出，保证编辑器关闭后焦点返回列表触发按钮。移除旧 Modal 样式、重复主题入口和移动导航遮罩逻辑。

以下命令在实施 worktree 中通过：

- `mise exec -- npm --prefix web run lint`（ESLint、Knip）
- `mise exec -- npm --prefix web run typecheck`
- `mise exec -- npm --prefix web run test`：29 个文件、135 项测试通过，包含跨 Modal/Drawer 堆叠、忙碌关闭保护、条件卸载、来源详情转编辑焦点恢复、退出生命周期、列选择和敏感值清理。
- `mise exec -- npm --prefix web run build`
- `git diff --check`

生产构建 JS 合计 832.60 kB（gzip 254.14 kB），CSS 合计 107.47 kB（gzip 20.79 kB）；相对基线分别增加 158.81/49.03 kB 和 21.27/4.48 kB（原始/gzip）。主入口 JS 为 435.92 kB（gzip 136.47 kB）。仅引入本批采用的 Mantine 组件样式及其依赖，后续迁移组件时需要补充对应样式；尚未进行运行时性能基准。

浏览器使用本地生产构建预览，经 Vite 代理访问用户授权的管理端。只读取配置和已有用量，打开/取消表单；未提交配置、轮换/撤销密钥、读取完整 Virtual Key 或调用模型。代表性检查：

| 范围 | 实际结果 |
| --- | --- |
| 来源浮层 | 桌面浅/深色长编辑表单标题与底部固定；Tab/Shift+Tab 保持在浮层，Esc 取消后返回编辑按钮；详情 → 编辑 → 取消返回原“查看”按钮，退出动画保留已输入内容 |
| 模型与路由 | 逻辑模型详情 → 编辑 → Esc 返回原“查看”按钮 |
| 请求事件 | 30 天真实记录可读，列设置切换和 Esc 返回触发按钮；键盘打开详情时页面滚动位置保持，背景滚轮被锁定，关闭返回事件行；390px 下长 ID 换行、固定底部与全宽抽屉正常 |
| 移动导航 | 390×844 浅色英文/深色中文切换；Esc 或选页后关闭，焦点返回导航按钮，关闭后的导航不留在可访问树 |
| 其他页面 | 总览/用量分析 30 天数据与图表可读；模型发现可筛选已确认记录并打开/关闭能力编辑；能力矩阵可读 native/不可路由和长详情；设置页轮换表单打开/取消正常 |

真实管理端的代表性截图仅保留在本地，不纳入 Git 提交或 PR，以免发布用量和账号标识。仓库中的迁移前截图仅包含未连接后端的错误态；后续公开视觉证据使用合成数据。

### 尚未验收

本批没有完成 Button/字段/下拉、反馈、表格与图表的迁移，也没有完成八页全部浅/深色、桌面/窄屏及加载/错误/空态组合。真实表单提交、模型发现确认、连接测试、密钥读写和导入导出未执行；已有 DOM 测试不能替代这些完整浏览器流程。密集事件列表滚动与列测量、嵌套 Select/Popover、屏幕阅读器及性能继续由后续批次验证。一次窄屏自动化指针点击虚拟事件行未打开详情，随后键盘打开成功；指针命中与虚拟化滚动组合仍需单独复核，不能据此宣称事件列表完整验收。没有 Rust 改动，未运行后端或 live Provider 测试。总任务 #166 保持未完成。
