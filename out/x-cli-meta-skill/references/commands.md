# x-cli 命令参考

> 来源：`crates/x-cli/src/main.rs` 的 clap 定义 + `ARCHITECTURE.md`。事实部分，未做推断。
>
> **前提**：binary 已通过 npm 装到 PATH（`pnpm install -g @myg133/x-cli`）。

## 全局

```bash
x --version     # 0.1.0
x --help        # 子命令列表
```

## `x parse <openapi>`

**作用**：解析 OpenAPI 并把 IR（`ApiSpec`）以 pretty JSON 打印到 stdout。**纯 debug 用**，不写文件。

```bash
x parse examples/petstore.yaml
x parse examples/superset.json | head -100
```

输出形状（节选）：

```json
{
  "title": "Pet Store",
  "version": "1.0.0",
  "base_url": "https://petstore.example.com/v1",
  "domains": [{"name": "pet", "description": "..."}],
  "endpoints": {
    "pet__get__pets_petId": {
      "id": "pet__get__pets_petId",
      "domain": "pet",
      "method": "get",
      "path": "/pets/{petId}",
      "params": [...],
      "request_body": null,
      "responses": [...]
    }
  }
}
```

## `x emit <openapi> --out DIR`

**作用**：把 OpenAPI 转成 skill 目录。这是**最常用的命令**。

```bash
x emit <file> --out <dir>                              # markdown（默认）
x emit <file> --out <dir> --workflow <wf.yaml>         # 带 workflow
x emit <file> --out <dir> --workflow wf1.yaml --workflow wf2.yaml  # 多个 workflow
x emit <file> --out <dir> --format anthropic            # Claude / Anthropic 风格
x emit <file> --out <dir> --format openai               # OpenAI function calling
x emit <file> --out <dir> --format mcp                  # MCP 协议（mcp-tools.json）
x emit <file> --out <dir> --format mcp --cli-tools <yaml>  # MCP + CLI 工具
```

### 默认输出位置

业务 skill 默认写到 `--out` 指定的位置。**推荐**写到 meta-skill 内的 `./generated/`：

```bash
x emit examples/petstore.yaml --out ./generated/petstore-skill
x emit examples/superset.json  --out ./generated/superset-skill
```

详见 `distribution.md`。

### `--format` 三种

| 值 | 产物 | 给谁用 |
|---|---|---|
| `markdown`（默认）| `SKILL.md` + `endpoints/<id>.md`（每接口一份）+ `.x-cli/ir.json` | 人读 / agent 参考 / 通用 |
| `anthropic` | 单 `SKILL.md`（含 frontmatter `name` + `description`）| Claude Code / Anthropic API |
| `openai` | 单 `functions.json`（`{ "tools": [...] }`）| OpenAI function calling |
| `mcp` | `mcp-tools.json` + `mcp-server.json` + `.x-cli/cli.json`（有 CLI 工具时）| MCP 协议客户端 |

**重要**：**任何 format 都会写出 `.x-cli/ir.json`**，因为 `x serve` 跑 workflow 时需要 IR 缓存。

### 进度输出

成功时（stdout）：

```
✓ 解析 N 个接口、M 个工作流，格式 FMT 写入 <DIR>
```

N = `ApiSpec.endpoints.len()`，M = workflow 文件数。**业务域数不在这里**（要去 `SKILL.md` 里看）。

### workflow 引用校验

emit 时如果 workflow.yaml 引用了不存在的 endpoint_id，**会 bail**（exit 1，stderr 报错）：

```
Error: workflow `买宠物并查询订单` 引用了不存在的 endpoint `pet__post__pets_typo`
```

## `x serve --skill DIR`

**作用**：从 skill 目录启动 stdio JSON-RPC 服务。**agent 调的就是这个**。

```bash
x serve --skill ./generated/petstore-skill
x serve --skill ./generated/petstore-skill --base-url https://api.real.com
x serve --skill ./generated/petstore-skill --auth-bearer "$TOKEN"
x serve --skill ./generated/petstore-skill \
    --auth-header "X-API-Key=xxx" \
    --auth-header "X-Tenant=acme"
```

### Flag

| Flag | 多次 | 说明 |
|---|---|---|
| `--skill <DIR>` | 否 | skill 目录（必须含 `.x-cli/ir.json`）|
| `--base-url <URL>` | 否 | 覆盖 IR 里的 base URL（`servers[0].url`）|
| `--auth-bearer <TOKEN>` | 是 | 自动加 `Authorization: Bearer <TOKEN>` |
| `--auth-header <KEY=VALUE>` | 是 | 加自定义 header，格式 `KEY=VALUE` |
| `--mcp` | 否 | 启动 MCP 协议（而非自定义 JSON-RPC），方法：initialize / tools/list / tools/call |

### 启动输出

```text
✓ 加载 2 个工作流                    # 有 workflow 时
✓ 注入 1 个认证 header               # 有 auth 时
# 等待 stdin 输入...
```

**注意**：启动消息走 **stdout**（不是 stderr）。如果 agent 用管道读 stdout，**这些启动消息会和响应混在一起**。建议 agent 启动后立刻发一个 `ping` 探测，把前面的启动消息当噪声忽略。

### 关闭 stdin = 退出

serve 是 blocking 的；EOF on stdin 触发退出。**agent 写完一个请求后必须 flush + 写换行**，否则 serve 不会处理。

## 调 API vs curl（核心决策）

**用 x 调 API** 的场景（占 80%）：

- 调有 OpenAPI 文档的后端（schema 校验 + auth 注入 + 错误结构化）
- 需要 `workflow.run` 多步串联
- 调 Superset / GitLab / 自建网关这种有 JWT / API Key 的后端

**用 curl** 的场景（占 20%）：

- 不是 OpenAPI 后端（自己写 HTTP / 临时测试）
- 需要流式（chunked transfer / SSE / WebSocket）
- meta-skill / 业务 skill 都没装 + 临时应急

**不要**用 curl 调有 OpenAPI 的后端 —— 失去 schema 校验 + auth 注入 + 错误结构化。

## JSON-RPC 协议

每行一条 JSON 请求，每行一条 JSON 响应。

### 三种 method

| Method | Params | 用途 |
|---|---|---|
| `ping` | — | 心跳 / 探活，返回 `{ "pong": true }` |
| `call` | `{ endpoint_id, path_params, query, headers, body }` | 调单个 endpoint |
| `workflow.run` | `{ workflow, inputs }` | 跑多步 workflow（端到端结果）|

### `call` 完整示例

请求：

```json
{"jsonrpc":"2.0","id":1,"method":"call","params":{
  "endpoint_id":"pet__get__pets_petId",
  "path_params":{"petId":"123"},
  "query":{},
  "headers":{},
  "body":null
}}
```

成功响应：

```json
{"jsonrpc":"2.0","id":1,"result":{
  "status":200,
  "headers":{"content-type":"application/json",...},
  "body":{...}
}}
```

失败响应（HTTP 错误）：

```json
{"jsonrpc":"2.0","id":1,"error":{
  "code":-32002,
  "message":"error sending request for url (...)"
}}
```

### `workflow.run` 完整示例

请求：

```json
{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{
  "workflow":"买宠物并查询订单",
  "inputs":{"petName":"fluffy"}
}}
```

成功响应（`outputs` = 最后一步 body）：

```json
{"jsonrpc":"2.0","id":1,"result":{
  "status":"ok",
  "steps":[
    {"name":"create_pet","endpoint":"pet__post__pets","status":201,"body":{...}},
    {"name":"get_pet","endpoint":"pet__get__pets_petId","status":200,"body":{...}}
  ],
  "outputs":{...}
}}
```

失败响应（step HTTP 错误）：

```json
{"jsonrpc":"2.0","id":1,"error":{
  "code":-32011,
  "message":"step `create_pet` HTTP failed: ...",
  "data":{"step":"create_pet","endpoint":"pet__post__pets","status":500,"body":{...}}
}}
```

## 错误码速查

| 码 | 含义 |
|---|---|
| `-32700` | JSON 解析错误 |
| `-32600` | 无效的 JSON-RPC 请求 |
| `-32601` | Method 不存在 |
| `-32602` | 参数不合法 |
| `-32001` | 端点不存在（`endpoint_id` 拼错）|
| `-32002` | HTTP 错误（连接 / 超时 / DNS）|
| `-32010` | workflow 不存在（`workflow.name` 没找到）|
| `-32011` | workflow step 失败（HTTP 4xx/5xx）|
| `-32012` | workflow 缺外部输入 |

完整排错见 `troubleshooting.md`。

## MCP 协议（--mcp 模式）

`x serve --mcp` 使用标准 MCP（Model Context Protocol）取代自定义 JSON-RPC。

### 方法

| MCP Method | 用途 | x-cli 映射 |
|---|---|---|
| `initialize` | 握手（返回 capabilities） | 返回 server info + tool 支持声明 |
| `notifications/initialized` | 客户端通知（无响应） | 接受后标记为已初始化 |
| `tools/list` | 获取所有可用工具 | 返回 HTTP endpoints + workflows + CLI tools |
| `tools/call` | 调用某个工具 | 路由到 HTTP call / workflow.run / CLI 子进程 |

### `tools/list` 响应示例

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "pet__get__pets",
        "description": "GET /pets — 获取宠物列表",
        "inputSchema": {
          "type": "object",
          "properties": {},
          "required": []
        }
      },
      {
        "name": "workflow.买宠物并查询订单",
        "description": "创建宠物然后查订单",
        "inputSchema": {
          "type": "object",
          "properties": {
            "petName": {"type": "string", "description": "宠物名字"}
          },
          "required": ["petName"]
        }
      }
    ]
  }
}
```

### tool 命名约定

| 来源 | 命名规则 | 示例 |
|---|---|---|
| HTTP endpoint | `endpoint.id`（`<domain>__<method>__<path>`）| `pet__get__pets_petId` |
| workflow | `workflow.<name>` | `workflow.买宠物并查询订单` |
| CLI tool | `CliTool.name`（用户定义）| `kubectl_get_pods` |
