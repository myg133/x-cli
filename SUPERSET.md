# 用 x-cli 接 Superset

把 Superset 的 OpenAPI 文档转成 agent 可加载的 skill，配 JWT 认证，调用真实 endpoint。

> 同样的流程适用于任何用 JWT / Bearer Token 鉴权的 OpenAPI 后端（GitHub、GitLab、Stripe 自建网关等）。

## 准备

- Superset 实例地址（假设 `https://superset.example.com`）
- 管理员账号（username / password）
- 装了 x-cli（`cargo build --release`）

## 1. 拿 Superset 的 OpenAPI

Superset 自己导出：

```bash
# 假设 Superset 跑在 8088
curl -L -o examples/superset.json \
  https://superset.example.com/swagger/v1/swagger.json
```

或者用项目里这份 Apache Superset 主分支的快照（1.27 MB / 276 endpoint）：

```bash
curl -L -o examples/superset.json \
  https://raw.githubusercontent.com/apache/superset/refs/heads/master/docs/static/resources/openapi.json
```

## 2. emit 出 skill

```bash
$ x emit examples/superset.json --out ./out/superset-skill
✓ 解析 276 个接口、0 个工作流，写入 ./out/superset-skill
  业务域: 38
```

0.19 秒，276 个接口全部进 IR，`$ref` 全部解析（305 个引用）。

## 3. 拿 JWT token

Superset 的 `/api/v1/security/login` 不在 OpenAPI spec 里（Superset 自己的实现），所以需要手工拿 token：

```bash
$ curl -X POST https://superset.example.com/api/v1/security/login \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"your-password"}'

{
  "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
  "refresh_token": "...",
  "expires_in": 86400
}
```

把 `access_token` 存到环境变量（避免 shell history 暴露）：

```bash
$ read -s SUPERSET_TOKEN
# 粘贴 access_token，回车
$ echo "$SUPERSET_TOKEN" | head -c 30
eyJhbGciOiJIUzI1NiIsInR5cCI6
```

## 4. 启动 x serve

```bash
$ x serve --skill ./out/superset-skill --base-url https://superset.example.com \
    --auth-bearer "$SUPERSET_TOKEN"

✓ 加载 0 个工作流
✓ 注入 1 个认证 header
# 等待 stdin 输入...
```

## 5. 调真实 endpoint

先从 Superset 拿一份 dashboard 列表试试：

```bash
$ echo '{"jsonrpc":"2.0","id":1,"method":"call","params":{
    "endpoint_id":"Dashboards__get__api_v1_dashboard_",
    "path_params":{},
    "query":{"q":"{\"page\":0,\"page_size\":5}"},
    "headers":{},
    "body":null
}}' | x serve --skill ./out/superset-skill --base-url https://superset.example.com \
    --auth-bearer "$SUPERSET_TOKEN"

{"jsonrpc":"2.0","id":1,"result":{
  "status":200,
  "headers":{...},
  "body":{
    "result":[
      {"id":1,"dashboard_title":"Sales Overview","slug":"sales"},
      {"id":2,"dashboard_title":"User Growth","slug":"user-growth"}
    ],
    "count":42
  }
}}
```

`endpoint_id` 怎么知道的？看 `out/superset-skill/SKILL.md` 或 `endpoints/Dashboards__get__api_v1_dashboard_.md`。

## 6. 写一个 workflow

`examples/superset-list-dashboards.yaml`：

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

emit 时带上：

```bash
$ x emit examples/superset.json --out ./out/superset-skill \
    --workflow examples/superset-list-dashboards.yaml
```

agent 调：

```json
{
  "jsonrpc":"2.0","id":1,"method":"workflow.run",
  "params":{
    "workflow":"列前 5 个 dashboard",
    "inputs":{}
  }
}
```

## 7. token 过期怎么办

Superset 的 access_token 默认 24 小时过期。x-cli 不会自动 refresh（v0.1 阶段），你需要：

1. 重新跑 step 3 拿新 token
2. 重新启动 `x serve`

未来如果想自动 refresh，看 ARCHITECTURE.md 的"未来扩展点"。

## 8. 自定义 header 场景

如果 Superset 配了 CSRF token 或者其他 header：

```bash
x serve --skill ./out/superset-skill \
    --auth-bearer "$SUPERSET_TOKEN" \
    --auth-header "X-CSRF-Token=$CSRF_TOKEN" \
    --auth-header "X-Tenant=acme"
```

格式：`KEY=VALUE`，可多次。

## 常见问题

**Q: 401 Unauthorized 但 token 是对的**
A: 检查 base URL 是否对。Superset 通常在 `/api/v1/` 前缀，openapi.json 里的 path 已经包含。`--base-url` 一定要是根 URL（如 `https://superset.example.com`）。

**Q: `endpoint_id` 找不到**
A: 拼写错。看 `out/superset-skill/SKILL.md` 业务域段，里面有完整列表。`endpoint_id` 格式是 `<Domain>__<method>__<sanitized_path>`，例如 `Dashboards__get__api_v1_dashboard_`。

**Q: 4xx / 5xx 错误想看 body**
A: workflow.run 失败时，error.data 含 `step` / `status` / `body` 字段。call 方法失败时 error.message 含 URL。

**Q: 用 OpenAI format 怎么做**
A: `--format openai`，输出 `functions.json`，agent 直接喂给 OpenAI API。
