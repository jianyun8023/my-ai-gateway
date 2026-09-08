# Mantine 控制台迁移清单（#166）

初始基线：2026-09-08，`main c99455e`（PR #168 已合并）。当前第六批基线为 `main f3ff2a3`（PR #173 已合并），分支 `codex/166-mantine-tables`。关联 [Issue #166](https://github.com/jianyun8023/my-ai-gateway/issues/166)，本清单随每批实现更新；未完成全部验收前不关闭总任务。

Issue 中 `de708e8` 的链接是历史调查依据。当前事件详情已位于 `features/usage/UsageEventDetails.tsx` 并复用公共 Modal；旧 Select、PortalTooltip、QuestionMarkHelp 等无调用实现已在 #168 删除，不再列为线上迁移对象。

## 职责与批次

1. Mantine 基础组件负责通用控件、Portal、焦点、滚动锁、定位和动画。
2. `components/ui` 负责品牌主题对接及确有项目契约的组合：Modal/Drawer 的统一标题、底部操作区、关闭禁用和退出回调；字段的 label/hint/error 关联；主次操作和状态语义。无需契约的 Mantine 组件从公共入口直接导出，不机械封装。
3. `features` 负责业务数据、字段、校验、提交、筛选和领域展示；不重新实现浮层基础交互。

首批：完整清单、主题接入、公共 Modal/Drawer、移动导航、事件列设置、退出生命周期与组合回归。后续批次继续控件、反馈、表格、图表和八页完整验收。本清单中的“接入”表示受公共基础覆盖，不等于该页完成验收。

## 页面与流程覆盖

路径相对于 `web/src/`。前五批 PR #169、#170、#171、#172、#173 均已合并且 CI 通过；下表已按当前第六批更新。接入和代表性验证不等于该页完成全部验收。

| 页面/流程 | 当前组件与实际入口 | 目标与保留项 | 迁移批次/PR | 验证证据与剩余工作 |
| --- | --- | --- | --- | --- |
| 应用壳 | `GatewayConsoleShell`：桌面侧栏、移动导航、连接、主题、语言、刷新 | 保留 hash 与状态契约；公共 Drawer/字段/按钮 | #169、#170、#171 | 代表性窄屏导航、焦点、双主题与语言已验证；导航和工具栏整体收敛、全部嵌套组合待验收 |
| 总览 | `UsageOverview`、`UsageFilters`、`UsageTrend` | Mantine 筛选/反馈/构成图；保留 Chart.js 与独立 Total | #169–#173 | 双主题趋势/精确表/单图构成、390px 已验证；KPI 与完整刷新/空错状态待收敛 |
| 用量分析 | `UsageAnalysis`、`TokenDistribution`、`TokenComposition` | Progress 分布、Source 平均/P95 Table；完整值与 missing 保留 | #169–#173，第六批共享 Table 主题 | 代表性中英文、长字段、真实零值与缺失已验证；导出及全状态组合待验收 |
| 请求事件 | `UsageEvents`、`UsageEventDetails` | Popover 列设置、公共 Drawer；保留 TanStack Virtual | #169–#172；列表待后续 | 详情/列设置/反馈已验证；窄屏行指针命中、密集滚动/列测量与刷新待验收 |
| 来源 | `SourcesPage`、`SourceForm`、`AccountForm`、`SourceDetailDrawer` | 公共表单/浮层、Table 来源/账号/预设差异 | #169–#172，第六批表格 | 编辑隔离、键盘详情与返回焦点、窄屏预设差异滚动已验证；真实提交/连接测试待验收 |
| 模型发现 | `ModelDiscoveryPage`、`discovery/*` | 公共过滤/Checkbox/反馈与 Mantine Table | #169–#172，第六批表格 | 可选范围、筛选清空、部分失败、批量选择已验证；发现/确认完整流程待验收 |
| 模型与路由 | `ModelsRoutesPage`、`models/*` | 三实体 Mantine Table；保留 Binding/Route/逻辑模型与运行时链 | #169–#172，第六批表格 | 详情转编辑、宽表键盘滚动/访问操作列已验证；三实体真实提交与全部状态待验收 |
| 能力矩阵 | `CapabilitiesPage` | 公共筛选、Mantine Table/Drawer；保留能力状态 | #169–#172，第六批表格 | 原生/降级/不可路由、筛选和长详情代表性验证；全部协议/主题/窄屏组合待验收 |
| 设置 | `SettingsPage`、`VirtualKeyForm`、`VirtualKeyRotationForm` | 公共表单/确认、Mantine Key Table；保留敏感值边界 | #169–#172，第六批表格 | 原有 Key DOM 回归、英文窄屏列表与缺失时间已验证；真实密钥读写与导入导出完整验收待完成 |

## 组件处置清单

| 类别 | 处置 |
| --- | --- |
| 活跃 Modal | 首批替换其手写焦点、滚动、计时和动画；来源/实体/能力详情的 380ms 外部计时改用 Mantine 退出回调 |
| 移动侧栏 | 首批替换遮罩、手写 body overflow 和 Esc/焦点计时器；桌面导航继续使用同一内容 |
| 活跃列设置 details | 首批迁移 Popover；复选项使用 Mantine Checkbox |
| Button、IconButton、FormField、CheckboxField | 第二批迁移公共入口及其调用页面；字段下拉使用 Mantine NativeSelect，第三批收敛页面内筛选和 Shell 密钥输入 |
| SegmentedTabs、LanguageSwitcher、Toggle | 第三批迁移到 Mantine Tabs、Button、Switch，保留面板/筛选语义和布尔值回调 |
| Notice、LoadingState、LoadingSpinner、EmptyState、StatusPill | 第四批采用 Alert、Loader、Paper、ThemeIcon、Text、Badge，保留持久错误、部分失败与重试信息 |
| Card、TableScroll、FormGrid、FilterBar、PageActions、DrawerSection | 第四批 Card 使用 Paper/Title/Text，样式归入 UI 层；第六批 TableScroll 与 Mantine Table 共用 UI 样式，领域组合继续保留 |
| Chart.js、TanStack Virtual、格式工具 | 保留专业实现；第五批统一趋势主题、数值表和缺失值，分布改用 Progress；代表性浏览器验收完成，运行时性能验证仍待完成 |
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

## 第二批：公共按钮与表单字段

基线为首批 `2966895`，分支 `codex/166-mantine-controls`。首批真实管理端截图已从提交历史移除，仅保留本地副本；本批公开截图全部使用合成数据。

迁移入口：`Button` → Mantine Button；`IconButton` → ActionIcon/Tooltip；`TextField`、`SelectField`、`TextAreaField` → TextInput、NativeSelect、Textarea；控制面 `CheckboxField` 移到 UI 层并使用 Mantine Checkbox。公共入口覆盖来源/账号、模型发现、模型与路由、设置等已有表单，以及页面主次操作与行操作。保留原生受控值和 option、独立表单提交按钮、加载禁用、布尔值回调、label/hint/error/调用方描述 ID。

删除旧 `.btn` 全局视觉、字段框、图标按钮及控制面复选框样式。品牌外观集中在 `Controls.module.scss` 和主题的 Input/InputWrapper 配置；页面保留领域布局。原有仅断言按钮样式字符串的测试删除，行为测试保留，并新增实际 SourceForm 的 JSON 校验、选择项/复选框提交、输入保留和忙碌禁用回归。

浏览器验证：真实管理端来源编辑可输入、改变模式/启用勾选后取消，再打开仍为原配置，关闭焦点回到编辑按钮；未提交真实配置。临时合成数据页面直接挂载同一个 SourceForm，确认无效 JSON 提示、修正后本地提交、提交时禁用控件，以及浅色桌面/深色 390px 下的长表单和固定操作区；窄屏字段与按钮均为 44px，页面无横向溢出。截图：[桌面浅色](evidence/166/b2-source-form-light.png)、[窄屏深色](evidence/166/b2-source-form-mobile-dark.png)。合成页面无后端请求，验证后已移除。

第二批不包含页面内直接编写的筛选控件、Toggle、分段/语言切换、反馈、表格与图表。NativeSelect 使用浏览器菜单，尚未取得可靠的原生菜单 Esc 组合自动化证据；Mantine Select/嵌套 Popover 的 Portal 组合也未纳入本批。

验证命令均在本批 worktree 执行通过：`mise exec -- npm --prefix web run lint`（ESLint/Knip）、`typecheck`、`test`、`build`，以及 `git diff --check`。测试为 28 个文件、134 项；相比首批删除 2 项旧样式字符串断言，增加 1 项来源表单行为回归。没有后端改动，未重复运行本地后端/live Provider 测试；首批 PR #169 的静态/构建、单元/PostgreSQL 两项 CI 均通过。

本批生产构建 JS 合计 873.88 kB（gzip 267.42 kB），CSS 合计 133.71 kB（gzip 24.55 kB）；相对首批增加 41.28/13.28 kB 与 26.24/3.76 kB（原始/gzip）。主入口 JS 为 474.66 kB（gzip 147.93 kB）。增加 Button、ActionIcon、Tooltip、Input、Loader 及 NativeSelect 所需 Combobox 样式；未新增依赖包。

## 第三批：页面筛选与选择控件

从前两批 PR #169、#170 合并后的 main `ed597cb` 开始，分支 `codex/166-mantine-navigation-filters`，继续关联 #166。

用量公共/高级筛选和自定义日期、能力矩阵搜索/条件、模型发现确认/可用状态使用共享 TextField/SelectField；趋势指标使用 Mantine NativeSelect，并补上可访问名称。Shell 桌面与移动 Admin Key 输入共用 TextField，保留原有会话密钥边界。时间预设及语言切换使用 Mantine Button；SegmentedTabs 的内容面板模式交由 Mantine Tabs 处理方向键/Home/End/循环与单一 Tab 停靠点，筛选组保留 `aria-pressed`。控制面 Toggle 使用 Mantine Switch，发现目录选择使用 Checkbox；紧凑复选图标通过关联 label 保持 44px 点击区域。

删除页面旧输入框、选择器、开关和语言按钮视觉，仅保留布局与必要的标签外观。保留原有 UTC/本地日期换算、相对时间立即应用、自定义/高级条件草稿应用、发现筛选清空选择、仅可用且待确认模型可选，以及未知/缺失状态。

新增四项行为回归：公共/高级筛选草稿与缺失用量应用/重置；发现可选范围与筛选清空；启用开关布尔回调与禁用；语言按压状态与持久化。原有标签键盘/面板关联、Admin Key 边界等测试继续通过。

浏览器检查使用本地构建预览读取授权管理端：来源/账号标签方向键与面板 ID 正确；发现可按已确认目录筛选，已确认行保持禁选；能力矩阵来源/路由状态筛选生效。没有提交真实配置或发送模型请求。另以直接挂载实际 Shell、FilterBar、SegmentedTabs、Toggle 的合成页面验证：草稿到应用、缺失用量、开关、窄屏标签点击、自定义日期、中英切换和移动侧栏。桌面浅色及 390px 深色无页面横向溢出；窄屏字段和操作按钮为 44px。公开截图：[桌面筛选](evidence/166/b3-filters-light.png)、[窄屏自定义与高级筛选](evidence/166/b3-filters-mobile-dark.png)。临时验证页面已移除。

本批仍不包含反馈、卡片/表格/图表迁移、全八页状态组合或真实写操作验收；原生菜单 Esc、嵌套 Mantine Select/Popover、虚拟事件行指针命中等先前未覆盖项继续保留。总任务 #166 尚未完成。

本批验证通过：`mise exec -- npm --prefix web run lint`（ESLint/Knip）、`typecheck`、`test`（28 个文件、138 项）、`build`、`git diff --check`。无 Rust 或后端契约改动，未运行后端/live Provider 测试。构建 JS 合计 882.82 kB（gzip 270.30 kB）、CSS 合计 141.51 kB（gzip 25.59 kB），相对第二批增加 8.94/2.88 kB 和 7.80/1.04 kB（原始/gzip）；主入口 JS 493.77 kB（gzip 153.60 kB）。新增 Tabs/Switch 按需样式，无新增依赖包。

## 第四批：反馈、状态与卡片

从 PR #171 合并后的 main `5d9449e` 开始，分支 `codex/166-mantine-feedback`。PR #171 的静态/构建、单元/PostgreSQL 两项 CI 均通过。

- Notice 使用 Mantine Alert，共享成功、警告、错误语义色；保留错误的 alert、成功/警告的 status 和重试/关闭操作。发现声明不支持、发现运行失败及 FormError 复用此入口，错误码、说明和已有目录仍可同时查看。
- LoadingSpinner 使用 Mantine Loader，装饰图标不再建立第二个空 status；LoadingState 外层播报标签。局部请求尝试加载使用 inline 布局；Loader 样式在减少动画偏好下停止旋转。
- StatusPill 使用 Badge，保留页面业务映射、大小写和未知状态；覆盖 Badge 默认的大写/省略展示以允许长文本换行，事件与控制面共享配色。
- EmptyState 使用 Paper/Text/ThemeIcon 组合，保留 inline/centered 和下一步按钮。Card 使用 Paper/Title/Text，保持标题层级、说明、元信息、操作区及 flush 表格容器，窄屏操作区堆叠。

公共样式收敛到 `Feedback.module.scss`、`Card.module.scss`；移除旧全局卡片、空态、加载动画及页面独立反馈样式。没有新增依赖包或改变后端契约。

DOM 回归覆盖错误重试到成功与关闭、单次加载播报转空态、警告详情及完整状态文本、卡片语义与操作；真实页面测试覆盖 401 重试恢复和发现失败仍保留目录。旧 Card/视觉样式字符串断言按新组件行为替换或删除。

浏览器使用本地生产构建只读取授权管理端：来源页卡片和状态密度正常，标题 16px、状态标签约 21px；桌面点击事件行可打开详情，Token/上游尝试卡片可读，局部加载最终显示尝试记录。合成页面验证失败→重试加载→成功→关闭，警告、长错误码/状态、空态操作及卡片头部；桌面浅色和 390px 深色无横向溢出，窄屏按钮为 44px。截图全部使用合成内容：[桌面浅色](evidence/166/b4-feedback-light.png)、[窄屏深色](evidence/166/b4-feedback-mobile-dark.png)。临时页面已移除，未提交真实配置或请求模型。

本批没有完成图表与表格迁移、全八页状态组合、屏幕阅读器实测或运行时性能基准。减少动画规则已实现，尚未在浏览器切换系统偏好复验。此前窄屏虚拟事件行指针命中、原生菜单 Esc 与嵌套 Select/Popover 验收缺口仍保留；本批桌面事件点击成功不能替代窄屏复验。#166 保持未完成。

验证通过：`mise exec -- npm --prefix web run lint`（ESLint/Knip）、`typecheck`、`test`（29 个文件、139 项）、`build` 和 `git diff --check`。没有后端改动，未重跑后端或 live Provider 测试。本批构建 JS 合计 889.92 kB（gzip 272.32 kB），CSS 合计 146.54 kB（gzip 27.03 kB）；相对第三批增加 7.10/2.02 kB 与 5.03/1.44 kB（原始/gzip）。主入口 JS 494.30 kB（gzip 153.67 kB）。按需引入 Alert、Badge、Text、Title、ThemeIcon 样式，复用已接入的 Paper/Loader。

## 第五批：图表主题、分布与原型核对

基线为 PR #172 合并后的 main `ab5dad0`，分支 `codex/166-mantine-charts`。原型逐项核对与数据依据见[图表核对记录](chart-prototype-review.md)。

- 总览增加默认 Input/Output 堆叠趋势；保留原有六类指标切换和独立 Total，双轴有明确名称。Canvas 图例、轴、网格、tooltip 和序列使用品牌 Token；提供可展开的精确数值表。
- 总览模型分布与分析八个维度共享 Mantine Progress 行，显示 Token、占同一筛选范围比例、请求数及前 N/总组数。移除固定高度横条 canvas 与类别索引 tooltip 路径；零值不画人为最小进度。
- Token 构成使用 Progress.Root/Section 合并为一张 Input/Output 分段图，按两者合计计算占比。推理、缓存读取和缓存创建改为紧凑数值明细；保留零值、missing 和独立上报 Total，合计不一致时说明。缓存/推理不参与归一化，简单缓存比值明确命名为“缓存读取 / 输入”。
- 来源延迟采用 Mantine Table，只比较 Source；接入已有后端 P95，显示平均值、P95 与请求数，缺失和真实零值分开。后端/数据库契约未变。

验证通过：`mise exec -- npm --prefix web run lint`（ESLint/Knip）、`typecheck`、`test`（30 个文件、145 项）、`build`、`git diff --check`。新增六项回归覆盖趋势切换/独立 Total/精确表、分布排序/零值/分母/截取、重叠与 missing、来源延迟/P95、空趋势及 API P95 映射。未运行后端/live Provider 测试；本批只通过本地生产构建读取授权管理端，未执行管理写操作或模型请求。

浏览器锁屏解除后，使用直接挂载实际 Shell、Overview、Analysis 的合成页面完成以下验收：

| 范围 | 结果 |
| --- | --- |
| 原型对照 | 实际打开归档总览/分析页，核对趋势柱、构成条、分布密度与来源延迟表；差距与取舍记录在图表核对文档 |
| 桌面浅/深色 | Input/Output 堆叠、Total/请求数双轴切换及展开/收起精确表正确；图例、轴、网格与 tooltip 可读。数据点提示 153,900 Token / 559 请求与同时间桶表格一致；堆叠提示 95,000 输入 / 58,900 输出与测试数据一致 |
| 单图构成 | 一个可访问图像展示输入/输出占比，推理/缓存仅作数值明细；浅色桌面与深色窄屏可读，中英切换后标签完整 |
| 390px | 趋势 canvas 为 324×230px；页面 scrollWidth 382px，没有横向溢出。查看/收起数据按钮 44px；420px 表格在局部容器滚动，可聚焦并通过方向键横向滚动 |
| 来源与分布 | 长来源 ID 可换行；平均/P95 2.1s/3.6s、真实 0ms、缺失 `—` 区分正确。Source 表只显示 Source；零 Token 分布没有假进度 |

公开截图：[总览浅色](evidence/166/b5-overview-light.png)、[暗色折线与精确表](evidence/166/b5-trend-dark.png)、[窄屏总览](evidence/166/b5-overview-mobile-dark.png)、[分析浅色](evidence/166/b5-analysis-light.png)、[单图构成浅色](evidence/166/b5-composition-light.png)、[单图构成窄屏深色](evidence/166/b5-composition-mobile-dark.png)。全部为合成数据，原型截图也是仓库静态示意内容。临时页面已移出仓库；本批浏览器验收不等于全八页或生产验收。

控制面表格、全八页状态组合、虚拟事件列表指针命中与滚动、先前记录的真实写流程和性能验收仍未完成。总任务 #166 保持开放。

本批构建 JS 合计 901.50 kB（gzip 276.08 kB）、CSS 合计 154.22 kB（gzip 28.28 kB）；相对第四批增加 11.58/3.76 kB 与 7.68/1.25 kB（原始/gzip）。主入口 JS 为 495.91 kB（gzip 154.24 kB）；新增 Progress/Table 按需样式，无新增依赖包。该记录是构建体积，不是运行时性能基准。

另通过本地生产构建复验授权管理端的总览与分析：既有用量已使用同一张输入/输出构成图，缓存/推理明细保留，未重算或写回用量。真实数据只用于当前浏览器检查，未保存为公开截图。

## 第六批：控制面表格与滚动容器

从 PR #173 合并后的 main `f3ff2a3` 开始，分支 `codex/166-mantine-tables`。同时将 #166 中已有完整依据的 23 项标为完成，保留其余 31 项复合范围与验收待办，并补齐前五批合并及验证记录。

九张表采用 Mantine Table：来源、账号、逻辑模型、Binding、Route、模型发现目录、能力矩阵、Virtual Key 和来源预设差异。Table/Th/Td 等直接使用 Mantine 语义元素，不新增通用数据表包装或改变查询/排序/选择模型。所有控制面列标题声明 `scope="col"`；已有可点击行仍保持表格语义，通过独立查看按钮提供键盘入口，操作/开关单元格继续阻止事件冒泡。

表头、单元格密度、行分隔线、悬停与操作按钮尺寸集中到 UI 的 `Table.module.scss`，主题默认纵向/横向间距为 10px/13px。TableScroll 保留带名称、可聚焦的原生横向滚动容器；移除控制面重复通用表格样式，保留各表最小宽度与领域列布局。窄屏状态文字保持一行，预设差异类型列保留 96px 最小宽度，避免短状态被挤成多行。图表数值表与来源延迟表也继承相同 Table 基础主题。

新增一项实际来源表行为回归，覆盖表头/滚动区语义、编辑不误开详情、关闭后再点击数据单元格打开详情。既有发现选择/禁用、三实体 CRUD、状态区别及 Virtual Key 边界测试继续通过。

浏览器使用挂载实际 Shell 和 GatewayManagementPage 的合成数据页面验证：

| 范围 | 结果 |
| --- | --- |
| 桌面来源表 | 单元格内边距 10px/13px，操作按钮 36px；编辑只打开编辑框，模拟停用不打开详情；键盘 Enter 查看、Esc 关闭后焦点回到原查看按钮 |
| 390px 深色 Binding 表 | 页面宽度与 scrollWidth 均为 390px；1260px 表在 364px 容器内滚动。方向键可横向滚动；键盘访问操作列后 scrollLeft 为 896px，焦点可达编辑按钮，四个操作按钮均为 44px |
| 预设差异抽屉 | 390px Drawer 中的 1040px 表在 348px 局部区域滚动；差异路径、前后值完整保留；类型列 96px，状态标签保持约 21px 单行高度 |
| 能力/发现/设置 | 浅深色表头与状态可读；原生、降级、不可路由保持区别；发现全选后待确认行被选中并启用批量确认。英文窄屏 Key 表不撑破页面，缺失最后使用时间保持 `—` |

公开截图均为合成数据：[来源浅色](evidence/166/b6-sources-light.png)、[窄屏深色操作列](evidence/166/b6-bindings-mobile-dark.png)、[能力矩阵深色](evidence/166/b6-capabilities-dark.png)。临时验证页面已移出仓库。另用本地生产构建只读复验授权管理端来源列表，四条记录正常显示且无页面横向溢出；未执行真实管理写操作或模型请求。

最终检查通过：`mise exec -- npm --prefix web run lint`（ESLint/Knip）、`typecheck`、`test`（30 个文件、146 项）、`build` 与 `git diff --check`。本批无后端、依赖或数据库变更，未运行后端/live Provider 测试。构建 JS 合计 902.43 kB（gzip 276.23 kB）、CSS 合计 154.17 kB（gzip 28.26 kB）；相对第五批分别变化 +0.93/+0.15 kB 与 -0.05/-0.02 kB（原始/gzip），主入口 JS 496.12 kB（gzip 154.34 kB）。

尚未完成：虚拟事件列表窄屏指针命中、密集滚动/列测量与刷新性能；导航与页面模式最终收敛；全八页核心流程及状态/主题/语言/窄屏组合；已记录的真实写流程、全部嵌套浮层和减少动画实测。控制面表格完成不代表 #166 的表格与虚拟列表复合条目或八页验收全部完成。
