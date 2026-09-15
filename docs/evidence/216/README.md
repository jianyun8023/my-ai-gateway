# #216 查询生命周期与导航整理

日期：2026-09-15。关联 [Issue #216](https://github.com/jianyun8023/my-ai-gateway/issues/216)。

基线：`main` `2343694b94e94878da3fe5da6bd20126097fed2b`（PR #215 合并后）。独立分支：`codex/216-frontend-query-lifecycle`。

## 问题与结果

管理页面原先只接收刷新版本，没有区分连接身份变更与普通刷新。应用其他 Admin Key 或清除会话 Key 后，失败的新查询仍会保留旧数据；运行事件正在加载的旧分页也没有随身份变更立即取消。

1. 管理空间以 `authGeneration` 作为组件身份：换 Key 或清除 Key 会卸载旧查询、分页、详情和表单状态。普通刷新保持组件身份，继续保留当前内容。
2. `useQuerySession` 统一首屏加载、取消、结果发布及后续请求的生命周期。`useAdminQuery` 在其上归一化错误；`useUsageData` 保留用量聚合、固定时间窗口、分页和导出状态。运行事件后续分页使用同一 session signal，在刷新开始时取消旧分页，同步防止重复请求并按事件 ID 去重。
3. 七个页面的 ID、分组、功能空间与图标名称收敛到 `consoleNavigation` 元数据，App 派生导航并解析 UI 图标和中英文文案。`useEventColumns` 收口列偏好的读写和组件状态，列定义文件只保留定义与纯计算。

## 行为验收

| 场景 | 结果 / 证据 |
| --- | --- |
| 换 Key 后新请求失败 | 旧管理行、详情和旧分页结果失效；`App.identity.test.tsx` |
| 清除会话 Key | 旧 Virtual Key 列表从管理页面移除；同上 |
| 同身份刷新失败 | 保留当前行和已打开的详情；同上 |
| 首屏或分页忽略 abort 后返回 | 旧结果不进入新查询；`useUsageData.test.tsx`、`RuntimeEventsPage.test.tsx` |
| Usage 相对时间与刷新失败 | 分页和导出继续使用已发布窗口、原游标；`useUsageData.test.tsx` |
| 旧导出在身份变更后成功或失败 | 不触发旧文件下载，不覆盖新导出的忙碌/错误状态；同上 |
| 运行事件重复分页、分页重叠 | 同步锁阻止重复请求，重叠事件只显示一次；`RuntimeEventsPage.test.tsx` |
| 运行事件刷新期间分页 | 开始刷新即取消旧分页；失败保留已加载页和游标，成功替换为新首屏；同上 |
| 导航顺序与中英文切换 | 七个入口按既定顺序显示，标签随语言更新；`App.navigation.test.tsx` |

换 Key 测试在修复前观察到旧分页 signal 未取消，修复后通过。测试通过实际 App、页面、AdminClient 和模拟 fetch 执行，不访问真实 Provider。

## 验证

通过仓库 `mise.toml` 的 Node 24.11.1 工具链，在独立 worktree 执行 `npm ci`，使用锁文件中的 Vitest 5.0.0、Mantine 9.6.0。未复用主工作区旧版本的 `node_modules`。

- 前端 ESLint / Knip：通过。
- 应用与测试 TypeScript：通过。
- 前端完整 Vitest：39 个文件、308 项通过，0 失败。
- Vite 生产构建：通过；主包仍有超过 500 kB 的既有提示。
- `git diff --check`：通过。

调用方式为在主仓库执行 `mise exec -- npm --prefix /private/tmp/my-ai-gateway-216/web <脚本>`，脚本分别为 `ci`、`run lint`、`run typecheck`、`test`、`run build`；安装使用独立 npm 缓存。没有修改依赖声明或锁文件。

本轮验证覆盖前端行为与构建；未运行浏览器视觉验收、Rust / Contract / PostgreSQL 或真实 Provider 验收，未部署。

## 原 Issue 其他建议的处理

- Mantine 原子组件直接导入：沿用 `design.md` 的既定规则，配置表格直接使用 Mantine Table；不增加全域禁止导入或机械薄封装。请求事件继续使用自己的单一虚拟滚动容器。
- `pages` / `features` 与 `gateway-usage` 目录迁移：当前装配与数据适配职责明确，本轮不迁移。
- Shell 连接状态入 Zustand：本次通过既有身份版本完成失效行为，不引入新状态存储或改变 Key 的 sessionStorage 边界。
- 主题、通知、浮层 hook 共置、测试目录统一、Theme 类型位置和大文件拆分：不作为本轮验收条件，后续按具体修改需要处理。

本记录对应分析后确认的三项实施范围，不代表原 Issue 的全部建议均已实现，也不据此关闭 Issue。
