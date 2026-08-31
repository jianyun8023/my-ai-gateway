# CI 门禁

仓库的 GitHub Actions 工作流位于 [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)。它在以下场景运行：

- 针对 `main` 的 Pull Request；
- 推送到 `main`；
- 手工触发 `workflow_dispatch`。

工作流使用 `mise.toml` 固定的 Rust 和 Node 版本，并启动临时 PostgreSQL 16 service。仓库不需要为 CI 配置 Provider 凭据、API Key 或数据库 Secret；工作流中的 PostgreSQL 用户和密码只属于当前 runner 上的临时测试实例。

## 门禁内容

`CI / verify` 执行与本地 `mise run verify` 相同的组合门禁：

| 阶段 | 检查 |
| --- | --- |
| `config-check` | 使用锁定的 Node 解析 `config.example.json` |
| `lint` | Rust fmt、check、Clippy；Web ESLint 和 TypeScript typecheck |
| `build` | Web 生产构建和 Rust workspace 构建 |
| `test` | Rust workspace 测试和 Web Vitest 测试 |
| `test-postgres` | 显式运行 `#[ignore]` 的 DB-first PostgreSQL 集成测试 |

CI 始终设置 `TEST_DATABASE_URL`。因此常规 Rust 测试中的 PostgreSQL migration、Usage、模型目录、模型发现和 API 回归不会走“未配置数据库”的跳过分支；随后 `test-postgres` 再运行唯一显式忽略的控制面测试。Rust 测试使用单线程执行，避免进程级环境变量和共享测试数据库造成不稳定竞争。

## 本地复现

先通过 Mise 安装固定工具链和前端锁定依赖：

```bash
mise install
mise run install
```

为完整复现 CI，需要准备一个专用 PostgreSQL 测试数据库并设置 `TEST_DATABASE_URL`：

```bash
export TEST_DATABASE_URL='postgres://gateway:gateway@127.0.0.1:5432/gateway_test'
mise run verify
```

测试会使用 migration，并在需要时创建带随机名称的隔离 schema。不要把运行时 `DATABASE_URL` 指向的生产或开发控制面数据库复用为 `TEST_DATABASE_URL`。

未设置 `TEST_DATABASE_URL` 时，`mise run verify` 仍会执行 JSON、静态检查、构建和普通测试，但会明确报告未运行显式忽略的 PostgreSQL 测试；这种运行不等同于 CI 完整门禁。

## Required Check

工作流首次成功运行后，在 GitHub 分支保护或 ruleset 中将 `CI / verify` 设为 `main` 的 required check。仓库中的 workflow 只能产生该检查，是否强制合并前通过仍由仓库设置控制。

## 安全与缓存

- workflow 权限固定为 `contents: read`，checkout 不保留 Git 凭据；
- 不读取或上传仓库 Secret，不上传 PostgreSQL 数据和测试日志产物；
- Mise 缓存固定工具链，Cargo 和 npm 缓存分别由锁文件与 `mise.toml` 计算 key；
- 并发组按 workflow 和 Git ref 隔离，同一 PR 的旧运行会在新提交到达时取消。
