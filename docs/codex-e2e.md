# Codex CLI E2E 测试

这组测试验证真实 Codex CLI 到 my-ai-gateway 的 Responses 链路，覆盖：

- 固定隔离的 `CODEX_HOME` 与 custom model provider；
- Codex CLI 发起本地 shell 工具调用，并接收工具结果；
- Codex CLI 的实时网络搜索（`--search`）与官方来源返回；
- Gateway 的 UsageEvent 持久化、Token 统计和 Adapter/native 路径。

测试分为两层：

| 层级 | 入口 | 是否访问上游 | 用途 |
| --- | --- | --- | --- |
| 契约回归 | `mise run test` | 否 | 参数、配置生成、JSONL 解析和断言逻辑 |
| 真实 E2E | `mise run test-codex-e2e` | 是 | 真实 Codex CLI、Gateway 和 Provider |

真实 E2E 默认只运行低成本的工具调用；网络搜索必须通过 `--include-search` 或显式
`--case codex.web_search` 开启。它不会在普通 CI 或 Pull Request 门禁中自动运行。

## 前置条件

1. 已安装仓库锁定的 Node/Mise 工具，并能执行 `codex --version`；
2. 已启动一个可访问的 Gateway，并且它的 PostgreSQL Runtime Snapshot 有对应模型的
   OpenAI Responses 路由；
3. 该 Gateway 的数据面 Key 和 Admin Key 可分别使用。Admin Key 用于确认 Usage，
   不应与 Provider Key 混用。

当前仓库的本机部署示例使用 `http://127.0.0.1:8788/v1`，可选择：

- `k3`：Kimi Responses Adapter；
- `deepseek-v4-flash`：DeepSeek 原生 Responses；
- `MiniMax-M3`：MiniMax 原生 Responses。

实际模型名以目标 Gateway 的 `GET /v1/models` 为准。Runner 会在启动 Codex CLI 前查询该
接口，并在提供 Admin Key 时同时解析 `/admin/routes/openai_responses/{model}`；指定模型未公开
或没有可用 Responses 路由时立即退出，避免把控制面配置问题误判为 CLI 或 Provider 故障。

## 配置

复制 [`.env.codex-e2e.example`](../.env.codex-e2e.example) 为 Git ignored 的
`.env.codex-e2e`，填入网关 Key：

```dotenv
CODEX_E2E_TESTS=1
CODEX_GATEWAY_BASE_URL=http://127.0.0.1:8788/v1
CODEX_GATEWAY_API_KEY=<gateway-data-plane-key>
CODEX_GATEWAY_ADMIN_KEY=<gateway-admin-key>
CODEX_MODEL=k3
CODEX_HOME=target/codex-e2e/home
CODEX_E2E_WORKSPACE=target/codex-e2e/workspace
CODEX_E2E_RESULT_DIR=target/codex-e2e/results
```

`.env.codex-e2e` 和 `CODEX_HOME` 必须是隔离路径；runner 会拒绝使用默认的
`~/.codex`，并以 `600` 写入生成的 `config.toml`。Key 只通过
`env_key = "CODEX_GATEWAY_API_KEY"` 注入，绝不会写入配置或 artifact。

Codex 的 custom provider 配置采用 Responses wire API。相关字段含义见 OpenAI 的
[custom model provider](https://learn.chatgpt.com/docs/config-file/config-advanced)
和 [`CODEX_HOME` 环境变量说明](https://learn.chatgpt.com/docs/config-file/environment-variables)。

## 运行

先查看稳定 case，不发出请求：

```bash
mise run test-codex-e2e -- --list
```

运行 Kimi Adapter 的 shell 工具 E2E（需要 `CODEX_E2E_TESTS=1` 和两个 Gateway Key）：

```bash
mise run test-codex-e2e -- --model k3
```

切换到 DeepSeek 原生 Responses：

```bash
mise run test-codex-e2e -- --model deepseek-v4-flash
```

显式加入网络搜索（会产生较高 Token 消耗和 Provider 费用）：

```bash
mise run test-codex-e2e -- --model k3 --include-search
```

也可以只运行搜索 case：

```bash
mise run test-codex-e2e -- --case codex.web_search --model deepseek-v4-flash
```

runner 将 `--search` 放在 `exec` 子命令之前，以兼容当前 Codex CLI 的全局参数解析。
每次执行使用 `--strict-config --ephemeral --json --sandbox read-only`，并把 workspace
限制在配置的隔离目录。Codex 官方的非交互模式说明见
[Codex exec](https://learn.chatgpt.com/docs/non-interactive-mode)。

没有 Admin Key 时可以显式跳过 Usage 检查：

```bash
mise run test-codex-e2e -- --skip-usage-check
```

这只验证 CLI/Gateway 行为，不适合作为完整用量回归。

## 断言和结果

`codex.tool` 会让模型读取 runner 写入的随机 `CANARY.txt`，并同时断言：

- JSONL 中出现成功的 `command_execution`；
- 命令输出包含本次随机 canary；
- 最终消息严格为 `CODEX_GATEWAY_E2E_OK:<canary>`；
- Admin Usage API 返回成功且 `total_tokens > 0` 的事件。

随机 canary 不再出现在 prompt 中，模型必须真实执行 shell 命令才能得到最终答案。失败 artifact
会给出 `diagnostic_stage`，区分 CLI 启动/超时、JSONL、Usage、命令事件缺失、命令失败、未观察到
canary 和最终文本不匹配。

`codex.web_search` 会断言：

- CLI JSONL 中至少有一个已完成的 `web_search` item；
- 最终消息符合 `SEARCH_E2E_OK:<version>:<url>`；
- URL 主机必须是 `blog.rust-lang.org`，避免模型仅凭记忆伪造任意来源；
- Admin Usage API 能查到对应 Token 事件。

结果写入 Git ignored 的 `target/codex-e2e/results/<run-id>.json`，只保存 commit、case、
事件类型/数量、断言布尔值、Usage 汇总和安全的 Usage 元数据，不保存 prompt、完整模型
输出、命令输出、Authorization 或 API Key。最终消息文件默认在断言后删除；只有显式传入
`--keep-output` 才会保留以便调试。

Artifact schema v2 将 Usage 中的 403/429 归类为 `provider_unavailable`，与行为断言失败分开；
只有真正的 `failed` 令命令返回非零。

## 维护规则

1. case ID 保持稳定；新增 case 同时更新 `tests/codex/cases.json`、契约单测和本文档；
2. 高成本搜索、长上下文或 reasoning 场景标记为 `cost=high`，默认不运行；
3. 对会变化的事实只断言官方来源域名和结构，不写死版本号；
4. 不把真实 Key、prompt/response 正文或搜索正文提交到 GitHub；
5. Provider 行为变化先记录到对应 Issue，再调整断言；
6. `mise run test` 必须保持无网络、无 Provider Secret、无 Codex session 依赖。

真实 E2E 的问题与运行摘要统一记录在 [Issue #62](https://github.com/jianyun8023/my-ai-gateway/issues/62)。
