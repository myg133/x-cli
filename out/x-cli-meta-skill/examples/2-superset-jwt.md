# 范例 2：Superset JWT 鉴权（最常见模式）

> 端到端：拿 OpenAPI → emit → 拿 token → serve 带 auth → 调真实业务 endpoint。**真实后端 + JWT**，最常被用到的模式。

## 前置

- Superset 实例（假设 `https://superset.example.com`）
- 管理员账号
- `x` 已装（`pnpm install -g @myg133/x-cli`）

## 步骤

### 1. 拿 OpenAPI

Superset 自己导出：

```bash
curl -L -o examples/superset.json https://superset.example.com/swagger/v1/swagger.json
```

或者用 Apache Superset 主分支的快照（1.27 MB / 276 endpoint）：

```bash
curl -L -o examples/superset.json https://raw.githubusercontent.com/apache/superset/refs/heads/master/docs/static/resources/openapi.json
```

### 2. emit

```bash
x emit examples/superset.json --out ./generated/superset-skill
```

预期输出：

```
✓ 解析 276 个接口、0 个工作流，格式 markdown 写入 ./generated/superset-skill
```

**实测耗时 ~0.2 秒**（276 endpoint / 305 $ref / 1.27 MB）。

### 3. 拿 JWT（**关键步骤**）

Superset 的 `/api/v1/security/login` **不在 OpenAPI spec 里**（Superset 自己的实现），所以手工拿：

```bash
curl -X POST https://superset.example.com/api/v1/security/login \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"your-password"}'
```

返回：

```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "refresh_token": "...",
  "expires_in": 86400
}
```

把 `access_token` 存到环境变量（避免 shell history 暴露）：

```bash
read -s SUPERSET_TOKEN
# 粘贴 access_token，回车
```

### 4. serve 带 auth

```bash
x serve --skill ./generated/superset-skill \
    --base-url https://superset.example.com \
    --auth-bearer "$SUPERSET_TOKEN"
```

**为什么加 `--base-url`**：Superset 的 OpenAPI 里 `servers[0].url` 通常是 `http://localhost:8088`，跟实际生产地址不符。

预期输出：

```
✓ 注入 1 个认证 header
# 等待 stdin 输入...
```

### 5. ping

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"ping"}' | x serve --skill ./generated/superset-skill --base-url https://superset.example.com --auth-bearer "$SUPERSET_TOKEN"
```

预期：

```json
{"jsonrpc":"2.0","id":1,"result":{"pong":true}}
```

### 6. 调真实业务：拉 dashboard 列表

`endpoint_id` 怎么知道的？看 `./generated/superset-skill/SKILL.md` 的 `Dashboards` 段，或者直接 `ls ./generated/superset-skill/endpoints/ | grep -i dashboard`。

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"call","params":{"endpoint_id":"Dashboards__get__api_v1_dashboard_","path_params":{},"query":{"q":"{\"page\":0,\"page_size\":5}"},"headers":{},"body":null}}' | x serve --skill ./generated/superset-skill --base-url https://superset.example.com --auth-bearer "$SUPERSET_TOKEN"
```

预期（**真实** Superset 会返回）：

```json
{"jsonrpc":"2.0","id":1,"result":{
  "status":200,
  "headers":{...},
  "body":{
    "result":[
      {"id":1,"dashboard_title":"Sales Overview","slug":"sales"},
      ...
    ],
    "count":42
  }
}}
```

## 加 workflow

写 `examples/superset-list-dashboards.yaml`：

```yaml
name: 列前 5 个 dashboard
description: |
  演示在 Superset 里用 workflow 拉数据。
steps:
  - name: list
    endpoint: Dashboards__get__api_v1_dashboard_
    inputs:
      query:
        q: "{\"page\":0,\"page_size\":5}"
```

emit 时带 workflow：

```bash
x emit examples/superset.json --out ./generated/superset-skill \
    --workflow examples/superset-list-dashboards.yaml
```

agent 调：

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{"workflow":"列前 5 个 dashboard","inputs":{}}}' | x serve --skill ./generated/superset-skill --base-url https://superset.example.com --auth-bearer "$SUPERSET_TOKEN"
```

## Token 过期怎么办

Superset 的 `access_token` 默认 24 小时过期。**x-cli v0.1 不自动 refresh**（设计如此，refresh 需要持久化状态）：

1. 重新跑 step 3 拿新 token
2. 重新启动 `x serve`（旧的进程用的是旧 token）

未来 v0.2+ 计划加自动 refresh（ARCHITECTURE 未来扩展点）。

## 失败处理

| 现象 | 原因 + 修复 |
|---|---|
| 401 但 token 看起来对 | **检查 `--base-url`**：Superset 通常在 `/api/v1/` 前缀，**base-url 一定是根 URL**（不带 `/api/v1`）|
| 步骤 4 serve 报 "注入 0 个认证 header" | `read -s` 后 `echo $SUPERSET_TOKEN` 是空 —— 重新跑 |
| 步骤 6 返回空 `result` | query 参数错（Superset 用 `q` 字段包 JSON 字符串）|
| token 突然 401 | 过期，重跑 step 3 |

## 复用本模式到其他后端

- **GitLab**：把 `security/login` 换成 GitLab 的 `POST /oauth/token`，传 `grant_type=password`
- **自建 JWT 网关**：通常 `POST /auth/login` → `{token: "..."}` → 同样 `--auth-bearer $TOKEN`
- **多租户**：再加 `--auth-header "X-Tenant=acme"`
