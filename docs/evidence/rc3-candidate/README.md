# RC3 候选镜像完整回归（2026-09-09）

本次以 [PR #189](https://github.com/jianyun8023/my-ai-gateway/pull/189) 合并后的
`a6a4078ec43ab28e0ffbc12aef1ee4f203da8886` 为候选，实际运行 GHCR 镜像，
不以本地 Rust 二进制替代镜像。严格门禁尚未全部通过，
[#192](https://github.com/jianyun8023/my-ai-gateway/issues/192)跟踪剩余结果，
因此未创建 RC3 标签或 Release。

- 镜像索引：`ghcr.io/jianyun8023/my-ai-gateway@sha256:8e82a301f8824df044e2843a8f5eb64807b655d0c47fb18715bbcdc2f96b8c81`。
- Linux arm64 manifest：`sha256:074cc6fff253b96761e1874c67c83b19088fe1fdf68f60dcc9463a943c1e2145`。
- Linux amd64 manifest：`sha256:6b96ce3b015a314e29f9321f6bd9ac36a239df1e273b0e803053aa69603ac4d6`。
- [双架构构建](https://github.com/jianyun8023/my-ai-gateway/actions/runs/34311060395)成功；运行平台为 macOS Docker 的 Linux arm64，amd64 管理面冒烟通过仿真执行。
- 真实 Provider 测试使用原有生产凭据，但网关和 PostgreSQL 均为本地隔离环境，每项独立 schema；未访问生产数据库、未部署生产。

## 协议与上游能力

**协议矩阵复验** **18/18**：DeepSeek、MiniMax、Kimi，
各三协议 × JSON/SSE。检查正常终止、Responses 事件顺序、上游明确报告的用量字段
与持久化事件、usage source、系统事件和 schema 清理。

**初次矩阵**保留原始 17/18 结果：临时对账脚本将上游未报告的
`reasoning_tokens` 归一化为 0，再与 MiniMax Chat SSE 的已有 thinking 估算比较，产生误报。
当前网关 `usage_for_sse_response → merge_thinking_into_report` 会在未报告 reasoning 时
按 #99 逻辑填入估算值；复验只移除对“未报告 reasoning 必须等于零”的错误假设，
所有上游明确计数（包括显式零）仍精确比较。未修改镜像或网关逻辑。

**完整 live 首轮**开启全部十项（含高成本搜索与 signature），
且使用 `--strict-known-issues`：**8 passed / 2 failed**。失败为 DeepSeek search call 未全部
completed、MiniMax function 未以 tool_calls 终止。其他八项包括 Kimi 搜索 JSON/SSE、
reasoning signature 回传和跨 Source fallback 均通过。

**相同请求与断言的定向复验**两项核心行为均通过；但 DeepSeek
`action.sources` 与 `url_citation` 仍均为 0，严格模式触发 #85 两个已知缺口并返回 exit 1。
MiniMax 首轮搜索则实际返回 10 sources / 10 citations。复验未覆盖首轮失败记录，
不把一次重试成功记为稳定性问题已经修复。临时诊断仅为失败增加状态/计数元数据，未放宽断言。

## 镜像控制面

**arm64** 与 **amd64** 各 **25/25**：
健康、metrics、控制台 HTML/JS/CSS、无凭据拒绝、Virtual Key 创建/显式读取、
普通列表脱敏、数据面与 Admin 隔离、Source 导入、当前 Schema 表列、重启持久化、
零重叠期轮换、撤销和 Admin 审计元数据。该组仅使用本地生成的测试 Key，不调用真实 Provider。

amd64 首次通过镜像索引启动时遇到本机 Docker classic image store 的跨架构 digest 冲突；
改用该索引已核对的 amd64 manifest 后通过，未更换候选构建。

## 离线故障、SDK 与负载

**故障回归** **65/65**，覆盖 HTTP 错误、retry/fallback、SSE 心跳/超时/
截断/取消、gzip JSON/SSE 和 #188 显式零计数，均同时核对 PostgreSQL 请求/attempt 归因。

**外部扫描与 SDK**：官方 SDK **10/10**；CompatCanary chat 5 + modern 7
全部通过；llmprobe quick **15 PASS / 0 FAIL / 92 SKIPPED / 2 UNSUPPORTED**，
跳过和不支持项保留既有覆盖边界，不记为通过。Mock 由相同源码构建，外部客户端调用的是
候选容器；故障组仅通过测试环境变量缩短既有超时/心跳，不修改镜像。

完整 24 配置负载的逐项结果、拓扑和最终清理记录维护在
[Issue #120 本次运行记录](https://github.com/jianyun8023/my-ai-gateway/issues/120#issuecomment-5595988975)。
该组包含 4 场景 × 6 并发 × 每 VU 100 次的 Direct/Gateway 对照；以记录中已执行结果
为准，不把计划数量视为通过。它是带 PostgreSQL 写入的本地 Mock 基线，不能代表生产性能。

## Codex 真实端到端

**聚合证据**最终 **8/9**：DeepSeek、MiniMax 各 3/3，
Kimi 工具和多轮通过、搜索失败。三家原生 Responses 使用 codex-cli 0.153.4，
全部保持只读沙箱、独立 CODEX_HOME/工作目录、Virtual Key 和 schema。

初轮工具/多轮失败来自本机外层嵌套 sandbox：透明诊断看到实际 exec_command 调用和
permission_error，本机无 Provider 的 sandbox-exec 也返回 exit 71、
`sandbox_apply: Operation not permitted`。在外层沙箱外复验，同时保留 Codex 的
`--sandbox read-only` 后，六项工具和多轮测试全部通过，不能把初轮执行环境问题
归因于网关或 Provider 工具不支持。

Kimi 搜索两轮都未匹配最终整行 `SEARCH_E2E_OK:<version>:<url>`。
复验有两个 completed search、非空 query，CLI exit 0，HTTP 200/native/parsed，
Usage 检查通过；但整行匹配失败导致 source_host/version_present 均未通过。
这两个字段不能单独证明回复中完全没有 URL/版本；额外文本、拒绝回答、来源缺失或
其他原因尚未区分，根因未确认。未继续重试掩盖结果，未放宽最终断言，未保存正文。

## 测试工具修复

[#190](https://github.com/jianyun8023/my-ai-gateway/issues/190) /
[PR #191](https://github.com/jianyun8023/my-ai-gateway/pull/191)修正 Codex CLI 0.153.4 的
全局 `--search` 位置，并移除多轮 artifact 的 expected/final_text 正文。
修复提交 `e8dd187` 的 [CI](https://github.com/jianyun8023/my-ai-gateway/actions/runs/34312008042)
通过；本地 Codex runner 23/23、全部脚本测试 186/186，实际 CLI 默认/search 参数解析均 exit 0。
真实 Codex 回归使用等价临时修正，保留原工具、精确文本、搜索和 Usage 断言；
不能据此声称旧版本测试脚本原样通过。

## 证据边界

逐请求 JSON 仅保留于本地 `target/release-regression/rc3-candidate/`，
未随本记录发布。仓库只记录汇总计数与已知问题，不包含逐请求 ID、Token 数或关联元数据。
本地产物不包含真实 Key、数据库 URL、prompt/response/thinking 正文。
此记录不覆盖生产部署和原生 amd64 机器性能，外部扫描的跳过项不记为通过。
