# SETUP.md — x-cli 自动化安装指南

> 此文档专为 AI agent 设计。agent 收到此文档的 URL 后，应按步骤自动安装 x-cli 并加载业务 skill。

---

## 概述

x-cli 是一个 CLI 工具，能把后端 OpenAPI / CLI 文档转成 agent 可加载的 **skill**，并支持 MCP 协议运行。

**典型流程**：

```
OpenAPI spec ──► x emit ──► skill 目录 ──► x serve ──► MCP 服务 ──► agent 调用
```

**两层 skill 架构**：

| 层 | 角色 | 谁用 |
|---|---|---|
| **meta-skill**（工具生成器） | 教 agent 用 `x` 命令，把 OpenAPI 转成业务 skill | FDE 工程师 / agent 首次搭建 |
| **业务 skill**（具体后端） | 调具体后端 API 的 agent 工具集合 | 业务用户 / agent 日常使用 |

---

## 第一步：安装 x-cli

agent 通过 npm 安装 x-cli 二进制：

```bash
npm install -g @myg133/x-cli
```

验证安装：

```bash
x --version
# 输出示例: x 0.1.4
```

> 如果 `x` 命令找不到，检查 `npm root -g` 是否在 PATH 中。

---

## 第二步：加载 meta-skill（造业务 skill 用）

meta-skill 位于仓库 `out/x-cli-meta-skill/` 目录，包含以下文件：

| 文件 | 用途 |
|---|---|
| `SKILL.md` | 入口，agent 加载此文件获取完整能力 |
| `commands.md` | `x` 命令速查 |
| `auth-patterns.md` | 认证配置模式 |
| `workflow-patterns.md` | 工作流编排模式 |
| `troubleshooting.md` | 常见问题排查 |
| `distribution.md` | 分发与打包说明 |
| `examples/` | 4 个端到端范例 |

**agent 加载 meta-skill 的步骤**：

1. 读取 `SKILL.md` 了解整体能力
2. 按需查阅 `commands.md` 了解 `x` 子命令
3. 按需查阅 `auth-patterns.md` 和 `workflow-patterns.md` 了解高级用法

**meta-skill 的职责**：当用户提供 OpenAPI 文档时，agent 使用 `x` 命令将文档转为业务 skill。

---

## 第三步：生成业务 skill

当用户提供 OpenAPI 文档（YAML / JSON）时，agent 执行：

```bash
# 基础用法
x emit <openapi-spec> --out <output-dir>

# 示例：把 petstore.yaml 转成 skill
x emit examples/petstore.yaml --out ./generated/petstore-skill

# 指定输出格式（默认四种格式全出）
x emit examples/petstore.yaml --out ./generated/petstore-skill --format mcp

# 带 workflow（业务编排）
x emit examples/petstore.yaml --out ./generated/petstore-skill \
    --workflow examples/petstore-workflow.yaml
```

输出的业务 skill 目录结构：

```
generated/petstore-skill/
├── SKILL.md              # 业务 skill 入口（agent 加载此文件）
├── .x-cli/
│   └── ir.json           # IR 数据（serve 用）
├── mcp-tools.json        # MCP 工具定义
├── mcp-server.json       # MCP 连接配置
└── workflows/            # 工作流定义
    └── <name>.yaml
```

---

## 第四步：启动 MCP 服务执行业务

生成业务 skill 后，启动 MCP 服务：

```bash
# 启动 stdio JSON-RPC 服务
x serve --skill ./generated/petstore-skill

# 或指定 MCP 模式
x serve --mcp --skill ./generated/petstore-skill
```

agent 通过 MCP 协议调用 `tools/list` 和 `tools/call` 来使用后端接口。

---

## 完整端到端示例

```bash
# 1. 安装 x-cli
npm install -g @myg133/x-cli

# 2. 查看帮助
x --help

# 3. 解析 OpenAPI 查看 IR
x parse examples/petstore.yaml

# 4. 生成业务 skill
x emit examples/petstore.yaml --out ./generated/petstore-skill

# 5. 启动 MCP 服务
echo '{"jsonrpc":"2.0","id":1,"method":"ping"}' | \
    x serve --skill ./generated/petstore-skill

# 6. 通过 workflow 执行业务
echo '{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{
  "workflow":"买宠物并查询订单","inputs":{"petName":"fluffy"}
}}' | x serve --skill ./generated/petstore-skill
```

---

## 平台支持

| 平台 | 二进制 |
|---|---|
| Windows x64 | `x-win32-x64.exe` |
| Linux x64 | `x-linux-x64` |
| macOS ARM64 | `x-darwin-arm64` |

npm 包会自动检测平台并安装对应二进制。

---

## 管理版本

```bash
# 查看当前版本
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