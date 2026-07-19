# x-cli 完整示例

> 从 0 到端到端：OpenAPI → skill → agent 加载 → workflow.run。

## 1. 准备

```bash
$ git clone <repo>
$ cd x-cli
$ cargo build --release
$ ls target/release/x*
target/release/x.exe
```

## 2. 用 petstore 跑一遍

```bash
$ ls examples/
petstore.yaml                              # 5 个接口 / 2 个业务域
petstore-workflow.yaml                     # 多步工作流
petstore-dag-workflow.yaml                 # DAG 工作流
httpbin.yaml
httpbin-workflow.yaml
superset.json                              # 1.27 MB / 276 接口
```

### 2.1 emit 出 markdown skill

```bash
$ ./target/release/x.exe emit examples/petstore.yaml \
    --out ./out/petstore-skill \
    --workflow examples/petstore-workflow.yaml

✓ 解析 5 个接口、1 个工作流，写入 ./out/petstore-skill
  业务域: 2
```

产物：

```
./out/petstore-skill/
├── SKILL.md                            # 索引
├── endpoints/
│   ├── pet__get__pets.md
│   ├── pet__get__pets_petId.md
│   ├── pet__post__pets.md
│   ├── store__get__store_orders_orderId.md
│   └── store__post__store_orders.md
├── workflows/
│   ├── 买宠物并查询订单.md
│   └── 买宠物并查询订单.yaml          # 机器可读，serve 启动加载
└── .x-cli/
    └── ir.json
```

### 2.2 起 serve

```bash
$ ./target/release/x.exe serve --skill ./out/petstore-skill
# 等待 stdin 输入...
```

serve 启动时打印：

```
✓ 加载 1 个工作流
```

### 2.3 调单 endpoint（`call` method）

```bash
$ echo '{"jsonrpc":"2.0","id":1,"method":"call","params":{
    "endpoint_id":"pet__get__pets_petId",
    "path_params":{"petId":"123"}
}}' | ./target/release/x.exe serve --skill ./out/petstore-skill
```

输出（`body` 是 `petstore.example.com` 返回的 404，因为是假 URL；真实后端会返回 200 + pet 数据）：

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32002,"message":"error sending request for url (https://petstore.example.com/v1/pets/123)"}}
```

### 2.4 调 workflow（`workflow.run` method）

```bash
$ echo '{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{
    "workflow":"买宠物并查询订单",
    "inputs":{"petName":"fluffy","petId":"p-001"}
}}' | ./target/release/x.exe serve --skill ./out/petstore-skill
```

输出（同样会因假 URL 报错，但 workflow 解析和结构正确）：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32011,
    "message": "step `create_pet` HTTP failed: ...",
    "data": { "step": "create_pet", "endpoint": "pet__post__pets" }
  }
}
```

## 3. 切到真实后端

把 `examples/petstore.yaml` 里的 `servers.url` 改成真后端（比如 `https://api.yourcompany.com/v1`），
重新 emit 即可。或者用 `--base-url` override serve 时的 base URL。

```bash
$ echo '...' | ./target/release/x.exe serve --skill ./out/petstore-skill --base-url https://api.yourcompany.com
```

## 4. 验证 Superset 真实大文档

```bash
# 下载（首次）
curl -L -o examples/superset.json \
  https://raw.githubusercontent.com/apache/superset/refs/heads/master/docs/static/resources/openapi.json

# emit
$ ./target/release/x.exe emit examples/superset.json --out ./out/superset-anthropic --format anthropic
✓ 解析 276 个接口、0 个工作流，格式 anthropic 写入 ./out/superset-anthropic

# 看 frontmatter
$ head -5 ./out/superset-anthropic/SKILL.md
---
name: Superset
description: API 版本 1，276 个接口，覆盖 Advanced Data Type、Annotation Layers、AsyncEventsRestApi... 当用户问及这些业务时使用此 skill。
---
```

## 5. 测试三种输出格式

```bash
# Markdown（默认）
./target/release/x.exe emit examples/petstore.yaml --out ./out/md

# Anthropic（单 SKILL.md + frontmatter）
./target/release/x.exe emit examples/petstore.yaml --out ./out/anthropic --format anthropic

# OpenAI function calling（单 functions.json）
./target/release/x.exe emit examples/petstore.yaml --out ./out/openai --format openai
```

## 6. 在 agent 代码里用

每个 `endpoints/<id>.md` 自带 Python 调用示例（`x serve` subprocess 模式）：

```python
import json, subprocess

req = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "call",
    "params": {
        "endpoint_id": "pet__get__pets_petId",
        "path_params": {"petId": "123"},
        "query": {},
        "headers": {},
        "body": None,
    },
}
proc = subprocess.run(
    ["x", "serve", "--skill", "./out/petstore-skill"],
    input=json.dumps(req),
    capture_output=True,
    text=True,
)
resp = json.loads(proc.stdout.strip())
```

更高效：agent 直接用 `workflow.run` 跑多步：

```python
req = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "workflow.run",
    "params": {
        "workflow": "买宠物并查询订单",
        "inputs": {"petName": "fluffy", "petId": "p-001"},
    },
}
```
