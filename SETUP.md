# SETUP.md — x-cli 自动化安装指南

> 此文档专为 AI agent 设计。agent 收到此文档的 URL 后，应按步骤自动安装 x-cli 并加载 meta-skill。

---

## 概述

x-cli 是一个 CLI 工具，能把后端 OpenAPI / CLI 工具转成 agent 可加载的 **skill**，并支持 MCP 协议运行。

**核心思路**：`x` 是打包工具，不是业务分析工具。业务的抽象（domain 划分、workflow 编排、CLI 封装）需要 **agent 的智能参与**来完成。

---

## 两层架构（agent 在两个层的角色不同）

```
Layer 1（分析构建层）──── FDE 工程师 + agent + meta-skill
│
├── 拿到 OpenAPI / CLI 文档
├── agent 阅读文档，理解业务语义
├── agent 编写 workflow.yaml（业务编排）
├── agent 编写 CliSpec YAML（CLI 封装）
├── agent 使用 `x emit` 打包 → 产出业务 skill 目录
│
                ▼
Layer 2（运行层）────── 业务用户 + agent + 业务 skill
│
├── agent 加载业务 skill
├── `x serve` 启动 MCP 服务
├── agent 通过 MCP 调业务工具
└── 不同用户传入不同配置参数
```

| 层 | 角色 | agent 做什么 | 工具 |
|---|---|---|---|
| **Layer 1**（造 skill） | 把后端文档变成业务 skill | 分析业务、写 workflow、写 CliSpec、调 `x emit` 打包 | meta-skill + `x` CLI |
| **Layer 2**（用 skill） | 执行业务操作 | 加载 skill、调 MCP 工具完成业务目标 | 业务 skill + `x serve` |

---

## 第一步：安装 x-cli

```bash
npm install -g @myg133/x-cli
```

验证：

```bash
x --version
# 输出示例: x 0.1.4
```

---

## 第二步：加载 meta-skill（学会造业务 skill）

meta-skill 位于 `out/x-cli-meta-skill/` 目录，是 agent 进入 **Layer 1** 的入口。

**agent 加载 meta-skill 后，获得的能力**：

1. 阅读 OpenAPI 文档，识别业务域（domain）和接口
2. 阅读 CLI 工具文档，编写 CliSpec YAML
3. 编写 workflow YAML，把多个步骤串成业务操作
4. 使用 `x emit` 打包成业务 skill
5. 使用 `x serve` 启动服务、验证生成的 skill

**关键文件**：

| 文件 | 用途 |
|---|---|
| `SKILL.md` | 入口，完整的 agent 指导 |
| `commands.md` | `x` 子命令速查 |
| `auth-patterns.md` | 认证配置模式 |
| `workflow-patterns.md` | 如何写 workflow 编排业务 |
| `examples/` | 4 个端到端范例 |

---

## 第三步：Layer 1 — agent 构建业务 skill

**这一步需要 agent 的智能参与**，`x` 只负责最后的打包。

### 两种源是平行的一等公民

```
OpenAPI 规范 ──► x parse 解析 IR ──┐
                                    ├──► 你写 workflow.yaml ──► x emit ──► 业务 skill
CLI 工具文档 ──► 你写 CliSpec YAML ─┘
```

无论是 OpenAPI 还是 CLI，最终都汇入同一个 workflow 抽象。

### 典型流程

```
1. 用户提供 OpenAPI 文档 / CLI 工具描述
   │
2. agent 分析源，理解业务域
   │  ├── OpenAPI：x parse 看 IR
   │  └── CLI：读文档，写 CliSpec YAML
   │
3. agent 设计 workflow 编排
   │  ├── 把多个步骤串成业务操作
   │  ├── 例：rsql 查数据库 → mc cp 上传到 minio
   │  └── 配置参数用 $input.* 引用（不同用户传入不同配置）
   │
4. agent 调用 x emit 打包
   │  └── x emit <spec> --out <dir> --workflow <workflow.yaml> --cli-spec <cli-spec.yaml>
   │
5. agent 验证生成的 skill
   │  ├── 检查 SKILL.md 的 frontmatter 和描述
   │  └── 用 x serve 测试 MCP 工具调用
   │
6. 交付业务 skill 给业务用户
```

### 命令示例

```bash
# === 纯 OpenAPI ===
# 1. 解析 IR
x parse examples/petstore.yaml

# 2. 打包
x emit examples/petstore.yaml --out ./generated/petstore-skill \
    --workflow examples/petstore-workflow.yaml

# === 纯 CLI 工具 ===
# 1. 写 CliSpec YAML（agent 读文档后写）
# cli-spec.yaml — 描述 rsql、mc 等命令

# 2. 校验 CliSpec 格式
x parse-cli-spec cli-spec.yaml

# 3. 打包
x emit --cli-spec cli-spec.yaml --out ./generated/stats-skill \
    --workflow stats-workflow.yaml

# === OpenAPI + CLI 混合 ===
x emit examples/petstore.yaml \
    --cli-spec cli-spec.yaml \
    --out ./generated/hybrid-skill \
    --workflow hybrid-workflow.yaml
```

### 输出产物

```
generated/<name>-skill/
├── SKILL.md              # 业务 skill 入口（agent 加载此文件）
├── .x-cli/
│   └── ir.json           # IR 数据（serve 用）
├── mcp-tools.json        # MCP 工具定义
├── mcp-server.json       # MCP 连接配置
└── workflows/            # 工作流定义
    └── <name>.yaml       # agent 编写的业务编排
```

---

## 第四步：Layer 2 — agent 执行业务

业务 skill 生成后，进入 **Layer 2**（运行层）。

```bash
# 启动 MCP 服务（stdio 模式）
x serve --skill ./generated/petstore-skill
```

agent 通过标准 JSON-RPC 调业务工具：

```json
// tools/list → 返回业务工具列表（含 workflow 作为业务工具）
{"jsonrpc":"2.0","id":1,"method":"tools/list"}

// tools/call → 执行业务操作
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
  "name":"统计用户调用并保存到对象存储",
  "arguments":{
    "db_connection":"postgres://user_a:pass@a-db:5432/stats",
    "bucket_url":"s3://a-bucket"
  }
}}

// workflow.run → 执行多步工作流
{"jsonrpc":"2.0","id":3,"method":"workflow.run","params":{
  "workflow":"统计用户调用并保存到对象存储",
  "inputs":{
    "db_connection":"postgres://user_a:pass@a-db:5432/stats",
    "bucket_url":"s3://a-bucket"
  }
}}
```

**不同用户使用同一个业务 skill，传入不同的配置参数**——DB 连接串、minio endpoint 都是运行时参数，不是写死在 skill 里的。

---

## 完整端到端示例

### OpenAPI 场景

```bash
# ===== Layer 1: 构建 =====
npm install -g @myg133/x-cli
x parse examples/petstore.yaml
x emit examples/petstore.yaml --out ./generated/petstore-skill \
    --workflow examples/petstore-workflow.yaml

# ===== Layer 2: 执行 =====
echo '{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{
  "workflow":"买宠物并查询订单","inputs":{"petId":"1"}
}}' | x serve --skill ./generated/petstore-skill
```

### CLI 工具场景（rsql + minio-cli）

```bash
# ===== Layer 1: 构建 =====
# 1. agent 写 cli-spec.yaml（描述 rsql query、mc cp 等命令）
# 2. agent 写 workflow.yaml（串成"查数据库→上传对象存储"）
# 3. 打包
x emit --cli-spec cli-spec.yaml --out ./generated/stats-skill \
    --workflow stats-workflow.yaml

# ===== Layer 2: 执行 =====
# 用户 A 用他家的 DB
echo '{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{
  "workflow":"统计用户调用并保存到对象存储",
  "inputs":{
    "db_connection":"postgres://user_a:pass@a-db:5432/stats",
    "bucket_url":"s3://a-bucket"
  }
}}' | x serve --skill ./generated/stats-skill

# 用户 B 用他家的 DB
echo '{"jsonrpc":"2.0","id":2,"method":"workflow.run","params":{
  "workflow":"统计用户调用并保存到对象存储",
  "inputs":{
    "db_connection":"mysql://user_b:pass@b-db:3306/stats",
    "bucket_url":"s3://b-bucket"
  }
}}' | x serve --skill ./generated/stats-skill
```

---

## 平台支持

| 平台 | 二进制 |
|---|---|
| Windows x64 | `x-win32-x64.exe` |
| Linux x64 | `x-linux-x64` |
| macOS ARM64 | `x-darwin-arm64` |

npm 自动检测平台安装对应二进制。

---

## 管理版本

```bash
# 查看版本
x --version

# 升级
npm update -g @myg133/x-cli

# 卸载
npm uninstall -g @myg133/x-cli
```

---

## 仓库地址

- GitHub: https://github.com/myg133/x-cli
- npm: https://www.npmjs.com/package/@myg133/x-cli
- meta-skill: `out/x-cli-meta-skill/`（在仓库内）