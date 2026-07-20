# x-cli

[![CI](https://github.com/myg133/x-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/myg133/x-cli/actions/workflows/ci.yml)
[![Latest tag](https://img.shields.io/github/v/tag/myg133/x-cli)](https://github.com/myg133/x-cli/tags)
[![License](https://img.shields.io/github/license/myg133/x-cli)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://www.rust-lang.org)

**把后端 OpenAPI 转成 agent 可加载的 skill。**

AI agent 通过 skill 调用外部工具，skill 内部通常用 CLI 命令落地。x-cli 的想法是：**让后端 OpenAPI 文档成为 skill 的单一来源**——读 OpenAPI、自动整理、生成多平台 skill 描述、agent 一加载就能调后端 HTTP。

```
   OpenAPI  ────►  IR (x-cli-core)
   (yaml/json)        │
                      ├─► Markdown skill  (给人看、agent 参考)
                      ├─► Anthropic skill  (Claude 风格 frontmatter)
                      └─► OpenAI tools     (function calling JSON)
                                    │
                                    └─►  agent 加载、调 x serve、调后端
```

## 它解决什么

- **作者侧**：给一份 OpenAPI → 自动生成 skill 目录
- **运行侧**：agent 加载 skill 后，通过 stdio JSON-RPC 调 x-cli，x-cli 转发到后端
- **多步场景**：用 `workflow.yaml` 描述多步任务（带 DAG 依赖、$input / $steps 引用），agent 一次 `workflow.run` 拿结果
- **多平台**：三种输出格式（markdown / Anthropic / OpenAI tools）覆盖主流 agent 平台

## 安装 / 构建

需要 Rust 1.75+。

```bash
git clone <repo>
cd x-cli
cargo build --release
# 产物：target/release/x.exe (Windows) / target/release/x (Unix)
```

放进 `PATH` 后 `x` 命令全局可用。

## 快速开始

```bash
# 1. 把 OpenAPI 转成 skill（默认 markdown 格式）
x emit examples/petstore.yaml --out ./out/petstore-skill

# 2. 启动 JSON-RPC 服务（agent 调这个）
x serve --skill ./out/petstore-skill

# 3. agent 调一个 endpoint
echo '{"jsonrpc":"2.0","id":1,"method":"call","params":{
  "endpoint_id":"pet__get__pets_petId",
  "path_params":{"petId":"123"}
}}' | x serve --skill ./out/petstore-skill
```

## 命令

| 命令 | 作用 |
|---|---|
| `x parse <openapi>` | 解析并打印 IR（debug 用） |
| `x emit <openapi> --out DIR [--workflow wf.yaml]... [--format md\|anthropic\|openai]` | 生成 skill 目录 |
| `x serve --skill DIR [--base-url URL]` | 启动 stdio JSON-RPC 服务 |

## 三种输出格式

### Markdown（默认）

```
out/petstore-skill/
├── SKILL.md              # 总索引（业务域、接口列表、调用约定）
├── endpoints/
│   ├── pet__get__pets.md
│   └── pet__get__pets_petId.md
└── .x-cli/
    └── ir.json          # runtime 加载用
```

适合：人读、agent 参考、嵌入文档站。**带完整 `$ref` 解析后的 schema 树**。

### Anthropic

```bash
x emit examples/superset.json --out ./out/skill --format anthropic
```

输出单个 `SKILL.md`，含 YAML frontmatter：

```yaml
---
name: Superset
description: API 版本 1，276 个接口，覆盖 ... 当用户问及这些业务时使用此 skill。
---
```

`description` 字段是关键——Claude 看这个决定何时加载 skill。

适合：Claude 系 agent（Anthropic API / Claude Code / .claude/skills/）。

### OpenAI Tools

```bash
x emit examples/petstore.yaml --out ./out/petstore-tools --format openai
```

输出单个 `functions.json`：

```json
{
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "pet__get__pets_petId",
        "description": "GET /pets/{petId} — 获取一只宠物",
        "parameters": {
          "type": "object",
          "properties": {
            "petId": { "type": "string", "description": "宠物 ID" }
          },
          "required": ["petId"]
        }
      }
    }
  ]
}
```

适合：OpenAI function calling / ChatGPT plugins / 其他 tools 协议。

---

## workflow：多步任务

简单场景下 agent 自己串多个 endpoint 调用就行。**复杂场景**（创建订单 + 拿订单详情 + 调支付）用 workflow.yaml 描述，agent 一次 `workflow.run` 拿结果。

### 基础 workflow

```yaml
name: 买宠物并查询订单
description: |
  1. 创建一只宠物
  2. 用返回的 id 查订单
inputs:
  - name: petName
    type: string
    default: "fluffy"
steps:
  - name: create_pet
    endpoint: pet__post__pets
    inputs:
      body:
        name: "$input.petName"
  - name: get_pet
    endpoint: pet__get__pets_petId
    inputs:
      path_params:
        petId: "$steps.create_pet.response.body.id"
```

emit 时加 `--workflow`，serve 时不用特别处理：

```bash
x emit examples/petstore.yaml --out ./out/skill --workflow examples/petstore-workflow.yaml
```

agent 调一次：

```json
{
  "method": "workflow.run",
  "params": {
    "workflow": "买宠物并查询订单",
    "inputs": { "petName": "fluffy" }
  }
}
```

返回：

```json
{
  "result": {
    "status": "ok",
    "steps": [
      { "name": "create_pet", "endpoint": "pet__post__pets", "status": 201, "body": {...} },
      { "name": "get_pet", "endpoint": "pet__get__pets_petId", "status": 200, "body": {...} }
    ],
    "outputs": {...}  // = 最后一步 body
  }
}
```

### InputRef 三种

```yaml
inputs:
  body:
    # 1. 引用工作流外部输入
    name: "$input.petName"

    # 2. 引用上一步响应
    petId: "$steps.create_pet.response.body.id"

    # 3. 静态值（其他都算静态）
    tag: "demo"
```

### DAG 依赖（拓扑执行）

数组顺序默认串行。**用 `depends_on` 显式声明依赖 → runtime 按拓扑序执行**。

```yaml
name: 平行获取宠物和订单
steps:
  - name: summarize
    depends_on: [fetch_pet, fetch_order]
  - name: fetch_pet
  - name: fetch_order
```

按拓扑序：`fetch_pet` + `fetch_order` 同层（按数组位置），`summarize` 在它们之后。

校验：
- 未知引用 → 拒绝
- 自依赖 → 拒绝
- 环 → 拒绝

---

## 错误码

| 码 | 含义 |
|---|---|
| -32700 | JSON 解析错误 |
| -32600 | 无效的 JSON-RPC 请求 |
| -32601 | Method 不存在 |
| -32602 | 参数不合法 |
| -32001 | 端点不存在 |
| -32002 | HTTP 错误（连接 / 超时） |
| -32010 | workflow 不存在 |
| -32011 | workflow step 失败（HTTP 4xx/5xx） |
| -32012 | workflow 缺外部输入 |

---

## 实际能力验证

- ✅ **OAS 3.0 / 3.1** 兼容（自动 3.0 → 3.1 转换：`parameters[].content` → `parameters[].schema`）
- ✅ **`$ref` 递归解析 + 循环引用** 不爆栈
- ✅ **真实大文档**：Apache Superset（1.27 MB / 276 endpoint / 305 `$ref`）0.19 秒解析
- ✅ **多 emitter**：3 种格式，1 个 binary
- ✅ **workflow DAG**：依赖校验、环检测、拓扑执行
- ✅ **60 个测试**，0 网络依赖（CI 友好），0.04 秒跑完

---

## 工程结构

```
crates/
├── x-cli/                # 主二进制 x（emit / serve / parse）
├── x-cli-core/           # IR + OpenAPI 解析 + protocol + workflow 解析
├── x-cli-runtime/        # JSON-RPC transport + HTTP 客户端 + WorkflowExecutor
└── x-cli-emitter-md/     # markdown / anthropic / openai 三种 emitter
```

依赖方向：`x-cli` → `{core, runtime, emitter-md}`，runtime 和 emitter 都基于 core 的 IR。

详细架构和 ABI 约定见 [ARCHITECTURE.md](./ARCHITECTURE.md)。

## 开发

```bash
# 跑全部测试
cargo test --workspace

# 跑某个 crate
cargo test -p x-cli-core
cargo test -p x-cli-emitter-md
cargo test -p x-cli-runtime

# Lint
cargo clippy --workspace

# 用真实 OpenAPI 验证
x emit examples/superset.json --out ./out/skill --format anthropic
```

## ABI 不变量

x-cli 守住的两条不变量（保证后续改造不破坏 skill ↔ x-cli 协议）：

1. **IR 独立 crate，emitter 用 trait 抽象** — 加 emitter 不改 core
2. **skill ↔ x-cli 是 JSON-RPC over stdio** — 不是 CLI 拼参数串，给后续 sidecar / sandbox / 多 agent 留空间

## License

MIT OR Apache-2.0
