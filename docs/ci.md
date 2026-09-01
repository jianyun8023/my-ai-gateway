# CI 与镜像发布

仓库将 Pull Request 门禁和容器镜像发布拆分为两个独立的 GitHub Actions workflow：

- [`.github/workflows/pull-request.yml`](../.github/workflows/pull-request.yml)：只在 Pull Request 上运行验证与测试。
- [`.github/workflows/publish-images.yml`](../.github/workflows/publish-images.yml)：在 `main` 或 Release tag 推送时构建镜像。

## Pull Request 门禁

PR workflow 使用 `mise.toml` 中锁定的 Rust 和 Node.js 版本，并把静态验证与测试拆分为两个并行 job。

### Validate and build

该 job 不连接数据库，依次执行：

| 阶段 | 命令 | 内容 |
| --- | --- | --- |
| 配置检查 | `mise run config-check` | 解析仓库中的 JSON 示例与测试 case |
| 静态检查 | `mise run lint` | Rust fmt/check/Clippy，以及 Web ESLint/typecheck |
| 构建 | `mise run build` | 构建 Web 生产资源和 Rust workspace |

### Unit and PostgreSQL tests

该 job 启动临时 PostgreSQL 16 service，并设置专用的 `TEST_DATABASE_URL`：

| 阶段 | 命令 | 内容 |
| --- | --- | --- |
| 单元与契约测试 | `mise run test` | Rust workspace、Web Vitest 和 Node 契约测试 |
| PostgreSQL 集成测试 | `mise run test-postgres` | 显式运行 `#[ignore]` 的 DB-first 回归 |

PostgreSQL 用户、密码和数据库只存在于当前 GitHub Actions runner，不依赖仓库 Secret。测试会通过 migration 创建随机隔离 schema，不会复用运行时控制面数据库。

## 镜像发布

镜像发布 workflow 为以下两个平台分别构建镜像 digest，再合并为同一个多架构 manifest：

| 平台 | Runner |
| --- | --- |
| `linux/amd64` | `ubuntu-latest` |
| `linux/arm64` | `ubuntu-24.04-arm` |

镜像发布到：

```text
ghcr.io/jianyun8023/my-ai-gateway:<tag>
```

### main

每次推送到 `main` 都会更新：

```text
ghcr.io/jianyun8023/my-ai-gateway:main
```

`main` 构建不会创建或更新 `latest`。

### Release tag

推送符合 `v*.*.*` 的 Git tag 时，workflow 会保留原始 tag，并生成语义化版本别名。例如 `v1.2.3` 会生成：

```text
ghcr.io/jianyun8023/my-ai-gateway:v1.2.3
ghcr.io/jianyun8023/my-ai-gateway:1.2.3
ghcr.io/jianyun8023/my-ai-gateway:1.2
```

稳定 Release tag 还会更新 `latest`；Prerelease tag 不更新 `latest`。

## 本地复现

先安装固定工具链和前端锁定依赖：

```bash
mise install
mise run install
```

复现验证 job：

```bash
mise run config-check
mise run lint
mise run build
```

复现测试 job 时，必须使用专用 PostgreSQL 测试库：

```bash
export TEST_DATABASE_URL='postgres://gateway:gateway@127.0.0.1:5432/gateway_test'
mise run test
mise run test-postgres
```

不要把生产或开发控制面使用的 `DATABASE_URL` 复用为 `TEST_DATABASE_URL`。

本地也可以执行 `mise run verify` 完成同等范围的组合门禁。未设置 `TEST_DATABASE_URL` 时，`mise run verify` 会明确跳过显式 ignored 的 PostgreSQL 回归，因此不等同于完整 PR 门禁。

## Required Checks

首次成功运行新 workflow 后，应在 GitHub 分支保护或 ruleset 中把以下检查设为 `main` 的 required checks：

- `Pull request checks / Validate and build`
- `Pull request checks / Unit and PostgreSQL tests`

旧的 `CI / verify` 检查可以从 required checks 中移除。是否强制合并前通过由仓库设置控制，workflow 本身只负责产生检查结果。

## 权限与缓存

- PR workflow 只授予 `contents: read`，checkout 不保留 Git 凭据。
- 镜像构建和 manifest 合并 job 额外授予 `packages: write`，用于写入 GHCR。
- CI 不读取 Provider Key、Gateway Key 或 Admin Key，也不上传数据库内容。
- Cargo 和 npm 缓存由锁文件与 `mise.toml` 计算 key。
- 多架构构建按平台使用独立的 GitHub Actions cache scope。
- 同一 PR 的旧运行会在新提交到达时取消；镜像发布运行不会被自动取消。
