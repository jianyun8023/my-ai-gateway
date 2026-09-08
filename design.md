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
| `Button` / `IconButton` | Mantine Button/ActionIcon，主次操作、禁用、加载与可访问名称；图标提示用 Tooltip，提交按钮显式设置 `type="submit"` |
| `TextField` / `SelectField` / `TextAreaField` | Mantine TextInput/NativeSelect/Textarea，共用 label、hint、error 关联；保留调用方描述 ID 和原生 change 事件 |
| `CheckboxField` | Mantine Checkbox 的表单组合，label/hint、禁用与布尔值回调；位于 UI 层，控制面共享入口仅转导出 |
| `StatusPill` | Mantine Badge；success / warning / danger / accent / muted，页面负责业务映射；保留原始大小写、完整文本与长标签换行 |
| `SegmentedTabs` | Mantine Tabs 负责内容面板键盘导航；`mode="group"` 使用 Mantine Button 保留 `aria-pressed` 筛选语义 |
| `LanguageSwitcher` / `Toggle` | 语言使用按压按钮并沿用既有持久化；启用状态使用 Mantine Switch，布尔值回调与禁用状态由页面控制 |
| `LoadingState` / `Notice` / `EmptyState` | Mantine Loader/Alert/Paper 组合；错误使用 alert，成功/警告使用 status；Loader 装饰化并由外层提供一次加载播报，局部加载用 inline；空表使用 centered 并保留下一步操作 |
| `TableScroll` | 带名称、可聚焦的横向滚动区；父级网格项需可收缩 |
| `Modal` | Mantine Modal/Drawer 的项目契约：标题、尺寸、底部操作、关闭禁用、退出回调；管理详情、事件详情和移动导航共用 |
| `overlays.ts` | 直接导出 Mantine Popover/Checkbox，列偏好在公共主题下使用，无需机械包装 |

通用组件在 `web/src/components/ui`，不反向依赖 API 或控制面模块。浮层布局由 [Overlay.module.scss](web/src/components/ui/Overlay.module.scss) 管理，行为交给 Mantine；其余尚未迁移组件继续使用 [ConsolePrimitives.module.scss](web/src/components/ui/ConsolePrimitives.module.scss) 与 [components.scss](web/src/styles/components.scss)。`features/control-plane/shared.tsx` 保留协议标签、错误展示、确认流程和业务布局；协议常量、错误归一化与格式化分别由 `lib/protocols.ts`、`admin-api/errors.ts` 和 `utils/format.ts` 提供。分层约束见 [前端架构](docs/frontend-architecture.md)。

按钮、图标和字段的品牌尺寸、状态样式集中在 [Controls.module.scss](web/src/components/ui/Controls.module.scss)，通过 Mantine Styles API 和语义变量接入。字段使用 Mantine Input.Wrapper 的 label/description/error；`attributes.input` 合并调用方描述 ID 与生成的 hint/error ID，不覆盖业务输入值。`SelectField` 采用 NativeSelect 保留 option/optgroup、禁用项和浏览器菜单交互，不新增搜索能力或模拟 change 事件。旧 `.btn`、手写字段框及控制面复选框样式已删除；页面操作布局按 `data-ui="button"` 定位，不能恢复旧按钮视觉类。

页面 SCSS 只维护布局与领域视觉。事件虚拟列表和配置表格分别保留各自的数据与滚动逻辑。

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

本批使用 Mantine **9.6.0**（core/hooks 精确锁定，React 19.2 兼容）。基础组件负责交互，公共组合负责项目契约，业务组件负责数据与提交。完整八页清单、批次和证据见 [迁移记录](docs/mantine-migration.md)。目前已迁移主题、浮层、公共按钮/字段、页面筛选、标签与语言切换、启用开关、发现选择框、反馈/状态与卡片；图表、表格和全页面完整验收仍在后续范围内。

- Modal/Drawer 默认层级 1000，由 Mantine stack 按打开顺序递增；Popover 1200、Tooltip 1300，统一在 theme.ts 修改。ConsoleProvider 通过 Mantine 公开的两种 StackContext 共享同一 stack，跨类型叠加时仅顶层处理 Esc 和焦点约束。条件卸载的详情会注销 stack 条目。
- 焦点恢复使用 Mantine useFocusReturn，与 stack 的 trapFocus 切换分离；条件挂载详情先完成关闭态挂载，再打开。不要在页面添加 focus 定时器。正文单独滚动，标题和底部操作保持可见；长 ID 可换行。关闭动画中的内容通过 inert 退出交互。
- 对话框默认 520px，各业务通过 width 表达尺寸；600px 以下 Drawer 全宽。移动侧栏用左侧 Drawer，桌面保留导航内容；920px 以下隐藏的导航从可访问树移除。
- closeDisabled 同时保护关闭按钮、Esc 和遮罩点击，提交按钮仍由业务 busy 防重复触发。没有提交中的普通浮层允许 Esc 和外部点击关闭。
- 非敏感编辑/确认数据使用 useOverlayState：setValue(record) 打开、setValue(undefined) 开始关闭，把 afterExit 传入 onExitTransitionEnd 才清空数据，避免关闭过程中标题/表单跳变。敏感 Key 使用原有即时清除流程。
- 来源/实体详情切换编辑时，先关闭详情并恢复焦点，再在 onExitTransitionEnd 中打开编辑。不要直接卸载正在持有编辑按钮的详情，否则编辑关闭后无法返回有效入口。
- 列偏好 Popover 使用 Portal、视口自动定位和 focus trap；交互内容使用 Popover，纯文本提示使用 Tooltip。公共字段选择器已采用 Mantine NativeSelect，保留浏览器菜单；用量与控制面页面筛选及 Shell 密钥输入已共用 Mantine 字段，页面仅保留布局。未来 Mantine Select 与嵌套 Popover 接入须按官方 Portal/事件规则单独验证，当前批次没有宣称这些组合已完成。

新增页面的评审需检查：复用组件入口和主题；label/hint/error 与提交契约；首次加载/刷新/错误/空态；键盘和关闭焦点；双主题、长文案与窄屏；图表/虚拟列表测量及资源体积。专业组件继续保留 Chart.js/TanStack Virtual，主题与数据语义验收不能省略。CPA Usage Keeper 的既有 MIT License 与来源说明继续保留。

用量图表使用 [UsageTrend](web/src/features/usage/UsageTrend.tsx) 保留 Chart.js 的时间序列绘制；默认 Input/Output 堆叠，其他指标使用有名称的独立双轴，并提供精确数值表。Canvas 配色由 [useChartTheme](web/src/features/usage/useChartTheme.ts) 读取品牌 Token，响应已有主题状态；分布/Token 构成采用 Mantine Progress，来源延迟与趋势数据采用 Mantine Table。Reasoning 与缓存可能与输入/输出重叠，不能拼成相加的 100% 构成图。原型差距、数据依据和本批取舍见[图表核对记录](docs/chart-prototype-review.md)。
