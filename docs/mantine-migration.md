# Mantine 控制台迁移清单（#166）

初始基线：2026-09-08，`main c99455e`（PR #168 已合并）。当前第十四批基线为 `main c1118db`（PR #181 已合并），分支 `codex/166-page-browser-evidence`、PR #182。关联 [Issue #166](https://github.com/jianyun8023/my-ai-gateway/issues/166)，本清单随每批实现更新；Issue/PR、代码合并和生产验收分别记录，不互相推断。

Issue 中 `de708e8` 的链接是历史调查依据。当前事件详情已位于 `features/usage/UsageEventDetails.tsx` 并复用公共 Modal；旧 Select、PortalTooltip、QuestionMarkHelp 等无调用实现已在 #168 删除，不再列为线上迁移对象。

## 职责与批次

1. Mantine 基础组件负责通用控件、Portal、焦点、滚动锁、定位和动画。
2. `components/ui` 负责品牌主题对接及确有项目契约的组合：Modal/Drawer 的统一标题、底部操作区、关闭禁用和退出回调；字段的 label/hint/error 关联；主次操作和状态语义。无需契约的 Mantine 组件从公共入口直接导出，不机械封装。
3. `features` 负责业务数据、字段、校验、提交、筛选和领域展示；不重新实现浮层基础交互。

首批：完整清单、主题接入、公共 Modal/Drawer、移动导航、事件列设置、退出生命周期与组合回归。第二至十二批继续控件、反馈、表格、图表、查询状态和逐页证据；第十三批完成组件查漏、遗留样式清理与适用交互收尾；第十四批只补评审点名的逐页业务状态证据。本清单中的“接入”表示受公共基础覆盖，不等于该页所有环境组合均已验收。

## 页面与流程覆盖

路径相对于 `web/src/`。前十三批 PR #169–#181 均已合并；第十二批查询状态、第十三批最终组件/交互和第十四批逐页状态矩阵见文末。接入和代表性验证不等于生产环境验收。

| 页面/流程 | 当前组件与实际入口 | 目标与保留项 | 迁移批次/PR | 验证证据与剩余工作 |
| --- | --- | --- | --- | --- |
| 应用壳 | `GatewayConsoleShell`：桌面侧栏、移动导航、连接、主题、语言、刷新 | 保留 hash 与状态契约；公共 Drawer/字段/按钮 | #169–#171，第八/十二批 | 八页导航/标题/选中态、临界宽度顶栏、移动语言/连接区、详情与确认焦点隔离已验证；第十二批增加不含密钥的认证代次，普通刷新不改变认证身份 |
| 总览 | `UsageOverview`、`UsageFilters`、`UsageTrend` | Mantine 筛选/反馈/构成图；保留 Chart.js 与独立 Total | #169–#173，第十/十二批 | 双主题趋势/精确表/单图构成、390px 已验证；第十二批验证同范围刷新保留已发布数据、切换范围立即隐藏旧数据及错误/空态 |
| 用量分析 | `UsageAnalysis`、`TokenDistribution`、`TokenComposition` | Progress 分布、Source 平均/P95 Table；完整值与 missing 保留 | #169–#173，第六/十/十二/十四批 | 代表性中英文、长字段、真实零值与缺失已验证；第十四批独立重放慢首屏、错误重试和空态；页面没有导出功能（N/A） |
| 请求事件 | `UsageEvents`、`UsageEventDetails` | Mantine Table/Popover/Drawer；TanStack Virtual 测量与单一滚动区 | #169–#172，第七/十/十二批 | 千行测量、重排、分页、窄屏详情已验证；第十二批增加分页/导出独立失败重试、认证身份隔离，以及数据集变空时关闭失效详情并恢复合理焦点 |
| 来源 | `SourcesPage`、`SourceForm`、`AccountForm`、`SourceDetailDrawer` | 公共表单/浮层、Table 来源/账号/预设差异 | #169–#172，第六/十一/十三/十四批 | 编辑隔离、详情/焦点、预设差异、连接成功/失败、来源/账号空态及删除均已由真实 App + 合成 API 重放；不据此推断生产凭据或 Provider |
| 模型发现 | `ModelDiscoveryPage`、`discovery/*` | 公共过滤/Checkbox/反馈与 Mantine Table | #169–#172，第六/十一至十四批 | 来源/筛选查询身份、可选范围与失败反馈已有回归；第十四批完整执行发现 → diff → 选择 → 确认失败保值 → 同请求重试成功 |
| 模型与路由 | `ModelsRoutesPage`、`models/*` | 三实体 Mantine Table；保留 Binding/Route/逻辑模型与运行时链 | #169–#172，第六/十一/十三/十四批 | 三实体表单归因、详情、宽表和 Select 已验证；第十四批补齐三空态、刷新失败保值/重试及三类删除 |
| 能力矩阵 | `CapabilitiesPage` | 公共筛选、Mantine Table/Drawer；保留能力状态 | #169–#172，第六/十一/十三/十四批 | 原生/降级/不可路由、三协议详情和窄屏已验证；第十四批补齐筛选无结果、无快照及刷新失败保值/重试，不扩写为全部协议 × 主题 × 宽度组合 |
| 设置 | `SettingsPage`、`VirtualKeyForm`、`VirtualKeyRotationForm` | 公共表单/确认、Mantine Key Table；保留敏感值边界 | #169–#172，第六/九/十一至十四批 | Key 结果清理、操作区、禁用提示和脱敏导出已有证据；第十四批补慢首屏、错误重试和空 Key。当前没有 UI 导入入口（N/A）；不执行生产密钥写入 |

## 组件处置清单

| 类别 | 处置 |
| --- | --- |
| 活跃 Modal | 首批替换其手写焦点、滚动、计时和动画；来源/实体/能力详情的 380ms 外部计时改用 Mantine 退出回调 |
| 移动侧栏 | 首批替换遮罩、手写 body overflow 和 Esc/焦点计时器；桌面导航继续使用同一内容 |
| 活跃列设置 details | 首批迁移 Popover；复选项使用 Mantine Checkbox |
| Button、IconButton、FormField、CheckboxField | 第二批迁移公共入口及其调用页面；第十三批将字段和趋势下拉统一为 Mantine Select，禁用 Tooltip 使用可聚焦的 `aria-disabled` / `data-disabled` 契约 |
| SegmentedTabs、LanguageSwitcher、Toggle | 第三批迁移到 Mantine Tabs、Button、Switch，保留面板/筛选语义和布尔值回调 |
| Notice、LoadingState、LoadingSpinner、EmptyState、StatusPill | 第四批采用 Alert、Loader、Paper、ThemeIcon、Text、Badge，保留持久错误、部分失败与重试信息 |
| Card、TableScroll、FormGrid、FilterBar、PageActions、DrawerSection | 第四批 Card 使用 Paper/Title/Text，样式归入 UI 层；第六批 TableScroll 与 Mantine Table 共用 UI 样式，领域组合继续保留 |
| Chart.js、TanStack Virtual、格式工具 | 保留专业实现；第五批统一趋势主题、数值表和缺失值，分布改用 Progress；代表性浏览器验收完成，帧耗时基准按独立性能任务记录 |
| 已删除无调用组件 | #168 已清理旧自研 Input/Select、MainActionButton、PortalTooltip、QuestionMarkHelp、QuestionMarkHelpButton；第十三批再删除无生产调用的四个旧全局 SCSS 入口，不重新引入 |

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


## 第七批：请求事件虚拟表

从 PR #174 合并后的 main `535a22e` 开始，分支 `codex/166-mantine-events`。第六批已合并且两项 CI 通过。

源码确认旧事件表头与行内容分属两个横向滚动容器，且虚拟行只有 58px 估算值，没有连接实际行高测量。此次将事件列表接入 Mantine Table 的共享表头、间距、分隔线与操作按钮，保留 TanStack Virtual、现有列设置、导出回调和详情读取契约。单一带名称、可聚焦的原生滚动区包含 44px sticky 表头和虚拟 tbody；两者使用同一列网格，避免独立横向滚动。稳定事件标识用于虚拟行缓存，`measureElement` 观察实际行高；滚动坐标扣除表头高度。

详情入口改为首列公共 IconButton，窄屏继承 44px 触摸尺寸；数据单元格仍可点击，点击时聚焦查看按钮并保留滚动位置，供关闭抽屉后返回。表头使用 `scope=col`，虚拟行提供逻辑行索引，尚有后续页时总行数声明为未知。追加加载复用 LoadingState，不替换已加载行。删除原先只断言旧 CSS 字符串的滚动测试。

新增三项行为回归，使用真实虚拟化实现，仅补足 happy-dom 缺失的布局尺寸：

- 1000 条记录挂载少于 40 行，74px 实际行高覆盖 58px 估算，滚动后逻辑行索引前进；刷新重排保留对应 DOM 与请求身份，列变更同步单元格。
- 详情按钮只触发一次对应请求读取，Esc 关闭后焦点返回；CSV 导出调用原回调，不误开详情。
- 接近末尾触发下一页，加载中不重复请求，追加后保留已有行与滚动区域。

Web ESLint/Knip、TypeScript、测试（30 个文件、149 项）和构建通过，`git diff --check` 通过。本批只有前端与设计说明改动，未执行后端或 live Provider 测试。构建 JS 合计 903.36 kB（gzip 276.58 kB）、CSS 合计 154.04 kB（gzip 28.25 kB）；相对第六批 JS +0.93/+0.35 kB，CSS -0.13/-0.01 kB（原始/gzip），主入口 496.20 kB（gzip 154.36 kB）。构建体积和 DOM 数量不等于运行时性能验收。

浏览器解锁后，使用直接挂载实际 Shell/EventsTable 的合成页面完成验证：

| 范围 | 实测结果 |
| --- | --- |
| 390px 指针与焦点 | 页面 scrollWidth 382px，局部表格滚动区 348px；查看按钮 44px。按钮和时间单元格均能打开对应详情，Esc 关闭后焦点回到查看按钮 |
| 表头与横向滚动 | 表头高度 44px；横向滚动 40px 后，前三列表头与数据单元格 x 坐标逐项相同。英文 Actions 首列调整为 80px，完整显示 |
| 列偏好、语言与主题 | 取消上游模型/协议列后表头与单元格同步；Esc 返回列偏好入口。中英文、浅深色可读，截断长字段在 Drawer 完整显示 |
| 连续分页 | 两次滚到末尾后，1000 条先追加到 2000，再到 3000；约 29–32 行 DOM，追加时保留已有内容 |
| 万行与刷新 | 加载 10000 条并滚到最后，逻辑索引为 9999，只挂载 21 行；模拟刷新使末行 Total 0→1，scrollTop 保持 580029px；英文 Enter 能打开末行对应详情。此处数值变化由合成测试按钮产生，不影响实际用量 |
| 尺寸变化与重测量 | 桌面行高 58px，390px 行高 65px；ResizeObserver 更新后相邻行间距均为 0，无持久重叠或空隙。尺寸变化时出现短暂估算位置，稳定后末行仍可达；未开展帧耗时基准 |

公开合成截图：[万行桌面](evidence/166/b7-events-desktop.png)、[窄屏深色列表](evidence/166/b7-events-mobile-dark.png)、[英文窄屏详情](evidence/166/b7-event-drawer-mobile-dark.png)。临时页面已移出仓库。最终生产构建另只读复验授权管理端事件入口：列表约 20 行 DOM，时间单元格点击后成功读取详情和 Token 卡片，随后恢复原分析页；没有保存真实数据截图、执行管理写入或发送模型请求。

本批解决此前窄屏事件详情指针验收缺口，完成虚拟列表代表性测量/滚动/刷新验证。八页完整状态、浮层全部组合、真实写流程与系统减少动画实测仍待完成，#166 保持开放。该浏览器观察和 DOM 数量记录用于检查明显 UI 回退，不等于生产性能或帧率基准。


## 第八批：应用壳、导航与工具栏

基线 main `12ebda9`（PR #175 已合并且两项 CI 通过），分支 `codex/166-mantine-shell`。

- 八页导航使用 Mantine NavLink（button），保留 hash、当前页面与 `aria-current=page`；通用选中态、焦点、40/44px 密度和减少动画规则集中到 UI Navigation 样式与主题。
- 顶栏采用 Paper、页面标题采用 Title，菜单/主题/刷新统一为 IconButton。刷新继承 ActionIcon loading，阻止重复点击。修复跟随系统深色时主题按钮判断错误：文案和切换目标使用 resolvedTheme。
- 顶栏大屏单行、921–1280px 两行，920px 以下将语言与连接字段移入导航 Drawer；复用一份连接控件，保留草稿、显式应用、清空和 session-only 契约。桌面侧栏限定可用高度并独立滚动，移动侧栏沿用公共 Drawer。移除手写导航/菜单按钮样式、重复密钥 JSX 及其专属文案。

新增两项 Shell 回归覆盖系统深色切换、刷新禁用、未应用草稿跨断点/开关导航保留、Enter 应用后清空；扩充 App 路由回归检查唯一选中项与 h1。Web lint/Knip、TypeScript、30 文件 151 项测试、build 和 `git diff --check` 通过。无后端改动，未运行后端/live Provider 测试。

浏览器使用实际 App 和八页的合成数据完成以下验证：

| 范围 | 结果 |
| --- | --- |
| 八页导航 | 逐个打开总览、分析、事件、来源、发现、模型与路由、能力、设置；hash、h1 与唯一当前导航一致，921px 均无页面横向溢出 |
| 桌面与临界宽度 | 1468px 导航 40px、顶栏 59px；921px 英文顶栏为 107px 两行，标题宽约 447px，长网关 URL 局部截断且完整值保留，操作可见 |
| 矮窗口 | 400px 高度桌面侧栏内导航区 331px，可滚动 134px 到设置，焦点可达末项 |
| 移动侧栏 | 390px Drawer 全宽，导航和操作均 44px；完整网关 URL 可换行，语言和连接区在 Drawer 内滚动可达。768px 下 Drawer 为 320px、无内容横向溢出 |
| 焦点与主题 | 移动侧栏内中英切换同步标题/导航；Tab 保持在侧栏，Esc 返回菜单按钮并恢复背景滚动。深色切换后来源详情的连续 12 次 Tab、删除确认框焦点均留在当前浮层；确认框仅取消，未提交 |
| 后台刷新 | 模拟慢请求时刷新按钮禁用、来源行保留，移动导航仍可打开；恢复即时响应后正常使用 |

公开截图：[桌面浅色](evidence/166/b8-desktop-shell-light.png)、[921px 两行顶栏](evidence/166/b8-compact-desktop-toolbar.png)、[移动导航深色](evidence/166/b8-mobile-navigation-dark.png)、[移动连接区深色](evidence/166/b8-mobile-connection-dark.png)。全部是合成数据，临时页面已移出仓库。本批没有执行真实管理写入或应用真实 Key；浏览器输入测试受 1Password 提示干扰，取消后重载到空草稿，密钥草稿/应用行为以 DOM 回归为证，不记作完整浏览器密钥验收。

构建 JS 合计 909.38 kB（gzip 278.52 kB），CSS 155.95 kB（gzip 28.54 kB）；相对第七批增加 6.02/1.94 kB 与 1.91/0.29 kB（原始/gzip）。NavLink 接入使主入口为 506.35 kB（gzip 157.46 kB），触发 Vite 默认 500kB 提示，构建仍通过；没有隐藏警告或为此引入额外分包方案/依赖。本批未建立帧耗时基准。

八页完整业务状态、重复页面组合、KPI/通知规则、全部嵌套浮层、原生菜单 Esc、系统减少动画偏好及真实写流程仍待收敛。这里只完成应用壳范围与代表性组合验证，不据此勾选八页整体验收或关闭 #166。


## 第九批：短暂通知与持久结果

基线 main `7b0beab`（PR #176 已合并且两项 CI 通过），分支 `codex/166-notification-feedback`。范围限于公共通知入口、来源、模型与路由、模型发现及设置的操作反馈，不包含 KPI、重复页面组合或八页整体验收。

- ConsoleProvider 接入唯一 Mantine Notifications 宿主，新增 `@mantine/notifications@9.6.0`，与 core/hooks 精确同版。右上角、400px 上限、5 秒自动关闭、悬停暂停、命名关闭按钮、窄屏 44px 操作、180ms 过渡与减少动画继承集中管理；样式使用品牌表面/文字/状态变量，通知层级 1400。
- 普通 CRUD / 保存 / 导出等使用薄入口 `notifySuccess`；新操作清掉旧成功提示，后续不同消息使用新 ID 重置完整时长。删除四页旧 notice state 和无调用 SuccessNotice，不新增业务抽象。
- 模型发现全部结果由最新运行详情展示，不再把 failed / unsupported 当成功通知；结果刷新失败保留目录和重试，不提前报成功。不支持结果独立展示错误码。能力保存通知在仍打开的 Modal 上方，失败继续保留行草稿；批量确认错误回到确认框内。
- Key 创建/读取/轮换只使用结果 Modal，复制反馈不含 Key 正文。轮换最晚有效时间（重叠期与原到期时间较早者）或立即失效说明持续保留，关闭即清除敏感值和复制状态。

验证通过：`mise exec -- npm --prefix web run lint`（ESLint/Knip）、`typecheck`、`test`（30 文件 **159 项**）、`build` 和 `git diff --check`。增加 8 项回归，覆盖通知单次 status/不抢焦点/命名关闭/自动关闭/连续替换重新计时、来源失败保留表单及重试、发现 failed/unsupported 单一反馈与目录/错误码、发现刷新失败重试、能力保存留在 Modal、Key 时限/复制/立即清理；既有 Key 创建和读取测试扩充无重复通知与正文不进 live region 断言。

独立只读审查发现固定 ID 会吞掉后续消息且可能继承旧计时，已改为替换旧通知并生成新 ID，4.9 秒后替换再等待 101ms 的边界回归通过；复核无剩余代码阻断。审查另纠正文档，默认布局只在悬停时暂停，不宣称键盘聚焦暂停。

浏览器使用真实 GatewayManagementPage 与来源/发现/设置组件，所有网络响应均为本地合成数据，未连接生产或调用 Provider：

| 范围 | 实测结果 |
| --- | --- |
| 桌面浅色来源保存 | 保存后唯一成功通知，编辑框关闭后焦点返回“编辑 source-a”；通知自动消失，列表保留。400px 通知未撑宽页面 |
| 深色发现结果 | 模拟失败后最新运行显示 failed、完整英文原因及错误码，已有 SourceModel 目录保留，没有伪成功通知 |
| 390px 英文能力保存 | 通知宽 358px，关闭按钮 44×44px，页面 scrollWidth 390px；编辑 Modal 保持打开，保存后焦点留在 Save capability。关闭通知不关闭能力编辑框；通知层级 1400 高于 Modal |
| Key 创建与轮换 | 浅色中文创建显示结果与自动复制状态，无额外通知；390px 深色英文轮换结果保留完整时限和新 Key，显式复制后时限仍可阅读。两种结果点击 Done/完成后立即不再包含合成 Key |

公开截图全部为合成数据：[来源成功浅色](evidence/166/b9-source-success-light.png)、[发现失败深色](evidence/166/b9-discovery-failed-dark.png)、[能力保存窄屏深色](evidence/166/b9-capability-mobile-dark.png)、[Key 结果窄屏深色](evidence/166/b9-key-result-mobile-dark.png)。临时验证入口已移出仓库，浏览器视口已恢复。

按 Vite 输出合计：JS **935.48 kB / gzip 286.90 kB**，CSS **160.26 kB / gzip 29.10 kB**；相对第八批约增加 **26.10/8.38 kB** 与 **4.31/0.56 kB**（原始/gzip）。主入口 **532.98 kB / gzip 165.90 kB**，继续保留默认 500kB warning，构建通过；没有隐藏提示或扩展分包改造。通知新增 store/transition 依赖由锁文件记录，未升级其他直接依赖。

未覆盖：系统减少动画偏好的浏览器实测、屏幕阅读器实际朗读、全部键盘关闭组合、全八页业务状态、真实配置提交/Provider/Key 验收和性能基准。系统减少动画由现有主题与 Mantine 实现继承，不能据源码或 DOM 测试记为浏览器通过。本批无后端改动，本地未运行 Rust/PostgreSQL/live Provider 测试；PR CI 结果另在关联 PR 与 Issue 记录。#166 保持开放。

## 第十批：用量 KPI、数值格式与状态表达

基线 main `b0e433c`（PR #177 已合并），分支 `codex/166-usage-metrics`。范围限于总览、分析和请求事件的指标、格式与来源状态，不改 Rust、Admin API、数据库或核算规则，不包含页面组合提炼、日期风格、刷新体验或虚拟化改造。

- 新增薄 `MetricCard`，以 Mantine Paper/Text 和品牌变量集中指标排版、辅助信息、精确值 title/可访问名称及窄屏换行；总览保留自己的四列/两列/单列网格。移除旧 Stat 及对应页面视觉样式。
- 百分比、ms/s、精确毫秒和事件 Token 格式集中复用；真实 `0 ms` 与缺失 `—` 分开。K/M/B/T 规则保持不变，详情显示精确带分隔符数值，大数窄屏按可用宽度布局。
- 最近请求、事件状态和 attempt 使用有文字的 `UsageStatus`；最近请求新增 `UsageBadge`，保持五类来源差异。missing/unknown 的记账零有明确说明；真实上报零保留，unknown 非零读数不丢弃。保留模型、Provider、Source、账号、协议和 fallback 归因，attempt 同时显示上游协议。
- 修复真实 summary 不返回 usage_sources 导致计数为空的问题：每次 summary 并行增加一条已有 `breakdown=usage_source` 查询，共享 filters/signal，用真实响应 `key/logical_requests` 组装。两项成功后统一发布，失败沿用错误/重试，不 catch 为零或空字典；分页和导出不增加来源查询。此处不承诺跨数据库查询的事务快照一致。
- 构成区域显示来源计数及 missing/estimated/unknown 解释；全 missing/unknown 且记账总量为零时，KPI、构成与分布不宣称已确认零。独立 Total、输入/输出、推理和缓存含义保持不变。

最终 Web 验证：`mise exec -- npm --prefix web run lint`（ESLint、Knip 两种门禁）、`typecheck`、`test`（32 文件 **176 项**）、`build` 通过，`git diff --check` 通过。新增 17 项测试及既有断言扩充，覆盖真实来源 API 夹具、完整筛选/signal、每轮一次来源查询、任一请求失败、筛选/刷新迟到响应隔离、零/缺失延迟、上报零/全 missing/unknown/混合来源、精确大数和无请求；保留图表独立 Total、零分布及事件 fallback 断言。独立只读审查发现最近请求列宽下限风险，修复后复核无剩余阻断；同时收窄页面颜色选择器，避免覆盖公共状态标签颜色。

浏览器使用真实 Overview / Analysis / EventsTable / EventDetails 组件与公开合成数据，未连接生产或调用 Provider：

| 实测范围 | 结果 |
| --- | --- |
| 390px 英文深色总览 | 四个 KPI 两列、0 ms/P95 缺失、1.2T 精确名称、长模型/来源、成功失败及估算/缺失标签可读；无页面横向溢出 |
| 375px 中文浅色全 missing | KPI 单列；总量、构成、分布与最近请求保留不可用说明/缺失标签，成功率独立显示；无横向溢出 |
| 375px 英文深色分析 | 来源计数与混合估算/缺失说明、长分布名称、Source 的 0 ms 与缺失 P95 可读；宽延迟表仅在自身容器滚动 |
| 390px 英文深色、375px 中文浅色事件详情 | 从实际列表入口打开详情，精确 `1,234,567,890,123`、缺失 Token 说明、模型/协议/Source/fallback 和成功/失败 attempt 可读；详情内部滚动，数字未撑宽内容 |
| 1024/1280/1281px 总览 | 桌面预留与 Shell 相同的 220px 侧栏空间；最近请求及其子元素无横向溢出，1281px 双列下成功/失败词完整显示 |

公开截图：[总览桌面浅色](evidence/166/b10-overview-desktop-light.png)、[总览 390px 深色](evidence/166/b10-overview-mobile-dark.png)、[全 missing 375px 浅色](evidence/166/b10-overview-missing-375-light.png)、[分析 375px 深色](evidence/166/b10-analysis-375-dark.png)、[事件详情深色](evidence/166/b10-details-mobile-dark.png)。全部数据为合成；开发服务未自动捕获部分文件变化，重启后按最终代码重验相关布局，临时入口已移出仓库，视口已恢复。

最终 Vite 资源：JS **938.54 kB / gzip 287.70 kB**，CSS **160.34 kB / gzip 29.08 kB**；相对第九批分别为 **+3.06/+0.80 kB** 与 **+0.08/−0.02 kB**（原始/gzip）。主入口 **533.49 kB / gzip 166.09 kB**，保留默认 500kB warning，构建通过。本批无新增依赖或无关分包优化。

交付中附带最小 CI 恢复：PR #178 两轮运行在 mise 自动选择 `2026.9.3` 后因下载 404 失败，未进入源码检查。两个 PR job 固定 mise 安装器为已发布的 `2026.9.2`；Rust/Node 版本、所有检查及权限保持原状，详见 [CI 说明](ci.md)。恢复后的最终 head 检查结果以 PR 为准。

未覆盖：屏幕阅读器实际朗读、全八页核心流程及加载/空态/错误/刷新组合、全部嵌套浮层/系统减少动画实测、真实配置和 Provider 验收、性能基准。日期显示沿用现状，本批不治理日期。无后端改动，本地未运行 Rust/PostgreSQL/live 测试；最终 head 的 PR CI 另在 Issue/PR 记录。#166 保持开放，只有指标/状态/格式条目可据本批完成；八页整体验收和其他复合条目继续留空。

## 第十一批：筛选容器、编辑操作与详情字段组合

基线 main `9e5e1ed`（PR #178 已合并），分支 `codex/166-shared-compositions`。仅提炼源码中实际重复的组合，无新依赖、接口或后端变化；保留既有 FormGrid、DrawerSection、PageActions。

- UI `FilterPanel` 基于 Mantine Paper，集中品牌表面、边框、12px 内边距/圆角、命名 section 和收缩边界。发现/能力 `FilterBar` 与用量 `FilterBar` 接入，字段排列、预设、高级区域和 draft/apply 仍在功能层；两套业务 props 未合并。
- UI `FormActions` 以 Fragment 输出取消和原生 submit 按钮，集中标签、可选图标、外部 form ID、busy 与取消回调。覆盖来源/账号、三类模型实体、发现字段编辑、设置创建与轮换共五个操作区；没有增加 footer 包装。页面保留防重复、closeDisabled、退出清理及详情转编辑焦点管理，Key 结果与即时清理没有重构。
- 将 `DetailList` / `DetailItem` 提升到 UI 层，控制面与事件共享 `dl/dt/dd`、标签/值样式、长字段换行和窄屏单列。行式用于来源/模型/能力/设置，网格式用于事件基本字段。Token 精确明细、来源质量、完整协议链和 attempt 列表继续由功能层提供；移除被替代的 filter 表面和 detail 样式/JSX。

Web 验证通过：`mise exec -- npm --prefix web run lint`（ESLint、Knip 两种门禁）、`typecheck`、`test`（32 文件 **180 项**）、`build`、`git diff --check`。新增四项行为测试：三类模型实体通过 footer 正确提交到各自 API、busy 禁用并防重复；能力筛选后读取有标签的协议详情。扩充既有回归：普通/高级筛选收起后仍保留草稿、显式应用与重置；事件标签/值与精确数值；来源失败重试和 Key 轮换改用真实外部 submit 按钮。原有日期校验、发现选择清空、来源输入/校验、Key 创建/敏感清理回归均通过。

独立只读审查覆盖公共组合职责、form ID/busy、详情语义与样式删除影响，未发现阻断问题。浏览器使用真实 GatewayManagementPage / GatewayUsagePage 与本地合成 API，无生产配置写入或真实 Provider 调用：

| 实测范围 | 结果 |
| --- | --- |
| 390px 中文浅色来源 | 查看详情→编辑→非法 JSON 校验失败→模拟 409 保存失败保留显示名→重试成功；footer 直接双按钮纵向布局，关闭后焦点返回原查看入口 |
| 390px 英文深色用量 | 普通/高级字段编辑不查询，应用后请求包含模型、Source、missing；重置清空条件。预设和日期校验另由现有行为测试覆盖 |
| 390px 英文深色发现 | 全选仅选 available + pending，unknown 禁用；切换 availability 为 unknown 后清空选择、禁用批量确认 |
| 375px 英文深色能力 | 筛选 degraded→详情，保留三协议、adapter、conversion chain、降级与不可路由错误解释；详情字段无横向溢出 |
| 375px 中文浅色模型 | LogicalModel、Binding、Route 分别通过 footer 保存，Binding 保留 Source/Account/模型归因，Route 修改协议准确进入提交内容 |
| 375px 英文深色设置 | Key 创建与轮换通过 footer 提交并显示独立结果；保留轮换时限，Done 后合成 Key 立即不再出现 |
| 375px 浅色来源及事件、390px 深色事件 | 长 URL/模型、协议链与精确 `1,234,567,890,123` 可读；详情内部滚动，页面无横向溢出；事件关闭后焦点回列表查看按钮 |

截图均为合成数据：[来源长字段](evidence/166/b11-source-375-light.png)、[用量筛选](evidence/166/b11-filters-390-dark.png)、[事件浅色](evidence/166/b11-event-375-light.png)、[事件深色](evidence/166/b11-event-390-dark.png)、[能力详情](evidence/166/b11-capability-375-dark.png)、[Route 编辑](evidence/166/b11-route-375-light.png)、[Key 操作区](evidence/166/b11-key-actions-375-dark.png)。临时验证入口已移出仓库，浏览器视口已恢复。合成大数夹具更新后重启开发服务并重验，未将旧缓存显示计为新数据验证。

Vite 资源合计：JS **938.46 kB / gzip 287.94 kB**，CSS **159.86 kB / gzip 29.05 kB**；相对第十批分别 **−0.08/+0.24 kB**、**−0.48/−0.03 kB**（原始/gzip）。主入口 **533.64 kB / gzip 166.13 kB**，保留默认 500kB warning，构建通过；未做无关分包优化。第十批 mise 安装器修复保持不变。

未覆盖：全八页所有状态/核心流程、全部浮层组合及减少动画浏览器实测、真实屏幕阅读器、生产配置/Provider 验收、帧耗时基准。本地未跑 Rust/PostgreSQL/live，最终 head 的 PR CI 另在 Issue/PR 记录。#166 保持开放；本批和已有 FormGrid/DrawerSection/PageActions 可共同支撑“提炼实际重复的筛选栏、编辑表单、详情分区和操作区”，不据此勾选全局 SCSS、宽度治理或整页完整验收。

## 第十二批：页面状态与查询生命周期验收

基线 main `95ad077`（PR #179 已合并），分支 `codex/166-page-state-acceptance`、PR #180。本批只收敛前端查询身份、刷新失败保留、事件分页/导出反馈和逐页验收证据，不改变 Rust、Admin API、数据库、核算规则或生产配置。

- `useAdminQuery` 增加显式查询身份；模型发现以来源、确认状态和可用状态组成 scope。跨 scope 加载立即隐藏旧目录，同 scope 刷新继续展示已发布快照并在失败后提供重试；没有来源时也保留刷新/错误入口。
- 用量查询将“正在加载的首屏请求”和“已经发布、供分页/导出的快照”拆开。相同查询刷新期间保留旧数据并暂停旧 cursor，成功后原子替换筛选窗口与第一页；刷新失败后继续使用旧窗口/cursor，并恢复分页与导出。切换筛选、页面或认证身份时立即隐藏旧快照，迟到响应不能回写。
- Shell 使用递增 `authGeneration` 标识 Admin Key 应用/清空；查询键只包含数字代次，不包含 session-only 密钥。普通刷新不改变认证代次。实际 App 合成验证中，被拒绝的新 Key 提交后旧事件行立即消失，401 后仍不回显；清空 Key 后重新取得当前身份的数据。
- 事件首屏、追加页和导出分别保留错误与重试状态；失败 cursor 只允许显式重试，CSV/JSON 重试保持原格式与已发布筛选窗口。详情选择绑定当前行身份；已选行在刷新后消失时关闭 Drawer，将焦点移到空状态，数据恢复后移到事件区域且不重新读取/打开旧详情。

本批最终 Web 验证通过：`mise exec -- npm --prefix web run lint`（ESLint、Knip 两种门禁）、`typecheck`、`test`（32 文件 **194 项**）、`build` 与 `git diff --check`。另以 `TZ=America/New_York` 运行 `filterState.test.ts` 和 `useUsageData.test.tsx`，2 文件 **26 项**通过，覆盖滚动时间窗跨时区稳定性。新增回归覆盖查询 scope、迟到响应、同范围刷新失败、旧 cursor 暂停/恢复、发布窗口导出、认证代次、分页/导出失败重试，以及事件 nonempty → empty → nonempty 的详情和焦点生命周期。

逐页证据按“本批实际覆盖”和“未在本批重放”分开记录；以前批次证据仅作为回归背景，不把四张第十二批截图扩写成八页全状态完成：

| 页面 | 本批实际验证与证据 | 未验证或不适用边界 |
| --- | --- | --- |
| 总览 | 实际 App + 本地合成 API 在 390px 英文深色下慢刷新，旧 KPI/图表保持且显示刷新状态；DOM 覆盖首次加载失败、刷新失败保留、空结果和筛选无匹配。[截图](evidence/166/b12-overview-refresh-390-dark-en.png) | 未在浏览器逐一重放浅/深色 × 桌面/窄屏 × 全部空错组合 |
| 用量分析 | 与总览共用首屏/刷新/认证查询生命周期；既有第十/十一批覆盖 375px 深色分析、长字段、筛选应用与重置 | 本批无独立分析页截图；分析页没有导出功能，导出验收为 N/A |
| 请求事件 | 实际 App + 本地合成 API 覆盖保留行的追加页失败/重试；DOM 精确覆盖 CSV/JSON Blob、文件名、失败格式重试和 cursor。最终代码另在实际 App 复核详情打开后刷新为空：Drawer 关闭、焦点进入空状态；恢复行后不重开详情，焦点进入事件区域。认证代次拒绝/恢复同样通过实际 App 合成复核。[截图](evidence/166/b12-events-page-retry-375-dark-en.png) | 内置浏览器未把下载落盘作为证据；文件内容、Blob 和下载名以 DOM 回归为准，不声称完成文件系统下载验收 |
| 来源 | `useAdminQuery` 同 scope 刷新保留和失败重试由控制面行为测试覆盖；既有第六/十一批覆盖来源表、详情、编辑失败重试 | 本批未重新浏览器执行空来源、账号、连接测试、删除和生产提交的全部组合 |
| 模型发现 | 实际 App + 合成 API 在 390px 英文深色切换第二个 Source，加载期间只显示新 scope，不泄露前一来源目录；DOM 覆盖首次失败、空来源刷新及同 scope 失败保留。[截图](evidence/166/b12-discovery-scope-switch-390-dark-en.png) | 本批未在浏览器完整串行执行发现 → diff → 确认写流程 |
| 模型与路由 | 共用控制面刷新保留；既有第十一批分别提交 LogicalModel、Binding、Route 合成表单并核对归因/协议 | 本批无独立截图，未重放三实体全部空态、错误和生产写入 |
| 能力矩阵 | 共用控制面刷新保留；既有第六/十一批覆盖 native/degraded/不可路由、筛选和窄屏详情 | 本批无独立截图，未重放所有协议 × 主题 × 宽度组合 |
| 设置 | 实际 App + 合成 API 桌面浅色覆盖运行时快照、Key 列表和配置导出页面；响应与截图不含密钥正文。[截图](evidence/166/b12-settings-redaction-desktop-light.png) | 当前产品没有 UI 导入入口，导入验收为 N/A；本批不执行生产 Key 创建/读取/轮换/撤销，敏感结果关闭清理由第九/十一批 DOM/浏览器合成证据覆盖 |

Vite 资源合计：JS **942.77 kB / gzip 289.04 kB**，CSS **159.86 kB / gzip 29.05 kB**；相对第十一批分别 **+4.31/+1.10 kB**、**0/0 kB**（原始/gzip）。主入口 **533.82 kB / gzip 166.18 kB**，默认 500kB warning 保留且构建通过；本批没有新增依赖或进行无关分包优化。

生产配置写入、真实 Provider 调用、屏幕阅读器人工朗读和帧耗时基准是本批未执行的环境/人工验证披露，不是 PR #180 新增的关闭条件。该 PR 只以本文列明的查询生命周期、状态恢复和代表性逐页证据为验收边界；#166 仍保持开放，后续是否关闭由 Issue 中原有未完成项和独立证据决定。

## 第十三批：最终组件审计与适用交互收尾

基线为 PR #180 合并后的 main `8eef98f`，分支 `codex/166-ui-closure`、PR #181。本批不改变 Rust、Admin API、数据库、核算规则、导航入口或业务 schema，只收敛前端组件、遗留样式和第十二批之后仍适用的真实浏览器交互。

### 组件与样式审计

- 生产源码中的 26 个 `SelectField` 调用点及 `UsageTrend` 的直接下拉全部前进迁移到 Mantine Select：调用方传 `data` 和受控字符串值，不解析旧 `<option>`、不合成 DOM change 事件，也不保留 NativeSelect 兼容层。主题统一配置 Portal、z-index、flip/shift、120ms 过渡与品牌 dropdown/option；按内部依赖补入 `ScrollArea.css`。生产与测试源码均无 `NativeSelect`、`<select>`、`<option>` 或 `HTMLSelectElement` 残留。
- 全组件扫描未发现生产源码直接编写的 button/input/select/textarea/details/summary/progress、手写 Portal/overlay，或旧 PortalTooltip/QuestionMarkHelp。当前公共体系覆盖 Button、ActionIcon/Tooltip、TextInput/Select/Textarea、Checkbox/Switch/Tabs、Alert/Loader/Paper/Badge、Modal/Drawer/Popover、Table/Progress/NavLink/Notifications。LanguageSwitcher、SegmentedTabs、TableScroll、FormGrid、DrawerSection、PageActions 和 DetailList 是仍有项目语义的组合，保留而不机械包装成新的 Mantine 抽象。
- 静态导入与选择器调用核对后删除无生产调用的 `components.scss`、`layout.scss`、`themes.scss`、`mixins.scss`，共 1,374 行；仍实际生效的卡片/控件尺寸与文字 Token 迁入 `gateway-brand.scss`。同时删除无调用的全局 utility/fade、刷新条动画和被更宽断点/父布局覆盖的重复响应式规则。最终加载顺序为 reset → 品牌 Token → Mantine 按需样式及依赖 → CSS Modules；`ConsolePrimitives.module.scss` 仅服务 SegmentedTabs。
- 禁用 IconButton 改用 Mantine 官方可提示模式：保留焦点与 Tooltip，暴露 `aria-disabled` / `data-disabled`，公共入口阻止按钮动作及父行 click。Modal 的 `closeDisabled` 显式同时关闭 close button、Esc 与 backdrop 三条关闭路径。
- Virtual Key 创建/轮换成功后先退出表单 Modal，再由 `onExitTransitionEnd` 打开结果 Modal；结果获得焦点，关闭后返回稳定触发器，Key 正文即时从 DOM 清除。避免同一 render 同时卸载焦点来源和挂载敏感结果。

### 实际 App 浏览器矩阵

Chrome 通过 Vite 运行最终源码，并经本地合成 API 覆盖可逆写流程；没有连接生产数据。先在 1900px 桌面、再在 390px 英文深色逐一进入八个真实 hash 路由，标题、一级标题和导航选中态一致，关闭残留浮层后各页根宽度不超过视口；宽表仅在命名的局部滚动区滚动。最终状态证据精确复用前批结果，并以本批实际重放补齐交互，不把单张截图扩写成笛卡尔积验收：

| 页面 | 最终采用的证据范围 |
| --- | --- |
| 总览 | 第十批桌面/390px 指标与图表；第十二批 390px 英文深色慢刷新、错误/空态与查询身份 |
| 用量分析 | 第十批 375px 深色构成/分布，第十一批长字段和筛选；该页无导出功能（N/A） |
| 请求事件 | 第七批虚拟列表测量/滚动，第十/十二批详情与分页恢复；本批重放列 Popover、详情滚动及 CSV/JSON 实际落盘 |
| 来源 | 第六/十一批表格、详情和长字段；本批重放 Tooltip、详情→编辑、Modal/Drawer 内 Select、禁用项与 Esc 层级 |
| 模型发现 | 第九批失败反馈与第十二批来源 scope 切换/刷新恢复；本批确认 Mantine Select 接入无旧数据泄漏或交互回退 |
| 模型与路由 | 第六批三表格和第十一批三实体表单/Route 编辑；本批确认全部下拉调用已迁移并可进入 |
| 能力矩阵 | 第六/十一批 native/degraded/不可路由、筛选与窄屏详情；本批确认筛选/编辑下拉已迁移并可进入 |
| 设置 | 第九/十一/十二批 Key 结果、操作区和脱敏导出；本批重放 pending 撤销、禁用 Tooltip、创建/轮换焦点与敏感 DOM 清理 |

适用浮层组合的本批结果：

| 组合 | 实际结果与证据 |
| --- | --- |
| Select in Modal / Drawer | 桌面英文深色和 390px 深色下由 Mantine 绘制，长文案、disabled `Unknown`、键盘选择和 Portal 均未被正文裁剪；第一次 Esc 只关闭 Select，第二次关闭父浮层。[来源 Drawer 截图](evidence/166/b13-source-select-drawer-desktop-dark-en.jpg) |
| Popover at 390px edge | 事件列设置在视口内完成 flip/shift，Shift+Tab 在交互内容内循环，勾选即时同步列；Esc 或外部点击关闭并把焦点还给触发器，随后详情 Drawer 可滚动并返回对应行。[截图](evidence/166/b13-events-popover-mobile-dark-en.jpg) |
| busy Modal | 撤销请求延迟期间 close button、Esc、backdrop 和重复提交均不关闭/不重发，请求日志只有一次 POST；完成后正常退出。创建/轮换按“来源 → 结果”串行交接焦点，截图不保存 Key 正文 |
| disabled Tooltip | 桌面表格 Tooltip 不被 TableScroll 裁剪；390px 英文深色禁用行操作仍可聚焦并显示原因，但不会执行或触发行点击。[桌面](evidence/166/b13-source-tooltip-desktop-light.jpg) / [移动](evidence/166/b13-settings-disabled-tooltip-mobile-dark-en.jpg) |
| mobile navigation Drawer | 390×844 浅色下焦点约束、body scroll lock、关闭卸载和无整页横向溢出通过。[截图](evidence/166/b13-mobile-navigation-drawer-light.jpg) |
| reduced motion | 通过 Chrome DevTools 实际启用 `prefers-reduced-motion: reduce`，页面 `matchMedia` 为 true，Modal content/overlay 过渡时长均为 0s，焦点进入和 Esc 返回仍正常；随后恢复默认模拟状态 |

当前生产代码没有 Menu、Popover 内 Select、Modal 内 Popover 或 Drawer 内二次确认，因此这些组合为 N/A；不为验收构造不存在的产品层级。事件 CSV（184 B）与 JSON（3,209 B）由真实浏览器下载到文件系统并检查文件名、格式与内容，补齐第十二批只验证 Blob 的披露。UI 合成写操作不代表生产 Key、配置或 Provider 验收。

### 验证与边界

最终 Web 验证通过：`npm run lint`（ESLint、Knip、production Knip）、`npm run typecheck`、`npm run test`（32 文件 **198 项**）、`npm run build` 和 `git diff --check`；其中组件/表单定向回归为 6 文件 **55 项**。生产源码扫描无上述原生控件与旧浮层残留。

Vite 资源合计：JS **960.74 kB / gzip 293.96 kB**，CSS **143.06 kB / gzip 25.62 kB**；相对第十二批分别 **+17.97/+4.92 kB**、**−16.80/−3.43 kB**（原始/gzip）。主入口 **567.00 kB / gzip 175.77 kB**，保留默认 500kB warning，构建通过；增长来自将浏览器原生下拉改为 Mantine Select，CSS 减少来自确认无调用的旧全局层。本批没有新增依赖或进行无关分包优化。

未执行 Rust/PostgreSQL/live Provider、生产配置/密钥写入、屏幕阅读器人工朗读或帧耗时基准。它们仍是环境/人工验证披露，不是本批新增加的关闭条件；第十三批的完成范围是 #166 剩余前端代码审计、适用交互和合成 API 浏览器验收。

## 第十四批：逐页业务状态证据闭环

基线为 PR #181 合并后的 main `c1118db`，分支 `codex/166-page-browser-evidence`、PR #182。本批只补评审明确指出仍缺少的真实 App 浏览器状态；没有改变前端、Rust、Admin API、数据库或业务契约。Chrome 直接运行最终源码，经本地内存合成 Admin API 在 1440×900 中文浅色和 390×844 中文深色下重放；合成服务不读取生产配置或凭据，不发送真实 Provider 请求。

本批的控制条件、浏览器操作、页面观察与请求日志如下。这里的“请求日志”来自合成服务按顺序记录的实际浏览器 HTTP 请求，不把直接设置夹具状态的 `GET /__control` 计作产品行为。

| 页面/流程 | 控制条件与实际浏览器操作 | 可见结果与请求证据 | 截图 |
| --- | --- | --- | --- |
| 来源连接测试 | 普通来源与账号；打开 `source-browser` 详情，依次点击 Chat Completions、Responses 的“测试” | 两次 `POST /admin/sources/source-browser/connection-tests` 均带 `account_id=account-browser` 与 `requested_by=admin-ui`；Chat 返回 200/88 ms 并显示“连接成功”，Responses 返回 503/241 ms、`synthetic_upstream_unavailable` 并显示“连接失败”，两项结果同时保留 | [成功/失败并存](evidence/166/b14-source-connection-results-desktop.jpg) |
| 来源/账号空态 | `sourcesMode=empty`、`accountsMode=empty` 后重新进入来源页并切换两个真实 Tab | `GET /admin/sources`、`GET /admin/accounts` 分别返回空数组；Tab 计数均为 0，来源显示“尚未配置来源”，账号显示“尚未配置账号”，且没有来源时“新增账号”保持禁用 | [来源空态](evidence/166/b14-source-empty-desktop.jpg) · [账号空态](evidence/166/b14-account-empty-desktop.jpg) |
| 来源/账号删除 | 普通数据下分别点击来源、账号删除入口；先核对标题、目标 ID、危险操作和取消路径，再经单独动作时授权逐项确认删除 | 确认框分别绑定 `source-browser` 与 `account-browser`；取消不发请求且值保持。确认后分别只产生一次 `DELETE /admin/sources/source-browser`、`DELETE /admin/accounts/account-browser`，对应内存数组变为空，页面计数变 0、显示各自空态和成功通知；每项核对后重置夹具，未串联伪造依赖级联 | [来源确认](evidence/166/b14-source-delete-confirm-desktop.jpg) · [账号确认](evidence/166/b14-account-delete-confirm-desktop.jpg) |
| 模型发现完整链 | 从无运行记录开始点击“运行发现”，查看 diff，选择全部两个可确认模型，第一次确认强制返回冲突，再在原确认框重试 | `POST /discoveries` 后显示新增 2、变更 1、缺失 1 及两条待确认目录。两次 `POST /admin/sources/source-browser/models/confirm` 请求体完全一致，均包含 alpha/beta 与空 metadata；第一次 409 `synthetic_confirm_conflict` 时确认框、`已选 2` 和两个勾选均保留，第二次成功后确认框关闭、提示确认 2 个模型、待确认筛选原子变为 0 | [运行与 diff](evidence/166/b14-discovery-run-diff-desktop.jpg) · [失败保值](evidence/166/b14-discovery-confirm-failure-retain-desktop.jpg) · [重试成功](evidence/166/b14-discovery-confirm-retry-success-desktop.jpg) |
| 模型与路由空态 | `catalogMode=empty` 后进入模型页，依次切换逻辑模型、绑定、路由规则 | 三个列表 GET 均返回空数组；三个 Tab 计数为 0，并分别显示“尚无逻辑模型”“尚无绑定”“尚无路由”及各自下一步说明 | [逻辑模型](evidence/166/b14-models-logical-empty-desktop.jpg) · [绑定](evidence/166/b14-models-binding-empty-desktop.jpg) · [路由](evidence/166/b14-models-route-empty-desktop.jpg) |
| 模型与路由刷新错误 | 先发布含 `logical-browser` 的正常目录，再令三列表刷新返回 503 并点击页面“刷新”，随后恢复服务并点击错误区“重试” | `catalog_refresh_failed` 作为持久错误显示，Tab 仍为 1/1/1，既有 `logical-browser` 行未被清空；重试成功后错误消失且同一行继续可见 | [错误保留快照](evidence/166/b14-models-refresh-error-retain-desktop.jpg) |
| 三类模型资源删除 | 普通数据下依次进入逻辑模型、绑定、路由规则 Tab；先核对标题、ID、不可撤销说明和取消路径，再经同一动作时授权逐项确认删除 | 确认框分别绑定 `logical-browser`、`166`、`route-browser`；取消均不发 DELETE。确认后分别只产生一次 `DELETE /admin/logical-models/logical-browser`、`DELETE /admin/model-bindings/166`、`DELETE /admin/routes/route-browser`；对应数组、Tab 计数和列表原子变空，页面显示专用空态与成功通知。每项独立 reset 后执行，避免上一删除影响下一项 | [逻辑模型](evidence/166/b14-model-logical-delete-confirm-desktop.jpg) · [绑定](evidence/166/b14-model-binding-delete-confirm-desktop.jpg) · [路由](evidence/166/b14-model-route-delete-confirm-desktop.jpg) |
| 能力矩阵无结果/无快照 | 正常快照输入 `no-matching-capability`；再令 `/admin/capabilities` 返回空 data | 筛选态准确显示 `0 / 1 行` 与“当前筛选没有能力行”；空响应保留 snapshot revision/fact source 元数据并显示专用“暂无已发布的模型能力”，不误写成筛选无结果 | [筛选无结果](evidence/166/b14-capabilities-filter-empty-390-dark.jpg) · [无快照](evidence/166/b14-capabilities-no-snapshot-390-dark.jpg) |
| 能力矩阵刷新错误 | 先发布 revision 166、`route-browser` 正常行，再令刷新返回 503；随后恢复并点击“重试” | `capability_snapshot_unavailable` 与重试入口出现时，revision 166、`1 / 1 行` 和既有能力行仍可见；重试后错误消失 | [错误保留快照](evidence/166/b14-capabilities-refresh-error-retain-390-dark.jpg) |
| 用量分析慢首屏/错误/空态 | `usageDelayMs=8000` 时首次进入分析页；再令全组 usage 请求返回 503；恢复为空数据并点击错误区“重试” | 慢请求期间只出现一次页面加载状态；失败时保留筛选控件并显示本地化持久错误和重试；重试后发布零 summary/空 breakdown，显示“当前范围暂无分析数据”而不构造图表或 KPI | [慢首屏](evidence/166/b14-analysis-initial-loading-390-dark.jpg) · [错误/重试](evidence/166/b14-analysis-error-retry-390-dark.jpg) · [空态](evidence/166/b14-analysis-empty-390-dark.jpg) |
| 设置慢首屏/错误/空 Key | `keysDelayMs=8000` 时首次进入设置；再令 `GET /admin/keys` 返回 503；恢复为空数组并点击“重试” | 慢请求显示统一页面加载；失败显示 `keys_unavailable` 和重试且不发布半成品页面；成功重试后管理连接、revision 166 运行时快照和脱敏导出正常发布，Virtual Key 区显示“尚无虚拟密钥” | [慢首屏](evidence/166/b14-settings-initial-loading-desktop.jpg) · [错误/重试](evidence/166/b14-settings-error-retry-desktop.jpg) · [空 Key](evidence/166/b14-settings-empty-keys-desktop.jpg) |

总览和请求事件没有在本批重复制造截图：总览的慢刷新、错误、空态和查询身份由第十二批真实 App 证据覆盖；事件的分页/导出失败重试、详情失效与焦点恢复由第十二批覆盖，文件落盘和 Popover/Drawer 实交互由第十三批覆盖。本批补上用量分析的独立三状态，因此不再以总览共用 hook 推断分析页呈现。

### 对 #166 原始未勾项的最终映射

| 原始验收组 | 组成证据 | 当前结论 |
| --- | --- | --- |
| 浮层 Portal、Esc、外部点击、焦点与滚动 | 第八批应用壳/事件组合、第十三批 Select/Popover/busy Modal/移动 Drawer/减少动画实测，以及本批五类删除确认框 | 当前生产代码中实际存在的组合均有浏览器证据；明确不存在的 Menu、Popover 内 Select、Modal 内 Popover、Drawer 内二次确认为 N/A |
| 页面 SCSS、宽度/间距/主滚动与旧实现清理 | 第十/十一批宽表与长字段，第十三批 1,374 行无调用全局 SCSS 清理、生产源码扫描和 1900px/390px 八路由遍历 | 前端源码范围已收敛，没有未说明的旧组件或通用页面样式残留 |
| 首次加载、刷新、空数据、筛选无结果、错误、重试、操作结果与上下文保留 | 第十二批查询身份/迟到响应/分页导出生命周期，加本批来源、发现、三目录、能力、分析和设置逐页状态 | 真实 App + 合成 API 已覆盖原始要求；失败保值与重试请求均有页面观察和请求日志，不再只依赖 DOM 单测 |
| 八个页面与应用壳 | 总览 B10/B12；分析 B10/B11/B14；事件 B7/B10/B12/B13；来源 B6/B11/B13/B14；发现 B9/B12/B13/B14；模型与路由 B6/B11/B13/B14；能力 B6/B11/B13/B14；设置 B9/B11/B12/B13/B14；应用壳 B8/B12/B13 | 每个真实路由均有迁移结果和对应业务状态/交互证据，没有以一张截图扩写成未执行的主题、宽度或状态笛卡尔积 |
| 导航、筛选/选择、表单/确认、列设置、详情、导出及主题/语言 | 第八至十三批行为与实际 App 证据，本批再补发现串行写流程、筛选无结果与删除目标确认 | 适用入口均已重放；用量分析导出、设置导入以及不存在的浮层组合继续明确为 N/A |
| 公共组件约定与新增页面指导 | `design.md`、`docs/frontend-architecture.md`、本清单职责矩阵，以及第十三批公共入口/生产调用扫描 | 常规控件无需复制页面内部样式；专业表格、虚拟列表和 Chart.js 的保留边界已有说明 |

本批未执行生产配置或密钥写入、真实 Provider、Rust/PostgreSQL、屏幕阅读器人工朗读或帧耗时基准。它们继续作为环境/人工边界披露，不由合成 API 证据推断。#166 是否勾选或关闭仍以本批 PR 合并和 Issue 协调结论为准；本分支不直接合并或关闭 Issue。
