# 范例 3：workflow.yaml 多步（httpbin）

> 演示多步 workflow：写 `workflow.yaml` → emit 时加 `--workflow` → agent 一次 `workflow.run` 拿多步结果。

## 前置

- `x` 已装（`pnpm install -g @myg133/x-cli`）
- OpenAPI：`examples/httpbin.yaml`
- workflow 例子：`examples/httpbin-workflow.yaml`

## 步骤

### 1. 看 OpenAPI

```bash
cat examples/httpbin.yaml | head -30
```

`httpbin` 是测试友好的 HTTP 服务（httpbin.org / httpbin.org 的本地克隆）：

- 提供各种 HTTP 方法 / 状态码 / 头 / body 的 echo endpoint
- 无鉴权
- 适合做 workflow demo

### 2. 看 workflow 例子

```bash
cat examples/httpbin-workflow.yaml
```

期望（实际项目里 `examples/httpbin-workflow.yaml` 长这样）：

```yaml
name: httpbin 多步 demo
description: |
  1. GET 一个 URL（path param 化）
  2. 用响应头做下一步的输入
inputs:
  - name: url
    type: string
    default: "/get"
steps:
  - name: first_request
    endpoint: Httpbin__get__<url>
    inputs:
      path_params:
        url: "$input.url"
  - name: second_request
    depends_on: [first_request]
    endpoint: Httpbin__get__anything
    inputs:
      query:
        from_first: "$steps.first_request.response.body.url"
```

### 3. emit

```bash
x emit examples/httpbin.yaml --out ./generated/httpbin-skill \
    --workflow examples/httpbin-workflow.yaml
```

预期输出：

```
✓ 解析 N 个接口、1 个工作流，格式 markdown 写入 ./generated/httpbin-skill
```

### 4. 看产物多了什么

```bash
ls ./generated/httpbin-skill/
# SKILL.md  endpoints/  workflows/  .x-cli/

ls ./generated/httpbin-skill/workflows/
# httpbin 多步 demo.md
# httpbin 多步 demo.yaml
```

**注意**：

- `workflows/<name>.yaml` 是 **机器可读**的（serve 启动时按文件加载）
- `workflows/<name>.md` 是 **人/agent 可读**的（带 description、参数说明、调用示例）

### 5. serve

```bash
x serve --skill ./generated/httpbin-skill
```

预期输出：

```
✓ 加载 1 个工作流
# 等待 stdin 输入...
```

### 6. 跑 workflow

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{"workflow":"httpbin 多步 demo","inputs":{"url":"/get"}}}' | x serve --skill ./generated/httpbin-skill
```

预期（**`outputs` = 最后一步 body**，`steps` 数组列每步详情）：

```json
{"jsonrpc":"2.0","id":1,"result":{
  "status":"ok",
  "steps":[
    {
      "name":"first_request",
      "endpoint":"Httpbin__get__<url>",
      "status":200,
      "body":{...}
    },
    {
      "name":"second_request",
      "endpoint":"Httpbin__get__anything",
      "status":200,
      "body":{"from_first":"...","args":{...}}
    }
  ],
  "outputs":{"from_first":"...","args":{...}}
}}
```

## 关键概念

- `outputs` = 最后一步的 `body`（**不是**整个 steps 数组）
- `steps` 数组里每步含 `name` / `endpoint` / `status` / `body` —— agent 可用于 audit / debug
- `inputs` 传错或缺 → 错误码 `-32012`，错误信息列出缺哪些 input
- 任一 step 4xx/5xx → 整个 workflow 立刻失败，错误码 `-32011`，`data` 字段含失败 step 的 status + body

## 业务推断的 workflow 模式

| 模式 | 描述 | 适用 |
|---|---|---|
| 登录态后续 | 步骤 1 登录拿 cookie/session，步骤 2-N 业务调用 | 需要登录的后端 |
| 分页拉全 | 步骤 N 翻页直到 next_page 缺 | 列表 API |
| 拉-转-写 | A 拉 → B 写 | ETL 场景 |
| 扇出汇总 | 多个独立查询 → 1 个聚合 | 仪表盘生成 |

详细见 `workflow-patterns.md`。

## 失败处理

| 现象 | 原因 |
|---|---|
| `workflow 不存在` (-32010) | `workflow` 字段名跟 workflow.yaml `name` 不一致（含空格、中文）|
| `缺外部输入` (-32012) | 漏传 input 或 input 名字拼错 |
| `step X 引用了不存在的 endpoint`（emit 阶段 bail）| workflow.yaml 的 `endpoint:` 拼错 |
| 步骤 N 报 HTTP 500 | 后端问题；单独用 `call` 调那个 endpoint 排查 |
