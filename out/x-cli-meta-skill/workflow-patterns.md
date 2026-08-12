# workflow.yaml 模式

> **推断来源**：`crates/x-cli-core/src/workflow.rs` 的解析逻辑 + `examples/petstore-workflow.yaml` + `examples/petstore-dag-workflow.yaml` + ARCHITECTURE "InputRef 三种" + "DAG 依赖" 段。**多步业务场景**的核心模式。

## 何时需要 workflow

**单 endpoint** 用 `call` method 就够了。**以下场景**才需要 workflow：

1. **步骤间有数据依赖** —— 步骤 B 需要步骤 A 的响应（如 create → read）
2. **多步串联替代 agent 手工串** —— agent 调一次 `workflow.run` 拿完整结果，省 3-5 次 RPC
3. **DAG 并行** —— 多步独立可并行（v0.1 顺序执行但拓扑序正确）
4. **后端登录态传递** —— 登录拿 token → 后续步骤用 token 调业务（v0.1 不直接支持，因为 token 注入在 serve 启动时；workflow 层面 token 复用要靠外部传入）

## 基础 workflow

```yaml
name: 买宠物并查询订单
description: |
  1. 创建一只宠物
  2. 用返回的 id 查宠物
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

emit：

```bash
x emit examples/petstore.yaml --out ./generated/skill \
    --workflow examples/petstore-workflow.yaml
```

agent 调：

```json
{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{
  "workflow":"买宠物并查询订单",
  "inputs":{"petName":"fluffy"}
}}
```

## InputRef 三种

```yaml
inputs:
  body:
    # 1. 引用工作流外部输入（workflow.run 的 inputs 字段）
    name: "$input.petName"

    # 2. 引用上一步响应（路径 = 响应 JSON 的字段路径，dotted 语法）
    petId: "$steps.create_pet.response.body.id"

    # 3. 静态值（其他 = 字面值）
    tag: "demo"
```

**注意**：

- `$steps.<name>.response.body.<path>` 里 `<path>` 是 JSON 字段路径，**点号分隔**
- 如果路径里某字段不存在 → step 失败，错误码 `-32011`
- 数组用 `$steps.x.response.body.users.0.id`（v0.1 不支持数字索引，要用业务字段名定位）

## DAG 依赖（`depends_on`）

数组顺序**默认串行**。用 `depends_on` 显式声明依赖 → 按拓扑序执行。

```yaml
name: 平行获取宠物和订单
steps:
  - name: summarize
    depends_on: [fetch_pet, fetch_order]   # 显式声明依赖
  - name: fetch_pet
  - name: fetch_order
```

拓扑序：`fetch_pet` + `fetch_order` 同层（按数组位置，先写的先跑）→ `summarize` 在它们之后。

**v0.1 同层顺序执行**（不并发）。**v0.2 计划用 `tokio::join!` 并发**。

### 校验规则

emit 阶段会拒：

- 未知引用（`$input.xxx` 没在 `inputs` 里定义）
- 未知 step 引用（`$steps.typo`）
- 自依赖（`depends_on: [self]`）
- 环（A → B → A）

错误信息会指出哪个 workflow 哪个 step 哪个字段。

## 典型 workflow 模式（业务推断）

### 模式 A：登录态后续调用

```yaml
# 注意：v0.1 workflow 拿不到 serve 启动时注入的 token 信息
# 这个模式是"假设后端 login 在 workflow 里的某个 step 完成"
name: 登录后查 dashboard
steps:
  - name: login
    endpoint: Auth__post__api_v1_login
    inputs:
      body:
        username: "$input.username"
        password: "$input.password"
  - name: list_dashboards
    endpoint: Dashboards__get__api_v1_dashboard_
    # 假设后端用 cookie / session，workflow 不需要显式传 token
```

### 模式 B：分页拉全

```yaml
name: 拉完所有 dashboard
steps:
  - name: page_0
    endpoint: Dashboards__get__api_v1_dashboard_
    inputs:
      query:
        page: 0
        page_size: 100
  - name: page_1
    depends_on: [page_0]
    endpoint: Dashboards__get__api_v1_dashboard_
    inputs:
      query:
        page: "$steps.page_0.response.body.next_page"
        page_size: 100
    # 注意：这个模式 v0.1 不内置循环，要手动展开 N 个 step
    # v0.2 计划加 `for_each` 表达式
```

### 模式 C：拉 + 转 + 写

```yaml
name: ETL：从 A 拉数据转存到 B
steps:
  - name: pull_from_a
    endpoint: A__get__api_data
  - name: transform
    depends_on: [pull_from_a]
    # 假设 transform 在后端有专用 endpoint，或：
    # 写一个外部 transform step（v0.1 不支持，要等 v0.2）
    endpoint: B__post__api_data
    inputs:
      body: "$steps.pull_from_a.response.body"
```

### 模式 D：扇出汇总

```yaml
name: 拉 3 个 dashboard 详情汇总
steps:
  - name: detail_a
    endpoint: Dashboards__get__api_v1_dashboard_pk
    inputs: { path_params: { pk: "1" } }
  - name: detail_b
    endpoint: Dashboards__get__api_v1_dashboard_pk
    inputs: { path_params: { pk: "2" } }
  - name: detail_c
    endpoint: Dashboards__get__api_v1_dashboard_pk
    inputs: { path_params: { pk: "3" } }
  - name: aggregate
    depends_on: [detail_a, detail_b, detail_c]
    endpoint: Aggregator__post__api_v1_aggregate
    inputs:
      body:
        items:
          - "$steps.detail_a.response.body"
          - "$steps.detail_b.response.body"
          - "$steps.detail_c.response.body"
```

## 失败行为

- 任一 step 4xx/5xx → **整个 workflow 立即失败**，后续 step 不跑
- 错误码 `-32011`，`data` 字段含 `step` / `endpoint` / `status` / `body`
- 4xx body 想看细节：从 `error.data.body` 拿
- **没有 retry**（v0.1）。要等 v0.2 计划加

## 排错速查

| 现象 | 原因 |
|---|---|
| `workflow 不存在` (-32010) | `workflow.run` 的 `workflow` 字段名跟 workflow.yaml 的 `name` 拼写不一致 |
| `step X 引用了不存在的 endpoint` (emit 阶段 bail) | workflow.yaml 里 `endpoint: xxx` 拼错；emitter 会列出所有可用 id |
| `缺外部输入` (-32012) | workflow 有 `inputs` 定义但 `workflow.run` 调用没传 |
| `$input.xxx` 在 emit 阶段 bail | workflow 里引了未定义的 input |
| 环依赖（emit 阶段 bail）| A 依赖 B，B 依赖 A |

完整排错见 `troubleshooting.md`。
