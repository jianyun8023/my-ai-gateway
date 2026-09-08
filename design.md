# 前端设计与组件规范

适用于 `/admin/` 控制台。领域模型与协议规则见 [架构设计](docs/ai-gateway-design.md)，开发命令见 [前端 README](web/README.md)。

## 视觉风格

Tech-Utility：冷灰底色、绿色强调、紧凑数据布局。使用固定侧栏、顶部工具栏、清晰标题与细边框卡片，以 Token、请求状态和模型归因为重点。普通卡片不使用浮动阴影。

页面文案只保留操作、状态和必要提示。标题足以说明内容时不再添加副标题；空态给出下一步；接口路径、存储方式、实现过程放在开发文档中。中文与英文同步维护，协议、模型名和 ID 保留原文。

## Token 与尺度

品牌 Token 统一维护在 [gateway-brand.scss](web/src/styles/gateway-brand.scss) 的 `:root`，页面、Mantine CSS 变量和 Portal 共享继承。主题选择与持久化仍由 `useThemeStore` 唯一管理，`ConsoleProvider` 将 `resolvedTheme` 传给 Mantine 的 `forceColorScheme`，不建立另一套主题存储。原生控件通过 `color-scheme` 跟随主题。

| 用途 | Token / 约定 |
| --- | --- |
| 背景与表面 | `--bg` 页面背景、`--surface` 卡片与浮层、`--border` 边框 |
| 文字 | `--fg` 主文字、`--muted` 辅助文字 |
| 状态 | `--accent` 强调、`--success` 成功、`--warn` 警告、`--danger` 失败；浅背景由 `color-mix` 派生 |
| 字体 | 系统 sans 优先；ID、协议和数值用 mono；数值使用 `tabular-nums` |
| 字号 | 页面标题 28px、卡片标题 16px、正文 14px、控件与标签 12px、紧凑元数据 11px |
| 间距 | 4 / 8 / 16 / 24 / 32px；普通卡片内边距 20px |
| 圆角 | 小控件 6px、常规容器 8px、卡片与弹窗 12px；状态标签为胶囊 |
| 控件 | 桌面普通按钮 36px、小按钮 32px；触摸布局普通按钮、表单和分段控件至少 44px |

现有 `--keeper-*`、`--text-*` 等变量由品牌层映射，页面不重复定义。Mantine 字体、间距、断点、组件默认值及语义变量映射集中在 [theme.ts](web/src/components/ui/theme.ts)。`themes.scss` 暂为未迁移组件保留基础默认值；不再新增另一套主题定义。旧 `index.css` 中的重复主题已删除。

加载顺序固定为全局 reset/旧组件样式 → 品牌 Token → [Mantine 按需样式](web/src/styles/mantine.css) → Root 引入的组件 CSS Modules。新增 Mantine 控件时在该入口补充其样式及基础依赖；不要在页面导入全库样式或依赖加载顺序覆盖公共交互。

## 组件职责

| 组件 | 用法 |
| --- | --- |
| `GatewayConsoleShell` | 导航、页面标题、Admin 连接、主题、语言和刷新 |
| `Card` | Mantine Paper/Title/Text 组合；标题、可选说明与操作区，`flush` 用于表格，沿用品牌尺度并在窄屏堆叠操作区 |
| `MetricCard` | Mantine Paper/Text 薄组合；标签、数值、辅助信息及可选精确值；精确值通过 title/可访问名称提供，网格列数由页面维护 |
| `Button` / `IconButton` | Mantine Button/ActionIcon，主次操作、禁用、加载与可访问名称；图标提示用 Tooltip，提交按钮显式设置 `type="submit"` |
| `TextField` / `SelectField` / `TextAreaField` | Mantine TextInput/NativeSelect/Textarea，共用 label、hint、error 关联；保留调用方描述 ID 和原生 change 事件 |
| `CheckboxField` | Mantine Checkbox 的表单组合，label/hint、禁用与布尔值回调；位于 UI 层，控制面共享入口仅转导出 |
| `StatusPill` | Mantine Badge；success / warning / danger / accent / muted，页面负责业务映射；保留原始大小写、完整文本与长标签换行 |
| `SegmentedTabs` | Mantine Tabs 负责内容面板键盘导航；`mode="group"` 使用 Mantine Button 保留 `aria-pressed` 筛选语义 |
| `LanguageSwitcher` / `Toggle` | 语言使用按压按钮并沿用既有持久化；启用状态使用 Mantine Switch，布尔值回调与禁用状态由页面控制 |
| `notifySuccess` / `Notifications` | ConsoleProvider 中唯一通知宿主，普通操作成功短暂显示；持久结果与错误按下文反馈规则处理 |
| `LoadingState` / `Notice` / `EmptyState` | Mantine Loader/Alert/Paper 组合；错误使用 alert，成功/警告使用 status；Loader 装饰化并由外层提供一次加载播报，局部加载用 inline；空表使用 centered 并保留下一步操作 |
| Mantine `NavLink` | Shell 使用 button 语义保留 hash 导航，选中态对应 `aria-current=page`；视觉与触摸尺寸由 UI 主题统一 |
| Mantine `Table` / `TableScroll` | 原生表格语义与带名称、可聚焦的横向滚动区；主题统一单元格密度、表头与分隔线，页面负责列宽和行操作 |
| `Modal` | Mantine Modal/Drawer 的项目契约：标题、尺寸、底部操作、关闭禁用、退出回调；管理详情、事件详情和移动导航共用 |
| `overlays.ts` | 直接导出 Mantine Popover/Checkbox，列偏好在公共主题下使用，无需机械包装 |

通用组件在 `web/src/components/ui`，不反向依赖 API 或控制面模块。浮层布局由 [Overlay.module.scss](web/src/components/ui/Overlay.module.scss) 管理，行为交给 Mantine；其余尚未迁移组件继续使用 [ConsolePrimitives.module.scss](web/src/components/ui/ConsolePrimitives.module.scss) 与 [components.scss](web/src/styles/components.scss)。`features/control-plane/shared.tsx` 保留协议标签、错误展示、确认流程和业务布局；协议常量、错误归一化与格式化分别由 `lib/protocols.ts`、`admin-api/errors.ts` 和 `utils/format.ts` 提供。分层约束见 [前端架构](docs/frontend-architecture.md)。

按钮、图标和字段的品牌尺寸、状态样式集中在 [Controls.module.scss](web/src/components/ui/Controls.module.scss)，通过 Mantine Styles API 和语义变量接入。字段使用 Mantine Input.Wrapper 的 label/description/error；`attributes.input` 合并调用方描述 ID 与生成的 hint/error ID，不覆盖业务输入值。`SelectField` 采用 NativeSelect 保留 option/optgroup、禁用项和浏览器菜单交互，不新增搜索能力或模拟 change 事件。旧 `.btn`、手写字段框及控制面复选框样式已删除；页面操作布局按 `data-ui="button"` 定位，不能恢复旧按钮视觉类。

页面 SCSS 只维护布局与领域视觉。配置表格直接使用 Mantine Table 及其 Thead/Tbody/Tr/Th/Td，表头声明 `scope="col"`；基础视觉集中在 [Table.module.scss](web/src/components/ui/Table.module.scss)，默认单元格内边距为纵向 10px、横向 13px。TableScroll 保留原生、可聚焦的局部滚动，页面只指定领域列宽与最小宽度。可点击行仍保留表格语义，并提供可通过键盘访问的“查看”按钮；开关和操作单元格阻止事件冒泡，避免误开详情。事件虚拟列表也使用 Mantine Table 主题，继续由 TanStack Virtual 管理范围与行高：单一原生滚动区包含 sticky 表头和虚拟 tbody，以稳定事件标识缓存测量，通过 `measureElement`/`data-index` 响应尺寸变化。表头固定 44px，CSS 与 `scrollMargin` 共享同一常量，行位置扣除表头偏移。查看按钮放在首列供窄屏与键盘访问，数据单元格点击同样打开详情；虚拟表声明完整行索引，尚有下一页时总行数使用未知值。不要用全量 DOM 或第二套滚动容器替代此实现。

## 交互与响应式

- 主操作使用 primary，次级操作使用 secondary/ghost；删除等危险操作说明后果并确认。
- 图标按钮必须有名称；加载按钮禁用并暴露 `aria-busy`；可交互元素提供 `focus-visible`。
- 字段错误同时使用文字、边框和 `aria-invalid`，hint/error 通过 `aria-describedby` 关联。
- Tabs 支持方向键、Home/End、单一 Tab 停靠点与关联 panel；筛选按钮使用 `aria-pressed`。
- 状态不能只靠颜色表达；`unknown` / `unsupported` 不显示为成功。估算与缺失用量分别标记。
- 长模型名、URL、英文文案允许换行；控制台 flex / grid 子项允许收缩，宽表只在表格容器内滚动。
- 用量默认“今天”，同时提供“昨天”和最近 24 小时等预设；自然日按浏览器本地时间计算，窄屏预设按钮允许换行。

| 断点 | 布局变化 |
| --- | --- |
| ≤920px | 侧栏变抽屉，扩大触摸控件，宽表横向滚动 |
| ≤600px | 筛选与表单以单列为主，卡片操作区和通知操作换行 |
| ≤380px | KPI 单列，导航和间距进一步收缩 |

遵循 `prefers-reduced-motion`。维护交互时检查键盘、浅色/深色、窄屏、长文案和 Portal；复用现有页面与组件回归测试。

## Mantine 浮层契约与迁移状态

本批使用 Mantine **9.6.0**（core/hooks/notifications 精确锁定，React 19.2 兼容）。基础组件负责交互，公共组合负责项目契约，业务组件负责数据与提交。完整八页清单、批次和证据见 [迁移记录](docs/mantine-migration.md)。目前已迁移主题、浮层、公共按钮/字段、页面筛选、标签与语言切换、启用开关、发现选择框、反馈/状态、卡片、图表和控制面表格；虚拟事件列表与全页面完整验收仍待收敛。

- Modal/Drawer 默认层级 1000，由 Mantine stack 按打开顺序递增；Popover 1200、Tooltip 1300、通知 1400，统一在 theme.ts 修改。ConsoleProvider 通过 Mantine 公开的两种 StackContext 共享同一 stack，跨类型叠加时仅顶层处理 Esc 和焦点约束。条件卸载的详情会注销 stack 条目。
- 焦点恢复使用 Mantine useFocusReturn，与 stack 的 trapFocus 切换分离；条件挂载详情先完成关闭态挂载，再打开。不要在页面添加 focus 定时器。正文单独滚动，标题和底部操作保持可见；长 ID 可换行。关闭动画中的内容通过 inert 退出交互。
- 对话框默认 520px，各业务通过 width 表达尺寸；600px 以下 Drawer 全宽。移动侧栏用左侧 Drawer，桌面保留导航内容；920px 以下隐藏的导航从可访问树移除。
- closeDisabled 同时保护关闭按钮、Esc 和遮罩点击，提交按钮仍由业务 busy 防重复触发。没有提交中的普通浮层允许 Esc 和外部点击关闭。
- 非敏感编辑/确认数据使用 useOverlayState：setValue(record) 打开、setValue(undefined) 开始关闭，把 afterExit 传入 onExitTransitionEnd 才清空数据，避免关闭过程中标题/表单跳变。敏感 Key 使用原有即时清除流程。
- 来源/实体详情切换编辑时，先关闭详情并恢复焦点，再在 onExitTransitionEnd 中打开编辑。不要直接卸载正在持有编辑按钮的详情，否则编辑关闭后无法返回有效入口。
- 列偏好 Popover 使用 Portal、视口自动定位和 focus trap；交互内容使用 Popover，纯文本提示使用 Tooltip。公共字段选择器已采用 Mantine NativeSelect，保留浏览器菜单；用量与控制面页面筛选及 Shell 密钥输入已共用 Mantine 字段，页面仅保留布局。未来 Mantine Select 与嵌套 Popover 接入须按官方 Portal/事件规则单独验证，当前批次没有宣称这些组合已完成。

新增页面的评审需检查：复用组件入口和主题；label/hint/error 与提交契约；首次加载/刷新/错误/空态；键盘和关闭焦点；双主题、长文案与窄屏；图表/虚拟列表测量及资源体积。专业组件继续保留 Chart.js/TanStack Virtual，主题与数据语义验收不能省略。CPA Usage Keeper 的既有 MIT License 与来源说明继续保留。

用量图表使用 [UsageTrend](web/src/features/usage/UsageTrend.tsx) 保留 Chart.js 的时间序列绘制；默认 Input/Output 堆叠，其他指标使用有名称的独立双轴，并提供精确数值表。Canvas 配色由 [useChartTheme](web/src/features/usage/useChartTheme.ts) 读取品牌 Token，响应已有主题状态；分布采用 Mantine Progress，来源延迟与趋势数据采用 Mantine Table。Token 构成使用一张 Progress 分段图展示 Input/Output 占两者合计的比例，推理和缓存放在数值明细中，不参与图中归一化；上报 Total 独立保留，合计不一致时明确说明。原型差距、数据依据和本批取舍见[图表核对记录](docs/chart-prototype-review.md)。


### 应用壳与导航

`GatewayConsoleShell` 统一八页导航、标题和顶栏。NavLink 的排版、选中态、焦点和减少动画规则维护于 `components/ui/Navigation.module.scss`，桌面导航至少 40px，移动导航至少 44px；页面不复制导航按钮样式。菜单、主题、刷新使用公共 IconButton，刷新中由 Mantine ActionIcon 禁用重复点击。页面一级标题采用 Mantine Title。

Shell 保留原生 CSS Grid/Flex 布局与现有 hash 导航、刷新版本及 session-only Admin Key 契约。大于 1280px 顶栏单行，921–1280px 将连接信息放到第二行；不挤掉标题或隐藏操作。920px 及以下由公共 Drawer 承载导航、语言和连接信息，只有一份连接字段被渲染。草稿受同一 state 控制，切换布局/开关导航不自动应用，显式应用或 Enter 后才清空草稿并触发刷新。桌面侧栏可独立纵向滚动，移动侧栏沿用公共 Drawer 的内容滚动和焦点规则。主题按钮根据 resolvedTheme 决定文案及切换目标，兼容跟随系统深色。

### 通知与持久反馈

- 普通 CRUD、保存、运行时重载及导出完成调用 UI 的 `notifySuccess(message)`，不再维护页面 `SuccessNotice`。开始下一次操作先 `clearOperationNotification()`，防止旧成功与新失败并存。一个操作只反馈一次；通知不放密钥、字段错误、重试按钮或需要持续阅读的业务结果。
- `ConsoleProvider` 只挂载一份 Mantine Notifications，最多一条、右上角 400px 上限、5 秒自动关闭，悬停时由 Mantine 暂停。关闭按钮有中英文名称、窄屏至少 44px；消息允许换行。视觉由 UI Notifications 样式与品牌变量统一，Portal 层级 1400 高于 Modal/Drawer，不抢输入焦点；180ms 过渡遵守 Mantine 的系统减少动画偏好。
- 页面加载/刷新失败、字段及提交失败继续用 `Notice` / `ErrorState` / `FormError`，保留错误码、重试、已有目录和表单草稿。保存成功通知仅表示写操作已完成，后续列表刷新失败仍由持久错误独立说明。
- 模型发现的 succeeded / failed / unsupported 都交给“最新运行”展示，不另发操作成功通知。结果刷新失败只显示可重试错误，保留已有目录，不提前宣告发现成功。协议能力保存保持编辑框打开，统一通知在浮层上方可见；行错误保留在对应编辑区。批量确认提交错误位于确认框内。
- Virtual Key 创建、读取、轮换以结果 Modal 为唯一结果入口，复制状态只在 Modal 内播报，不把 Key 正文放入 live region 或通知。轮换结果保留旧 Key 最晚有效时间（取重叠期和原到期时间的较早者）或立即失效说明；复制不清除时限。关闭即清除 Key 与复制状态，不等待退出动画。

## 用量指标、数值与来源状态

总览四项 KPI 使用 `MetricCard`，页面只保留网格布局；标签和辅助信息可换行，数字使用 mono 与 tabular-nums。`utils/formatCompact.ts` 保留既有 K/M/B/T 阈值，百分比统一由 `formatPercent` 接收比率并输出一位小数。`features/usage/formatters.ts` 的 `formatDuration` 用于总览、分析和事件：真实 0 为 `0 ms`，无值为 `—`，概览使用 ms/s，详情使用精确整数毫秒，不使用 K/M/B。详情 Token 使用带分隔符的精确数值。

请求结果由 `UsageStatus` 映射公共 `StatusPill`，成功/失败必须有文字，不能只靠颜色点。`UsageBadge` 单独表达 upstream / parsed / estimated / missing / unknown；成功请求也可能 missing，不能用来源推导请求成功率。事件 missing/unknown 的零 Token 显示 `—` 或来源标签并提供说明，上游真实上报的零仍显示 0，unknown 的非零读数保留并标注来源不确定。

`GatewayUsageClient.summary` 每次并行读取 summary 与一条 `breakdown=usage_source`，共享完整 filters 和 AbortSignal，经 adapter 用 `key/logical_requests` 组装来源计数。两项都成功才发布 summary，任一失败进入既有错误/重试流程；组件、分页和导出不额外查询来源。`useUsageData` 继续丢弃已取消会话的迟到响应。前端同筛选、同轮发布不等于后端数据库事务快照一致。

总览和分析的构成区域显示真实来源请求计数及 missing/estimated 解释。全部计数来源均缺失/未知且汇总为记账零时，KPI、构成和分布显示不可用说明与 `—`；混合来源保留已有汇总值，明确估算已计入、missing 未计入。独立 Total、Input/Output、推理和缓存语义不变，趋势仍展示原有记账数据，不依据来源重算 Token。

## 筛选、编辑操作与详情组合

- `FilterPanel` 以 Mantine Paper 提供命名 section、品牌表面/边框、12px 内边距与圆角，以及子项收缩边界。控制面的 `FilterBar` 只排列发现/能力字段；用量的 `FilterBar` 保留预设、普通/高级筛选布局与 draft/apply 回调。预设立即应用，普通字段显式应用，日期校验仍由用量功能层维护。
- `FormActions` 返回取消/提交两个按钮的 Fragment，标签、可选提交图标、`form` ID、busy 和取消回调由页面传入。原生 `type="submit"` 关联现有表单；busy 禁用按钮。它不拥有表单值、校验、请求或浮层状态。页面仍传入 `closeDisabled`，执行防重复、退出清理与详情转编辑焦点契约。来源/账号、三种模型实体、发现编辑、Key 创建/轮换共用此组合；不改变敏感 Key 的即时清理。
- UI 的 `DetailList` / `DetailItem` 统一 `dl/dt/dd`、标签/值、长字段换行和窄屏单列。默认行式用于来源、模型、能力、设置；`layout="grid"` 用于请求事件基本字段。Token 精确明细、来源质量和 attempt 列表由用量功能层维护。领域协议标签与转换链继续由调用方提供。
- 已有 `FormGrid`、`DrawerSection` 和 `PageActions` 继续负责控制面表单布局、详情/长表单分区和页操作排列；新表单复用 `SourceForm` 中的字段与分区组合方式，不复制其 CRUD 或生命周期。通用 UI 不包含 schema、mutation/query、通知或浮层生命周期。
