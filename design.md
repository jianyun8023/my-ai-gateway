# 前端设计与组件规范

适用于 `/admin/` 控制台。领域模型与协议规则见 [架构设计](docs/ai-gateway-design.md)，开发命令见 [前端 README](web/README.md)。

## 视觉风格

Tech-Utility：冷灰底色、绿色强调、紧凑数据布局。使用固定侧栏、顶部工具栏、清晰标题与细边框卡片，以 Token、请求状态和模型归因为重点。普通卡片不使用浮动阴影。

页面文案只保留操作、状态和必要提示。标题足以说明内容时不再添加副标题；空态给出下一步；接口路径、存储方式、实现过程放在开发文档中。中文与英文同步维护，协议、模型名和 ID 保留原文。

## Token 与尺度

品牌 Token 统一维护在 [gateway-brand.scss](web/src/styles/gateway-brand.scss)，位于 `body`，页面与 Portal 浮层共享继承。`html[data-theme='dark']` 切换深色；原生表单通过 `color-scheme` 跟随主题。

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

现有 `--keeper-*`、`--text-*` 等变量由品牌层映射，页面不重复定义。`themes.scss` 提供基础尺度，品牌样式最后加载。颜色与尺寸的精确值以源码为准。

## 组件职责

| 组件 | 用法 |
| --- | --- |
| `GatewayConsoleShell` | 导航、页面标题、Admin 连接、主题、语言和刷新 |
| `Card` | 标题、可选说明与操作区；`flush` 用于表格 |
| `Button` / `IconButton` | 主次操作、禁用、加载与可访问名称；提交按钮显式设置 `type="submit"` |
| `TextField` / `SelectField` / `TextAreaField` | 原生表单，共用 label、hint、error 关联；保留调用方描述 ID |
| `StatusPill` | success / warning / danger / accent / muted；页面负责业务状态映射 |
| `SegmentedTabs` | tabs 模式切换内容面板；`mode="group"` 用于筛选 |
| `LoadingState` / `Notice` / `EmptyState` | 加载、错误重试、成功反馈与空态；空表使用 `layout="centered"` |
| `TableScroll` | 带名称、可聚焦的横向滚动区；父级网格项需可收缩 |
| `Modal` | dialog / drawer、焦点约束与恢复、滚动锁；管理详情与事件详情共用 |

通用组件在 `web/src/components/ui`，不反向依赖 API 或控制面模块。公共样式由 [ConsolePrimitives.module.scss](web/src/components/ui/ConsolePrimitives.module.scss) 与 [components.scss](web/src/styles/components.scss) 管理。`features/control-plane/shared.tsx` 保留协议标签、错误展示、确认流程和业务布局；协议常量、错误归一化与格式化分别由 `lib/protocols.ts`、`admin-api/errors.ts` 和 `utils/format.ts` 提供。分层约束见 [前端架构](docs/frontend-architecture.md)。

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
