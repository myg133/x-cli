---
name: x-cli-skill-factory
description: >-
  **x-cli 造 skill 的 meta-skill，用于 Layer 1（分析构建层）。** 当用户提供 OpenAPI / CLI 文档、
  希望"让 agent 能调这个后端"时加载此 skill。此 skill 教 agent 如何分析后端文档、理解业务域、
  设计 workflow 编排、编写 CliSpec，最后用 x emit 打包成业务 skill。**x 不是业务分析工具，是打包工具。
  业务抽象（domain 划分、workflow 编排、CLI 封装）需要 agent 的智能参与。** 如果用户已经有现成的
  业务 skill 在 generated/，直接加载业务 skill（Layer 2），而非本 skill。
---

# x-cli skill factory

> 这个 skill 不是调业务后端的，是**教 agent 造业务 skill + 设计业务抽象**的。
> agent 接到"把 OpenAPI 转成 skill"的请求时，加载这个。

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

## 第二步：分析 OpenAPI，理解业务

**不要直接 `x emit`**。先分析文档，理解业务结构。

### 2.1 解析 OpenAPI 看 IR

```bash
x parse <openapi.yaml>
```

IR 会告诉你：
- 有哪些 **domain**（业务域）
- 每个 domain 下有哪些 **endpoint**
- 每个 endpoint 的 method / path / 参数 / 响应

### 2.2 识别业务域

根据 IR 的 domain 划分，理解每个 domain 的业务含义：

| OpenAPI tag | 业务域 | 典型操作 |
|---|---|---|
| `pet` | 宠物管理 | 增删改查宠物 |
| `store` | 商店订单 | 下单、查库存、查订单 |
| `user` | 用户管理 | 注册、登录、权限 |

如果 OpenAPI 的 tag 划分不理想（比如所有接口都打了一个 tag），你需要**自己重新组织**业务域。

### 2.3 设计业务编排（workflow）

分析完业务域后，识别**哪些操作需要多步串联**：

```
例："买宠物" = 查库存 → 创建宠物 → 下单
例："用户注册" = 创建用户 → 发验证邮件 → 返回 token
```

然后写 `workflow.yaml`：

```yaml
name: 买宠物并查询订单
description: 先查宠物库存，有货就买，然后查订单状态
steps:
  - id: find_pet
    endpoint: pet__get__/pet/{petId}
    input:
      petId: "$input.petId"
  - id: place_order
    endpoint: store__post__/store/order
    depends_on: [find_pet]
    input:
      petId: "$steps.find_pet.id"
      quantity: 1
  - id: get_order
    endpoint: store__get__/store/order/{orderId}
    depends_on: [place_order]
    input:
      orderId: "$steps.place_order.id"
output: "$steps.get_order"
```

**workflow 是业务抽象的核心**——它把多个原始 API 调用封装成一个业务操作，agent 在 Layer 2 调一次 `workflow.run` 就完成。

---

## 第三步：打包成业务 skill

分析完业务、写好 workflow 后，用 `x emit` 打包：

```bash
# 基础用法
x emit <openapi.yaml> --out ./generated/<name>-skill

# 带 workflow（业务编排）
x emit <openapi.yaml> --out ./generated/<name>-skill \
    --workflow <workflow.yaml>

# 带 CLI 工具（如 kubectl）
x emit <openapi.yaml> --out ./generated/<name>-skill \
    --workflow <workflow.yaml> \
    --cli-tools <cli-tools.yaml>

# 指定格式（默认四种格式全出）
x emit <openapi.yaml> --out ./generated/<name>-skill \
    --workflow <workflow.yaml> \
    --format mcp
```

`x emit` 做的事：
- 解析 OpenAPI 生成 IR
- 把 workflow 嵌入 IR
- 渲染 SKILL.md（业务 skill 入口）
- 生成 mcp-tools.json（MCP 工具定义）
- 输出 `.x-cli/ir.json`（serve 加载用）

**它不做的事**（需要你来做）：
- ❌ 不会分析业务域——你来做
- ❌ 不会写 workflow——你来做
- ❌ 不会设计业务抽象——你来做
- ❌ 不会理解业务语义——你来做

---

## 第四步：验证生成的 skill

打包后，验证业务 skill 是否可用：

```bash
# 查看 SKILL.md（业务 skill 入口）
cat ./generated/<name>-skill/SKILL.md

# 启动 MCP 服务测试
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | \
    x serve --skill ./generated/<name>-skill

# 测试 workflow
echo '{"jsonrpc":"2.0","id":2,"method":"workflow.run","params":{
  "workflow":"买宠物并查询订单","inputs":{"petId":"1"}
}}' | x serve --skill ./generated/<name>-skill
```

---

## 第五步：交付

把生成的业务 skill 目录路径告诉用户。用户（或业务 agent）在 Layer 2 加载这个 skill 即可使用。

---

## 何时加载

匹配以下任一即加载：

- 用户提供 OpenAPI 文件 / URL，说"做 skill" / "让 agent 能调这个后端"
- 用户说"用 x-cli 处理这个 OpenAPI"
- 用户问"怎么把后端 OpenAPI 变成 agent skill"
- 已有 skill 加载失败，用户说"重新生成" / "OpenAPI 变了，刷新一下"

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
| `commands.md` | 不确定某个 x 子命令的 flag / 输出格式时 |
| `auth-patterns.md` | 需要配置鉴权时 |
| `workflow-patterns.md` | **写 workflow 前必读**——语法、依赖、$input / $steps 引用 |
| `troubleshooting.md` | 验证失败 / 401 / endpoint 找不到时 |
| `distribution.md` | 首次拿到这个 skill 时先读——知道怎么打包、怎么分发 |
| `examples/1-petstore-no-auth.md` | 无 auth 的最简参考实现 |
| `examples/3-httpbin-workflow.md` | workflow.yaml 多步范例（**写 workflow 前必看**）|
| `examples/4-large-spec.md` | 1MB+ / 200+ endpoint 的大文档注意事项 |

---

## 关键约束

- **binary 通过 npm 装**，不是 meta-skill 自带
- **x 是打包工具，不是业务分析工具**——业务抽象需要你来设计
- **三种输出格式互不冲突**：可以同时 emit 多份喂给多种 agent
- **serve 是 stdio JSON-RPC**，stdout 数据 / stderr logging。关闭 stdin = serve 退出
- **业务 skill 产物默认在 `generated/`**，不进 git

---

## 给 agent 的硬性提示

1. **不要直接 `x emit` 就完事**——先分析 OpenAPI、理解业务、写 workflow，再 emit
2. **workflow 是业务抽象的核心**——把多个原始 API 调封装成一个业务操作
3. **不要在 fixture 里写真实网络调用**——所有验证用本地 mock server
4. **不要改 Endpoint.id 格式**（`<Domain>__<method>__<sanitized_path>`）——已发布的 skill 全靠这个 id
5. **不要改 JSON-RPC 错误码数值**——agent 端 hardcode 了这些码
6. **不要在 workflow.yaml 里写 token**——用 `serve --auth-bearer` 启动时注入
7. **Agent 不要碰 token 生命周期**——统一由 x-cli serve 按 auth.yaml 自动管理