# x-cli Scope 定义（3 层模型）

> **目的**：消除 "x-cli 干啥 / agent 干啥" 的歧义。meta-skill 的其他文档都引用本 scope 模型。
>
> **生效版本**：scope 概念 v0.1 起就成立（文档层面）；`auth.yaml` 自动登录的 x-cli 实现是 v0.1 重构内容。

## 模型总览

```
+---------------------------------------------------------------+
| L3 AGENT（skill code / 调用方）                               |
|   * 选 endpoint_id                                            |
|   * 构造 call params / workflow params                        |
|   * 不管 login / token / 401 retry                            |
+---------------------------------------------------------------+
                         |  JSON-RPC over stdio（agent 无 auth 概念）
                         v
+---------------------------------------------------------------+
| L2 x-cli serve（per-skill 进程）                              |
|   * 启动时加载 auth.yaml                                       |
|   * 如有 login 配置 -> 自动登录拿 token                        |
|   * Token 进程内 cache（不写盘）                               |
|   * 每请求注入 Bearer header                                   |
|   * 401 -> 自动 re-login + retry 一次                          |
|   * Schema 校验 / 错误码 / workflow 编排                       |
+---------------------------------------------------------------+
                         |  HTTP（login + business）
                         v
+---------------------------------------------------------------+
| L1 BACKEND（OpenAPI server）                                  |
|   * /login /refresh + 业务 endpoints                          |
+---------------------------------------------------------------+
```

## L3 Agent —— 干啥 / 不干啥

**干这些**：

| 行为 | 说明 |
|---|---|
| 选 endpoint_id | agent 读 `endpoints/<id>.md` 选要调的接口 |
| 构造 call params | body / path_params / query / headers |
| 调 `call` / `workflow.run` | 发 JSON-RPC 给 serve 等结果 |
| 决定业务流 | "先列 dashboard 再点开看图表" |
| 处理业务结果 | 解析 response.body 给用户看 |

**不干这些**（越界 → 文档错）：

- 拿 token / 调 login 端点（用 curl 或别的）
- 解析 401 后决定要重登
- 重启 serve 进程
- 读 / 写 `auth.yaml`
- 把 token 存到 env var 或 workflow.yaml

## L2 x-cli serve —— 干啥 / 不干啥

**干这些**：

| 行为 | 说明 |
|---|---|
| 加载 `.x-cli/ir.json` | 启动时读 |
| 加载 `auth.yaml` | v0.1+ 新增：有就按配置自动登录 |
| Token 进程内 cache | 登录成功存内存，进程生命周期有效 |
| 每请求注入 auth header | Bearer 或自定义 |
| 401 重试 | `refresh.on_401=true` 时自动 re-login + 重试原请求 1 次 |
| Schema 校验 | request / response |
| Error code 结构化 | -32700 ~ -32012 |
| Workflow DAG 执行 | 拓扑序 |
| 加载 `workflows/*.yaml` | 启动时按文件加载 |

**不干这些**：

- 把 token 写盘（不持久化，重启 serve = 重登）
- 调度 cron（"5 分钟后自动 refresh"）
- 主动探测后端 /health
- 多用户 / 多 session 隔离（一个 serve 进程 = 一个 token；多 session = 多进程）
- 接管业务结果解析（agent 干这个）

## L1 Backend —— 干啥（参考用）

- 暴露 login / refresh 端点
- 暴露业务 endpoints
- 返回 JWT（含可选 `expires_in` / `refresh_token`）
- 返回 401（token 失效）

## Session 生命周期决策表

| 场景 | 谁处理 | 怎么走 |
|---|---|---|
| serve 启动 | x-cli | 读 `auth.yaml` -> 调 login -> 拿 token -> 注入 |
| 业务请求 | x-cli | 自动注入 Bearer header -> 转 backend |
| 后端返回 401 | x-cli | re-login -> 重试原请求 1 次 |
| re-login 后仍 401 | x-cli | 把 401 透传给 agent（agent 报错给用户）|
| serve 进程退出 | OS | token 失内存，下次启动重登 |
| agent 想强制 refresh | **v0.1 不支持** | v0.2+ 计划加 `auth.refresh` JSON-RPC method |
| token 疑似泄露 | 用户 | 重启 serve（不写盘 = 新 token）|
| 想看当前 token 状态 | **v0.1 不支持** | v0.2+ 计划加 `auth.status` |

## Auth 模式的归属

| 模式 | 配置在哪 | 谁负责拿 token | 谁注入 header |
|---|---|---|---|
| 无 auth（内部 / mock）| 不需要 `auth.yaml` | - | x-cli 不注入 |
| 静态 token（API key）| `auth.yaml: token.bearer` 或 `--auth-bearer` flag | 用户一次性提供 | x-cli |
| 自动登录（JWT）| `auth.yaml: token.login` | **x-cli 自动** | x-cli |
| Cookie / Session | **v0.1 不支持** | - | - |

## 为什么这么切（vs 旧模式）

| 旧模式（v0.1 之前文档描述）| 问题 | 新模式（v0.1+ 重构）|
|---|---|---|
| agent 用 curl 拿 token | 每个 agent 造一遍轮子 | agent 零 auth 代码 |
| token 存 env var | shell 重启丢 | token 在 serve 进程内 |
| 401 -> agent 重启 serve | 不可移植 | x-cli 自动 re-login |
| token 写进 workflow.yaml | git 泄漏风险 | 不在 git 里（auth.yaml gitignored）|
| serve 单 token 启动 | 没法 refresh | 自动 re-login 一次 |
| agent 不知道 x-cli 边界 | 反复造 / 互相覆盖 | 本文档显式声明 |

## 关键不变量（agent 改 skill / 改 x-cli 时不要破）

1. **agent 不碰 token** —— agent 只发 JSON-RPC；不读 / 不写 `auth.yaml`；不解析 401 后做重登决策
2. **x-cli 不写盘 token** —— 重启 serve = 重登（防泄漏 + 简单）
3. **`auth.yaml` 不进 git** —— `auth.example.yaml` 是模板（结构 + 占位符），`auth.yaml` 是实例（明文 creds，gitignored）
4. **session 是 per-serve 进程** —— 多 agent 并发 = 多 serve 进程（每个 serve 独立 token）
5. **401 透传是 fallback** —— re-login 一次还失败 = 真错，让用户看

## v0.2+ 路线图

| 功能 | 状态 | 说明 |
|---|---|---|
| `auth.refresh` JSON-RPC method | 计划 | agent 主动强制 refresh |
| 主动 refresh（按 expires_in 提前）| 计划 | 不再只等 401 |
| Cookie / Session 支持 | 计划 | 取代现 auth-patterns.md Mode 4 的 workaround |
| Keychain 集成 | 计划 | Windows Credential Manager / macOS Keychain / Linux secret-service |
| OAuth2 client credentials | 计划 | 不需用户介入的 server-to-server |
| 多 token per serve | 视需求 | "A 账号读 + B 账号写" |

## 跟其他文档的关系

| 文档 | 关系 |
|---|---|
| `auth-patterns.md` | 列具体 auth 写法，所有写法都在本 scope 内 |
| `commands.md` | x-cli CLI flag 列表（在本 scope 内）|
| `troubleshooting.md` | 401 / 过期等异常处理，遵循本 scope 的 401 决策 |
| `examples/5-auth-yaml.md` | 端到端范例（v0.1 重构后新增），演示 `auth.yaml` 完整用法 |
| 业务 skill 的 `SKILL.md` | 由 emitter 生成；头部 "本 skill 支持 `auth.yaml` 引导" 段由 emitter 写入 |

## 决策速查（"这事谁干"）

| 我想做的 | 谁干 | 怎么走 |
|---|---|---|
| 调一个 endpoint | agent | `call` method |
| 跑多步业务 | agent | `workflow.run` method |
| 改鉴权策略 | 用户（编辑 `auth.yaml`）+ x-cli（按配置执行）| 不需要改 agent 代码 |
| token 过期了 | x-cli | 自动 re-login + retry |
| 加新的 endpoint | 用户（用 `x emit` 重跑）| 不需要改 agent 代码 |
| 后端换了 login 端点 | 用户（改 `auth.yaml: token.login.request.url`）| agent 无感 |