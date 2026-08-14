---
name: x-cli-meta-skill
description: >-
  x-cli 造 skill 的 meta-skill，用于 Layer 1（分析构建层）。当用户提供 OpenAPI / CLI 文档、
  希望"让 agent 能调这个后端"时加载此 skill。此 skill 教 agent 如何分析后端文档、理解业务域、
  设计 workflow 编排、编写 CliSpec，最后用 x emit 打包成业务 skill。x 不是业务分析工具，是打包工具。
  业务抽象（domain 划分、workflow 编排、CLI 封装）需要 agent 的智能参与。
---

# x-cli skill factory

> 这个 skill 不是调业务后端的，是**教 agent 造业务 skill + 设计业务抽象**的。
> agent 接到"把 OpenAPI / CLI 转成 skill"的请求时，加载这个。

---

## 两层架构速览

```
Layer 1（分析构建层）──── 你 + 这个 meta-skill
│
├── 用户提供 OpenAPI / CLI 文档
├── 你分析文档，理解业务语义
├── 你设计 domain 划分、workflow 编排
├── 你写 workflow.yaml / CliSpec YAML
├── 你用 x emit 打包 → 产出业务 skill 目录
│
                ▼
Layer 2（运行层）────── 业务用户 + 业务 skill
│
├── 加载业务 skill
├── x serve 启动 MCP 服务
├── 通过 MCP 调业务工具完成目标
```

**你在这里扮演 Layer 1 的角色**——分析业务、设计抽象、用 x 打包。

---

## 第一步：装 binary

```bash
npm install -g @myg133/x-cli
```

验证：

```bash
x --version
```

---

## 第二步：理解两种源（OpenAPI / CLI）是平等的

**OpenAPI 和 CLI 工具是平行的一等公民**，走完全相同的两层逻辑。区别只是源不同，你的分析方式不同，但到 workflow 抽象和 `x emit` 打包的流程是一样的。

```
源类型         │ 你分析什么                     │ 产出物
────────────────┼────────────────────────────────┼──────────────────────
OpenAPI 规范    │ domain、endpoint、参数、响应    │ IR 自动解析
CLI 工具文档    │ 命令、子命令、flag、位置参数    │ 你写 CliSpec YAML
```

无论是 OpenAPI 还是 CLI，最终都汇入同一个 workflow 抽象：

```
OpenAPI endpoint ──┐
                   ├──► workflow.yaml（业务编排）──► x emit ──► 业务 skill
CLI 工具命令 ──────┘
```

---

## 第三步：分析源，理解业务

### 3.1 如果是 OpenAPI

```bash
x parse <openapi.yaml>
```

IR 告诉你有 domain、endpoint、参数、响应。

### 3.2 如果是 CLI 工具

CLI 工具没有 OpenAPI 规范。你需要**阅读工具文档**，然后写 CliSpec YAML。

CliSpec YAML 的格式：

```yaml
tools:
  - name: 查询数据库
    description: 用 SQL 查询数据库
    command: rsql
    subcommand: ["query"]
    args:
      - name: sql
        description: SQL 查询语句
        flag: --sql
        required: true
      - name: format
        description: 输出格式（json / csv）
        flag: --format
        default: json
      - name: db
        description: 数据库连接串
        flag: --db
        required: true
    output: json

  - name: 上传到对象存储
    description: 上传文件到 minio 对象存储
    command: mc
    subcommand: ["cp"]
    args:
      - name: source
        description: 源文件路径
        position: 0
        required: true
      - name: target
        description: 目标路径（bucket/prefix）
        position: 1
        required: true
      - name: recursive
        description: 递归上传
        flag: --recursive
    output: text
```

`x` 提供 `x parse-cli-spec` 命令校验 CliSpec YAML 的格式：

```bash
x parse-cli-spec <cli-spec.yaml>
```

### 3.3 识别业务域

无论 OpenAPI 还是 CLI，都要理解业务域：

| 业务域 | OpenAPI 实现 | CLI 实现 |
|---|---|---|
| 用户调用统计 | `GET /api/stats/users` | `rsql query --sql "SELECT ..."` |
| 保存到对象存储 | `PUT /api/storage/upload` | `mc cp stats.json minio/bucket/` |

---

## 第四步：设计业务编排（workflow）

这是**业务抽象的核心**。workflow 把多个步骤（无论是 OpenAPI 调用还是 CLI 命令）串成一个业务操作。

### 4.1 典型场景：OpenAPI 多步编排

```yaml
name: 买宠物并查询订单
description: 先查宠物库存，有货就买，然后查订单状态
steps:
  - id: find_pet
    endpoint: pet__get__/pet/{petId}   # ← OpenAPI endpoint
    input:
      petId: "$input.petId"
  - id: place_order
    endpoint: store__post__/store/order  # ← OpenAPI endpoint
    depends_on: [find_pet]
    input:
      petId: "$steps.find_pet.id"
      quantity: 1
  - id: get_order
    endpoint: store__get__/store/order/{orderId} # ← OpenAPI endpoint
    depends_on: [place_order]
    input:
      orderId: "$steps.place_order.id"
output: "$steps.get_order"
```

### 4.2 典型场景：CLI 多步编排

```yaml
name: 统计用户调用并保存到对象存储
description: 每天从数据库统计用户调用信息，保存到 minio 对象存储
steps:
  - id: query_stats
    cli: 查询数据库                    # ← 引用 CliSpec 里的 tool name
    input:
      sql: "SELECT user_id, COUNT(*) AS calls FROM api_logs WHERE date = CURRENT_DATE GROUP BY user_id"
      format: json
      db: "$input.db_connection"       # ← 运行时传入，不同用户不同 DB
  - id: save_to_storage
    cli: 上传到对象存储                 # ← 引用 CliSpec 里的 tool name
    depends_on: [query_stats]
    input:
      source: "$steps.query_stats.stdout"  # ← 上一步的输出作为输入
      target: "$input.bucket/stats/{{date}}.json"
      recursive: false
output: "$steps.save_to_storage"
```

### 4.3 混合场景：OpenAPI + CLI 混合编排

workflow 可以混合 OpenAPI 和 CLI 步骤：

```yaml
name: 生成报表并发布
steps:
  - id: fetch_data
    endpoint: stats__get__/api/stats/daily  # ← OpenAPI
    input:
      date: "$input.date"
  - id: generate_report
    cli: 生成报表命令                      # ← CLI
    depends_on: [fetch_data]
    input:
      data: "$steps.fetch_data.result"
      format: pdf
  - id: upload_report
    cli: 上传到对象存储                    # ← CLI
    depends_on: [generate_report]
    input:
      source: "$steps.generate_report.stdout"
      target: "$input.bucket/reports/{{input.date}}.pdf"
output: "$steps.upload_report"
```

### 4.4 配置差异化（不同用户不同配置）

**CLI 工具的参数（如 DB 连接串、minio endpoint）是运行时传入的**，不是写死在 workflow 里的。不同的终端用户使用同一个业务 skill，但传入不同的配置：

```yaml
# workflow.yaml — 用 $input 引用运行时参数
steps:
  - id: query_stats
    cli: 查询数据库
    input:
      sql: "..."
      db: "$input.db_connection"       # ← 用户 A 传 A 的 DB，用户 B 传 B 的 DB
  
  - id: save_to_storage
    cli: 上传到对象存储
    input:
      target: "$input.bucket_url/stats.json"  # ← 用户 A 传 A 的 minio，用户 B 传 B 的 minio
```

Layer 2 调用时：

```json
// 用户 A
{"method":"workflow.run","params":{
  "workflow":"统计用户调用并保存到对象存储",
  "inputs":{
    "db_connection":"postgres://user_a:pass@a-db:5432/stats",
    "bucket_url":"s3://a-bucket"
  }
}}

// 用户 B
{"method":"workflow.run","params":{
  "workflow":"统计用户调用并保存到对象存储",
  "inputs":{
    "db_connection":"mysql://user_b:pass@b-db:3306/stats",
    "bucket_url":"s3://b-bucket"
  }
}}
```

---

## 第五步：打包成业务 skill

分析完业务、写好 workflow 和 CliSpec 后，用 `x emit` 打包：

```bash
# 纯 OpenAPI
x emit <openapi.yaml> --out ./generated/<name>-skill \
    --workflow <workflow.yaml>

# 纯 CLI 工具
x emit --cli-spec <cli-spec.yaml> --out ./generated/<name>-skill \
    --workflow <workflow.yaml>

# OpenAPI + CLI 混合
x emit <openapi.yaml> --out ./generated/<name>-skill \
    --workflow <workflow.yaml> \
    --cli-spec <cli-spec.yaml>

# 指定格式（默认四种格式全出）
x emit <openapi.yaml> --out ./generated/<name>-skill \
    --workflow <workflow.yaml> \
    --format mcp
```

`x emit` 做的事：
- 解析 OpenAPI 生成 IR
- 读取 CliSpec 嵌入 IR
- 把 workflow 嵌入 IR
- 渲染 SKILL.md（业务 skill 入口）
- 生成 mcp-tools.json（MCP 工具定义）
- 输出 `.x-cli/ir.json`（serve 加载用）

**它不做的事**（需要你来做）：
- ❌ 不会分析业务域——你来做
- ❌ 不会写 workflow——你来做
- ❌ 不会写 CliSpec（CLI 没有规范，需要你读文档后写）——你来做
- ❌ 不会设计业务抽象——你来做
- ❌ 不会理解业务语义——你来做

---

## 第六步：验证生成的 skill

打包后，验证业务 skill 是否可用：

```bash
# 查看 SKILL.md（业务 skill 入口）
cat ./generated/<name>-skill/SKILL.md

# 启动 MCP 服务测试
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | \
    x serve --skill ./generated/<name>-skill

# 测试 OpenAPI workflow
echo '{"jsonrpc":"2.0","id":2,"method":"workflow.run","params":{
  "workflow":"买宠物并查询订单","inputs":{"petId":"1"}
}}' | x serve --skill ./generated/<name>-skill

# 测试 CLI workflow（不同用户不同配置）
echo '{"jsonrpc":"2.0","id":3,"method":"workflow.run","params":{
  "workflow":"统计用户调用并保存到对象存储",
  "inputs":{
    "db_connection":"postgres://user:pass@host:5432/db",
    "bucket_url":"s3://my-bucket"
  }
}}' | x serve --skill ./generated/<name>-skill
```

---

## 第七步：交付

把生成的业务 skill 目录路径告诉用户。用户（或业务 agent）在 Layer 2 加载这个 skill 即可使用。

**不同用户使用同一个业务 skill，传入不同的配置参数即可**——DB 连接串、minio endpoint 都是运行时参数，不是写死在 skill 里的。

---

## 何时加载

匹配以下任一即加载：

- 用户提供 OpenAPI 文件 / URL，说"做 skill" / "让 agent 能调这个后端"
- 用户说"用 x-cli 处理这个 OpenAPI"
- 用户提供 CLI 工具（如 `kubectl`、`rsql`、`mc`），说"让 agent 能调这个 CLI"
- 用户说"把 rsql 和 minio-cli 串起来做一个业务单元"
- 用户问"怎么把后端 OpenAPI 变成 agent skill"
- 已有 skill 加载失败，用户说"重新生成" / "源变了，刷新一下"

**不匹配**（不要加载）：

- 用户已经有现成的业务 skill 目录（加载那个业务 skill——它自己在 Layer 2 运行）
- 用户只想跑一个**非 OpenAPI** 的 HTTP 请求（直接 curl 即可）
- 用户问 x-cli 的实现细节（看项目根的 `ARCHITECTURE.md`）
- 平台不是 Windows / Linux / macOS（x-cli npm 包支持这三平台）

---

## 文件索引

按需查阅，不要一次性全读：

| 文件 | 何时读 |
|---|---|
| `references/commands.md` | 不确定某个 x 子命令的 flag / 输出格式时 |
| `references/auth-patterns.md` | 需要配置鉴权时 |
| `references/workflow-patterns.md` | **写 workflow 前必读**——语法、依赖、引用语法 |
| `references/troubleshooting.md` | 验证失败 / 401 / endpoint 找不到时 |
| `references/scope.md` | 理解两层架构和 agent 角色时 |
| `distribution.md` | 首次拿到这个 skill 时先读——知道怎么打包、怎么分发 |
| `examples/` | 写 workflow 前看参考实现 |

---

## 关键约束

- **binary 通过 npm 装**，不是 meta-skill 自带
- **x 是打包工具，不是业务分析工具**——业务抽象需要你来设计
- **OpenAPI 和 CLI 是平行的一等公民**——用同样的 workflow 抽象
- **CLI 工具没有规范，需要你读文档后写 CliSpec YAML**
- **workflow 的配置参数是运行时传入的**——不同用户用同一个 skill，传不同配置
- **三种输出格式互不冲突**：可以同时 emit 多份喂给多种 agent
- **serve 是 stdio JSON-RPC**，stdout 数据 / stderr logging。关闭 stdin = serve 退出
- **业务 skill 产物默认在 `generated/`**，不进 git

---

## 给 agent 的硬性提示

1. **不要直接 `x emit` 就完事**——先分析、理解业务、写 workflow 和 CliSpec，再 emit
2. **workflow 是业务抽象的核心**——把多个步骤（OpenAPI / CLI / 混合）串成一个业务操作
3. **CLI 工具需要你写 CliSpec YAML**——没有规范，只能靠你读文档
4. **配置参数用 `$input.*` 引用**——让不同用户传入不同配置
5. **不要在 fixture 里写真实网络调用**——所有验证用本地 mock server
6. **不要改 Endpoint.id 格式**（`<Domain>__<method>__<sanitized_path>`）——已发布的 skill 全靠这个 id
7. **不要改 JSON-RPC 错误码数值**——agent 端 hardcode 了这些码
8. **不要在 workflow.yaml 里写 token / 密码**——用 `serve --auth-bearer` 启动时注入
9. **Agent 不要碰 token 生命周期**——统一由 x-cli serve 按 auth.yaml 自动管理