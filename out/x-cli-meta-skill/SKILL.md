---
name: x-cli-skill-factory
description: >-
  当用户希望 agent 调 OpenAPI 后端 HTTP 接口（拿数据 / 测接口 / 调业务），或用户提供 OpenAPI 文档希望"造可调用的 skill"时使用此 skill。此 skill 教 agent 通过 npm 自动安装 x-cli 二进制到 PATH（pnpm install -g @myg133/x-cli），然后用裸 x 命令把任意 OpenAPI 3 后端转成 markdown / Anthropic / OpenAI tools / MCP 四种格式的 skill，并通过 stdio JSON-RPC 或 MCP 协议转发到后端 HTTP / CLI 子进程。**比 curl 优势：自动 schema 校验 / auth header 注入 / 错误结构化 / workflow 串联 / 多步 CLI 编排**。如果用户已经有现成的业务 skill 在 generated/，直接加载业务 skill 而非本 skill。
---

# x-cli skill factory

> 这个 skill 不是调业务后端的，是**造业务 skill 的 + 教 agent 用 x 调 API**。agent 接到"调 X 后端接口 / 把 OpenAPI 转成 skill"的请求时，加载这个。

## 立即执行（agent 第一步：装 binary）

本 skill 通过 **npm 分发 binary**。agent 接到本 skill 后**先**装 binary 到系统 PATH：

```bash
# 三个等价，挑一个
pnpm install -g @myg133/x-cli
# 或
npm install -g @myg133/x-cli
# 或
yarn global add @myg133/x-cli
```

**验证**：

```bash
x --version
# x 0.1.0
```

如果 `x --version` 报"command not found" → **重开 shell**（PATH 修改对新 shell 才生效）。重开后**还**报 → 平台不支持（当前只 Windows x64；macOS/Linux 见 `packages/x-cli-npm/README.md`）。

**为什么必须装**：x-cli 不是 meta-skill 自带的 binary —— 单独走 npm 分发（`@myg133/x-cli` 包）。meta-skill 只装文档，binary 通过 npm 自动装到 `node_modules/.bin/x`（pnpm 设计上就在 PATH 里）。

## 何时加载

匹配以下任一即加载：

- 用户提供了 OpenAPI 文件 / URL，并说"做 skill" / "转一下" / "让 agent 能调"
- 用户说"用 x-cli 处理这个 OpenAPI"
- 用户问"怎么把后端 OpenAPI 变成 agent skill"
- 用户希望 agent 调 OpenAPI 后端 HTTP 接口（拿数据 / 测接口 / 调业务）—— **用 x 替代 curl**
- 已有 skill 加载失败，用户说"重新生成" / "OpenAPI 变了，刷新一下"

**不匹配**（不要加载）：

- 用户已经有现成的业务 skill 目录（加载那个业务 skill —— 它知道自己怎么调）
- 用户只想跑一个**非 OpenAPI** 的 HTTP 请求（直接 curl 即可）
- 用户问 x-cli 的实现细节（直接看项目根的 `ARCHITECTURE.md`）
- 平台不是 Windows x64 且用户没装 Rust 工具链（x-cli npm 包只 Windows x64）

## 工作流（5 步）

```
OpenAPI 源 ──1──> emit skill ──2──> 选鉴权策略 ──3──> 起 serve ──4──> 验证 ──5──> 交付
```

1. **拿到 OpenAPI 源** —— URL 就 `curl` 下来；用户贴的就保存到临时文件；本地有就 `read_to_string` 确认能读。**如果需要支持 CLI 工具**（如 kubectl / docker），分析 CLI 文档后按 CliSpec schema 写 `cli-tools.yaml`（schema 见仓库 `crates/x-cli-core/src/ir.rs` 的 `CliSpec` / `CliTool` / `CliArg`）。
2. **`x emit` 生成 skill 目录** —— 默认 `markdown` 格式；agent 平台是 Claude 用 `anthropic`；OpenAI 用 `openai`；**MCP 协议用 `--format mcp`**（统一输出 `mcp-tools.json` + `mcp-server.json`）。如果提供了 `cli-tools.yaml`，**加 `--cli-tools cli-tools.yaml`**。**任何 format 都会写 `.x-cli/ir.json`，serve 时要靠它**。**业务 skill 默认输出到 ./generated/<name>/**（可用 --out 覆盖,详见 distribution.md）。**v0.1+ emit 时还会写 auth.example.yaml 模板**（用户填 creds 后 cp 成 auth.yaml）。
3. **配鉴权（如需）** —— 默认 auth.yaml 不存在 = 无 auth。需要登录就 cp auth.example.yaml auth.yaml + 填 token.login.request.url / body / response.token_path。**Agent 不写鉴权代码** —— token 拿 / 401 重试 / serve 重启全归 x-cli（scope 详见 scope.md，4 种 auth 模式见 auth-patterns.md）。
4. **启动 `x serve`** —— 默认启动 JSON-RPC 服务；**MCP 模式加 `--mcp`**（初始化时 agent 发 `initialize` 握手，然后走 `tools/list` / `tools/call`）。如果 emit 时带了 CLI 工具，**MCP 模式下 `tools/call` 自动路由到 CLI 子进程**。**`serve` 是长跑进程，反复发请求都行**。
5. **验证 + 交付** —— 跑一次 `ping`（JSON-RPC）或 `tools/list`（MCP）测连通；再跑一个真实 tool 验证；把生成的 skill 目录路径告诉用户。

## 调 API 速查（最常用场景）

如果你想调 API（不是造 skill），三步走：

```bash
# A) 默认 JSON-RPC 模式
# 1. emit（造业务 skill，一次）
x emit <openapi.yaml> --out ./generated/<name>-skill

# 2. serve（启 JSON-RPC，长跑）
x serve --skill ./generated/<name>-skill [--base-url URL] [--auth-bearer TOKEN]

# 3. call（通过 stdin 发请求）

# B) MCP 模式（推荐给 agent 使用）
# 1. emit 时指定 MCP 格式 + 可选 CLI 工具
x emit <openapi.yaml> --out ./generated/<name>-skill --format mcp --cli-tools cli-tools.yaml

# 2. serve 用 MCP 协议
x serve --mcp --skill ./generated/<name>-skill

# 3. agent 通过 MCP 调 tools（tools/list → tools/call）
echo '{"jsonrpc":"2.0","id":1,"method":"call","params":{"endpoint_id":"<id>","path_params":{}}}' | x serve --skill ./generated/<name>-skill
```

**对比 curl**：

| 维度 | curl | x serve + x call |
|---|---|---|
| Schema 校验 | ❌ 自己写 | ✅ IR 里有完整 schema |
| Auth header | ❌ 每次写 | ✅ serve 启动时注入 |
| 错误结构化 | ❌ 解析 text | ✅ JSON-RPC error code + data |
| Workflow 串联 | ❌ 手动串 | ✅ `workflow.run` 一次拿多步 |

完整对比 + 反模式见 `commands.md` / `auth-patterns.md`。

## 文件索引

按需查阅，不要一次性全读：

| 文件 | 何时读 |
|---|---|
| scope.md | **首次读 meta-skill 时先读** —— x-cli / agent / backend 三层边界 + session 生命周期 |
| commands.md | 不确定某个 x-cli 子命令的 flag / 输出格式时 |
| `auth-patterns.md` | 步骤 3 选鉴权策略时 |
| `workflow-patterns.md` | 业务需要多步串联（不是单 endpoint）时 |
| `troubleshooting.md` | 步骤 5 验证失败 / 401 / endpoint 找不到时 |
| `distribution.md` | **首次拿到这个 skill 时**先读——知道怎么打包、怎么分发、业务 skill 输出到哪 |
| `examples/1-petstore-no-auth.md` | 无 auth 的最简参考实现 |
| `examples/2-superset-jwt.md` | JWT 鉴权 + base URL override 范例（**最常见**）|
| `examples/3-httpbin-workflow.md` | workflow.yaml 多步范例 |
| `examples/4-large-spec.md` | 1MB+ / 200+ endpoint 的大文档注意事项 |
| `examples/5-auth-yaml.md` | **v0.1 推荐**：auth.yaml 自动登录 + 401 retry 端到端 |

## 关键约束

- **binary 通过 npm 装**，不是 meta-skill 自带。`pnpm install -g @myg133/x-cli` 后才能用 `x` 命令。
- **当前只 Windows x64**。POSIX 上需要先 `cargo build --release` 或等 cross-compile CI。
- **x-cli 的 OpenAPI 解析对 3.0 / 3.1 都支持**（3.0 自动转 3.1，query/header schema 不会丢）。`oas3 0.16` 按 3.1 解析。
- **`$ref` 循环引用自动检测**，不会爆栈。Superset 实测 305 个 ref、0.19 秒解析。
- **三种输出格式互不冲突**：可以同时 emit 三份喂给三种 agent。
- **serve 是 stdio JSON-RPC**，stdout 数据 / stderr logging。**关闭 stdin = serve 退出**。
- **自包含文档**：业务 skill 产物在 `generated/`。meta-skill 文档可独立分发，binary 走 npm。

## 给 agent 的硬性提示

1. **不要在测试 / fixture 里写真实网络调用**。所有验证用本地 mock server 或内联字符串。
2. **不要给 x-cli-core 加 tokio/reqwest 依赖**。core 是同步纯计算层。
3. **不要改 Endpoint.id 格式**（`<Domain>__<method>__<sanitized_path>`）。已发布的 skill 全靠这个 id。
4. **不要改 JSON-RPC 错误码数值**。agent 端 hardcode 了这些码。
5. **不要在 workflow.yaml 里写 token** —— token 进 git，泄漏风险。用 `serve --auth-bearer` 启动时注入。
6. **不要纠结 meta-skill 里有没有 binary** —— binary 永远在 npm 包（`@myg133/x-cli`），不在 meta-skill 目录。
7. **Agent 不要碰 token 生命周期**（不读 auth.yaml、不解析 401 重启 serve、不手写 curl 拿 token）—— 统一由 x-cli serve 按 auth.yaml 自动管理。三层 scope 划分详见 scope.md。

## 业务逻辑推断（透明给 agent 看）

这个 skill 的内容**一半来自 x-cli 项目文档**（commands / 错误码 / 工作流语法 —— 事实），**一半从项目示例 + README 推断**（典型后端模式 / 常见失败模式 / 排错套路 —— 经验）。当推断部分与实际后端不符时，以实际后端的 OpenAPI 文档为准。
