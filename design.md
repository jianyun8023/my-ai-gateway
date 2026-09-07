# 前端 UI 设计与组件维护

基于 2026-09-07 的 `main 01fe40d` 前端实现梳理，并记录本次组件化后的维护方式。适用于 `/admin/` 控制台的八个入口。视觉方向参考 [品牌规范](docs/brand-spec.md)，领域含义以 [架构设计](docs/ai-gateway-design.md) 为准；本文负责连接视觉意图与实际组件。

## 1. 当前风格

**Tech-Utility：冷灰底色、绿色强调、紧凑数据布局。** 左侧固定导航、顶部工具栏、页面标题与说明、筛选区、内容卡片构成稳定层级。通过细边框、留白与文字权重分区，普通卡片不使用浮动阴影。Token、请求状态、模型和 Source 归因是视觉重点。

- 监控：总览、用量分析、请求事件。图表与数字卡片汇总数据，事件列表支持虚拟滚动、列设置和详情。
- 配置：来源管理、模型发现、模型与路由、能力矩阵。列表展示事实，抽屉展示详情，Modal 承载编辑与确认。
- 系统：设置。分组管理 Admin 连接、运行快照与 Virtual Key 等功能。
- 页面使用简体中文和英文翻译。协议、模型名、ID 保留准确原文；长标识可换行，不能让整页横向溢出。

实际代码入口是 `App → GatewayConsoleShell → GatewayUsagePage / GatewayManagementPage → features/control-plane`。这次不改变数据请求、领域枚举、路由或凭据契约。

## 2. 视觉 Token

运行期唯一品牌入口是 [gateway-brand.scss](web/src/styles/gateway-brand.scss)。Token 放在 `body`，让页面和挂载在 `body` 的 Modal、Drawer、Tooltip 同源继承；深色模式通过 `html[data-theme='dark']` 切换。旧 `--keeper-*`、`--text-*` 等名称是现有组件的消费接口，不在页面中重复覆盖。

| 角色 | 浅色 | 深色 |
| --- | --- | --- |
| 页面 `--bg` | `oklch(98% 0.005 250)` | `oklch(17% 0.012 250)` |
| 表面 `--surface` | `oklch(100% 0 0)` | `oklch(22% 0.012 245)` |
| 主文字 `--fg` | `oklch(22% 0.02 240)` | `oklch(94% 0.006 250)` |
| 辅助文字 `--muted` | `oklch(50% 0.018 240)` | `oklch(72% 0.012 245)` |
| 边框 `--border` | `oklch(90% 0.008 240)` | `oklch(34% 0.012 245)` |
| 强调 `--accent` | `oklch(58% 0.16 145)` | `oklch(68% 0.15 145)` |

成功、警告、失败使用 `--success` / `--warn` / `--danger`，浅背景由 `color-mix` 派生。状态必须同时有文字，不能只靠颜色区分。`unknown` 与 `unsupported` 不得显示为成功；估算与缺失 usage 保持独立含义。品牌对比度数值是设计目标，本轮没有声称完成全页面 WCAG 对比度认证。

| 尺度 | 当前约定 |
| --- | --- |
| 字体 | 系统 sans 优先，Inter / Segoe UI 后备；ID、协议和数值用系统 mono，JetBrains Mono / IBM Plex Mono 后备；不新增在线字体请求 |
| 字号 | 页面标题 28px；正文 14px；卡片标题 16px；标签和控件 12px；紧凑元数据 11px；22px 保留为较大区块标题 Token |
| 间距 | 4 / 8 / 16 / 24 / 32px；普通卡片内边距 20px；页面布局按 shell 的响应式规则收缩 |
| 圆角 | 6px 小控件、8px 常规容器、12px 卡片与弹窗；状态标签为胶囊 |
| 控件 | 桌面按钮 36px、小按钮 32px；触摸布局普通按钮、表单与分段按钮至少 44px |
| 数字 | `tabular-nums` 等宽数字；大数复用 `formatCompact`，精确值保持可查 |

`themes.scss` 仍提供 Keeper 基础尺度，`gateway-brand.scss` 在样式入口最后加载并覆盖控制台品牌值。主题切换时原生表单通过 `color-scheme` 跟随明暗。

## 3. 组件边界

| 层级 | 文件 / 组件 | 责任 |
| --- | --- | --- |
| 全局布局 | `components/gateway/GatewayConsoleShell` | 八入口导航、标题、Admin 连接、主题、语言、刷新、移动端侧栏 |
| 容器 | `components/ui/Card` | 标题、说明、标题元信息、操作区；`flush` 适合表格 |
| 按钮 | `Button` / `IconButton` | variant、尺寸、禁用、加载、可访问名称；Button 默认 `type="button"`，提交需显式声明 |
| 表单 | `FormField.tsx` 中的 `TextField` / `SelectField` / `TextAreaField` | 共用 FieldFrame，关联 label、hint、error，保留调用方 `aria-describedby` |
| 输入扩展 | `Input` / `Select` | 既有带右侧元素输入、自定义选择器；按需要使用，普通管理表单优先原生 Field |
| 状态 | `StatusPill` | success / warning / danger / accent / muted；消费页面决定业务状态映射 |
| 分段选择 | `SegmentedTabs` | 默认 tabs 模式，方向键/Home/End 切换、单一 Tab 停靠点、关联 panel；`mode="group"` 用于时间粒度等筛选，用 `aria-pressed` |
| 反馈 | `LoadingState` / `Notice` / `EmptyState` | 加载通告、错误重试、成功反馈、空态；空表使用 `EmptyState layout="centered"` |
| 滚动 | `TableScroll` | 带名称、可聚焦的横向滚动区，宽表保留列结构 |
| 浮层 | `Modal variant="dialog" / "drawer"` | Portal、关闭、焦点约束与恢复、滚动锁；复用既有实现 |
| 业务组合 | `features/control-plane/shared.tsx` | 协议名称、领域错误映射、确认对话框、表单/详情布局等业务组合 |

通用组件只依赖 React、i18n、图标和 UI 样式，不反向导入控制面或 API。页面直接导入 `components/ui`，不通过控制面文件转导出。公共交互样式放在 [ConsolePrimitives.module.scss](web/src/components/ui/ConsolePrimitives.module.scss)，Card/Button/Modal 等现有全局组件仍由 [components.scss](web/src/styles/components.scss) 维护。

具体业务表格和图表保留在各自页面中。事件虚拟列表与配置 CRUD 表格的数据模型、滚动机制不同，不强行套用一个万能表格。

## 4. 交互与响应式细节

- 主操作突出创建或保存；次级操作使用 secondary/ghost。危险操作沿用当前实底 danger 按钮与确认流程。
- 图标按钮必须传 `label`。加载按钮同时禁用并暴露 `aria-busy`，避免重复触发；表单内非提交操作不得意外提交。
- 字段错误使用文字、错误边框、`aria-invalid` 和关联说明；调用方的描述 ID 与生成的 hint/error ID 合并，不能被 props 展开覆盖。
- 所有可交互元素提供可见焦点环；分段 Tab 的焦点、选中项与 panel 同步。筛选按钮标记选中状态，高级筛选标记展开状态。
- 加载、错误、空数据是不同状态。失败提供重试；空态说明缺什么和下一步，不用虚假数字填充。
- ≤920px：侧栏变抽屉；普通按钮、字段、分段控件扩大至 44px；宽表在自身容器内滚动。
- ≤600px：筛选与表单以单列为主；卡片标题与操作区堆叠；通知操作另起一行；空态按钮可占满宽度。
- ≤380px：沿用现有 KPI 单列和更紧凑导航处理。长模型名和英文按钮必须检查换行与溢出。
- 减少动态效果偏好下，关闭按钮浮动与字段过渡；Modal、导航沿用现有 reduced-motion 规则。

现有表格内图标按钮仍有桌面密度优化，触摸布局恢复 44px。专项控件（如列菜单、语言切换器）的完整触摸与辅助技术审计不属于本轮验证结论。

## 5. 从现状到本次修正

原品牌文档描述了目标方向，但原实现中品牌色仅在 `.app-frame`、卡片尺度仅在 shell，Portal 浮层退回暖灰与 24px 圆角。现在品牌色、字体、卡片尺度和焦点 Token 在 `body` 统一。

原控制面 `shared.tsx` 同时维护基础控件和业务组合，用量页又独立实现 badge、加载和错误反馈。现在基础控件移入 UI 层，五个控制面页面与三个用量入口共用相应组件；删除被替代的重复样式。保留现有 MIT 来源说明，不引入新的 UI 依赖。

品牌文档中“所有颜色在 :root”“危险按钮透明底”“图标 28px”“禁止 Inter”与代码中的 body 继承、实底危险按钮、现有尺寸和字体栈存在差异。本文以源码为依据说明实际维护方式；品牌方向仍保留，不把尚未实现的目标宣称已实现。

## 6. 后续维护方式

1. 页面需求先找同职责 UI 组件。增加变体前至少说明现有调用方和复用目的。
2. 调整颜色、圆角和尺度先改 Token；调整交互在公共组件中完成；页面 SCSS 只保留布局和领域视觉。
3. 新页面复用 shell、Card、Field、StatusPill 与反馈组件，并为 loading / error / empty / success 提供明确状态。
4. 新交互验证键盘、字段关联和提交行为；样式检查覆盖明暗、窄屏、长文本、Portal。低影响样式改动不编写镜像实现的测试。
5. 运行 `mise exec -- npm --prefix web run lint`、`typecheck`、`test` 和 `build`；纯前端修改不将 Rust、数据库和真实 Provider 验收写为已通过。

本轮执行证据与限制见交付说明；组件行为回归在 `components/ui/test/ConsolePrimitives.test.tsx`，既有页面测试继续覆盖导航与控制面交互。

## 7. 本轮验证记录

- 前端 ESLint、TypeScript、生产构建通过；26 个测试文件、103 项测试通过。
- 浏览器使用仓库的本地测试夹具，检查桌面总览、来源表格、Portal 编辑弹窗，以及 390px 深色表单；验证 Escape 关闭后焦点恢复。
- 窄屏发现 Tab panel 的网格最小宽度会被内部宽表撑开，已补 `min-width: 0`，使横向滚动限制在表格区域。
- 测试输出存在 React 懒加载的 `act(...)` 提示，测试未失败。没有执行 Rust、PostgreSQL、真实 Provider 或生产环境验收。
