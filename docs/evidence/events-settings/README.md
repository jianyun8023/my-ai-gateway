# 运行事件与系统设置验证

关联：[#235](https://github.com/jianyun8023/my-ai-gateway/issues/235)。基于 `main` 的主题治理提交 `85903b3`，与 #234 独立。

## 修改范围

- 运行事件列表展示本地化事件类型、脱敏错误摘要、HTTP 状态、模型及来源/账号；完整 ID 和技术元数据保留在详情。
- 详情支持复制 ID、关联事件筛选、来源详情、账号额度及精确请求详情导航。历史请求从既有详情接口获取真实事件与 attempts，不受当天列表筛选限制。
- 设置快照字段统一读取最新 capabilities 查询结果；重载后外部配置再更新，刷新能继续显示新版本。
- 短状态与操作保持一行；密钥时间固定为日期/时间两行；长模型允许换行，宽表只在表格容器内滚动。事件查看列在横向滚动时保持可见。
- 时间明确标记本机时区，移除列表中的内部读模型/数据库表名；设置统计准确表述为能力矩阵条目。

## 验证

- 前端 ESLint、Knip（完整及生产入口）、TypeScript 通过。
- 全量前端 43 个文件、357 项回归通过；快照刷新、失败保留、日期分行、历史详情、错误重试、鉴权/路由切换取消、ID 复制及关联导航均有行为测试。
- Vite 生产构建通过，仍有现存入口 chunk 超过 500 kB 的提示。
- `scripts/frontend-browser-smoke.mjs` 使用临时 localhost 合成 API 和 Chrome，验证中文桌面 1280×900、窄屏 390×844 及 Nebula 深色英文布局：事件失败恢复、真实请求详情跳转与 Escape 关闭、快照 107→108→109 刷新、短标签行高、单元格内容不越界、操作同行、局部横滚与固定查看入口。保留来源编辑隔离、页面焦点、Portal/Escape 回归。
- `git diff --check` 与脚本语法检查通过。未修改后端接口/数据库，也未执行真实 Provider 或生产数据验收。

复验命令：

```sh
mise exec -- npm --prefix web run lint
mise exec -- npm --prefix web run typecheck
mise exec -- npm --prefix web test
mise exec -- npm --prefix web run build
FRONTEND_SMOKE_SCREENSHOT_DIR=/tmp/events-settings-screenshots mise exec -- node scripts/frontend-browser-smoke.mjs
```

## 合成数据截图

- [运行事件桌面](runtime-events-desktop.png)
- [系统设置桌面](settings-desktop.png)
- [深色英文窄屏：事件上下文及固定查看入口](runtime-events-dark-en-mobile-context.png)
- [深色英文窄屏：密钥状态及同行操作](settings-dark-en-mobile-actions.png)

截图中的账号、请求和错误内容都是测试夹具；浏览器原生日期输入显示格式由浏览器地区设置决定。
