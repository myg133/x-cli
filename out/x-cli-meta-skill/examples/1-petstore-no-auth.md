# 范例 1：petstore 无鉴权（最简链路）

> 端到端：拿到 OpenAPI → emit → serve → ping → 调一个 endpoint。**5 步，~10 秒**。

## 前置

- `x` 已装（`pnpm install -g @myg133/x-cli`）
- OpenAPI：`examples/petstore.yaml`（5 个接口 / 2 业务域 / 无鉴权）

```bash
x --version
# x 0.1.0
```

## 步骤

### 1. 看 OpenAPI 长什么样

```bash
cat examples/petstore.yaml | head -20
```

确认：

- `servers[0].url` = `https://petstore.example.com/v1`（emit 时这会是 base URL）
- `security` 字段为空 → 无鉴权

### 2. emit

```bash
x emit examples/petstore.yaml --out ./generated/petstore-skill
```

预期输出：

```
✓ 解析 5 个接口、0 个工作流，格式 markdown 写入 ./generated/petstore-skill
```

### 3. 看产物

```bash
ls ./generated/petstore-skill/
# SKILL.md  endpoints/  .x-cli/

ls ./generated/petstore-skill/endpoints/
# pet__get__pets.md
# pet__get__pets_petId.md
# pet__post__pets.md
# store__get__store_inventory.md
# store__get__store_orders_orderId.md
# store__post__store_orders.md

cat ./generated/petstore-skill/SKILL.md | head -20
```

`SKILL.md` 是总索引：业务域 + endpoint 链接 + 调用约定。

### 4. 起 serve

```bash
x serve --skill ./generated/petstore-skill
```

预期输出：

```
# 等待 stdin 输入...
```

**注意**：没 workflow 时**不**打印 `✓ 加载 N 个工作流`，没 auth 时**不**打印 `✓ 注入 N 个认证 header`。

### 5. ping + 调一个 endpoint

```bash
# 5a. ping（探活）
echo '{"jsonrpc":"2.0","id":1,"method":"ping"}' | x serve --skill ./generated/petstore-skill

# 5b. 调 GET /pets/{petId}
echo '{"jsonrpc":"2.0","id":1,"method":"call","params":{"endpoint_id":"pet__get__pets_petId","path_params":{"petId":"123"}}}' | x serve --skill ./generated/petstore-skill
```

预期（petstore.example.com 是假 URL，会报 -32002，但**结构**对）：

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32002,"message":"error sending request for url (https://petstore.example.com/v1/pets/123)"}}
```

## 完成标志

✅ 步骤 5a 返回 `{"result":{"pong":true}}`
✅ 步骤 5b 返回结构正确的 JSON-RPC 错误（**业务**错不是**协议**错）

## 失败处理

| 现象 | 原因 |
|---|---|
| `x: command not found` | 没装 binary：`pnpm install -g @myg133/x-cli` 后重开 shell |
| 步骤 4 serve 立刻退出 | skill 目录不对，缺 `.x-cli/ir.json` |
| 步骤 5a 没响应 | stdin 写完没换行（serve 按行解析）|
| 步骤 5b 报 `-32601` | method 拼错（不是 `call`）|
| 步骤 5b 报 `-32001` | endpoint_id 拼错，看 `SKILL.md` 找正确 id |

## 业务侧能做什么

- 把 `petstore.example.com` 换成真实后端（`--base-url https://api.yourcompany.com`）
- 加 workflow：`--workflow examples/petstore-workflow.yaml`（买宠物并查询订单）
- 切格式：`--format anthropic` 给 Claude / `--format openai` 给 OpenAI
