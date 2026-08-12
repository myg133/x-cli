# Troubleshooting

> **推断来源**：ARCHITECTURE 错误码表 + SUPERSET.md "常见问题" + 实测容易踩的坑。**按错误码 → 现象 → 原因 → 修复** 排版，方便 agent 快速定位。

## 平台前置

**`x` 命令不存在 / command not found**：

- 没装：跑 `pnpm install -g @myg133/x-cli`（或 `npm install -g @myg133/x-cli`）
- 装了但找不到：重开 shell（PATH 修改对新 shell 才生效）
- 平台不支持：当前只 Windows x64。macOS/Linux 用户：
  - 本机有 Rust：`cargo install x-cli`（从 crates.io） 或 `cargo build --release` 后 `cp target/release/x /usr/local/bin/`
  - 不想编译：等 cross-compile CI（看 `packages/x-cli-npm/README.md`）

## 按错误码

### `-32700` JSON 解析错误

**现象**：serve 报 `parse error: ...`，stdout 立刻 EOF。

**原因**：request 不是合法 JSON（漏引号、缺逗号、UTF-8 BOM）。

**修复**：

- agent 端 `json.dumps(...)` 之前 `ensure_ascii=False`（中文不要 escape）
- Windows 平台注意换行（`\r\n` vs `\n`）—— serve 用 `lines()` 分割，`\r` 会卡在 token 里
- 一行一个 JSON，**不要**用 pretty-print 多行

### `-32600` 无效请求

**现象**：缺 `jsonrpc: "2.0"` 字段或 `id` 字段。

**修复**：固定模板：

```python
req = {"jsonrpc": "2.0", "id": <唯一 id>, "method": "...", "params": {...}}
```

### `-32601` Method 不存在

**现象**：method 拼错（如 `call` → `calls`）。

**修复**：3 个 method 名字面量是 `ping` / `call` / `workflow.run`。**`workflow.run` 带点**，不是 `workflow_run`。

### `-32602` 参数不合法

**现象**：`call` 缺 `endpoint_id`，`workflow.run` 缺 `workflow` 字段。

**修复**：每个 method 的 params 形状是固定的（见 `commands.md`）。

### `-32001` 端点不存在

**现象**：

```json
{"error":{"code":-32001,"message":"endpoint not found: pet__get__petz"}}
```

**原因**：`endpoint_id` 拼写错。

**修复**：

1. 在生成的 `<skill>/SKILL.md` 找完整列表（业务域段）
2. id 格式是 `<Domain>__<method>__<sanitized_path>`，比如 `pet__get__pets_petId`
3. **id 跟 markdown 文件名是 1:1 对应**（除了 `__` → 空格），文件名错 = id 错

### `-32002` HTTP 错误（连接 / 超时 / DNS）

**现象**：

```json
{"error":{"code":-32002,"message":"error sending request for url (https://...)"}}
```

**原因**：

- 后端服务没起来
- `--base-url` 写错（**最高频**）
- DNS 解析失败
- 网络不通（agent 在沙箱里）

**修复**：

- `curl <base-url>/some-path` 直接探活
- 检查 `<skill>/SKILL.md` 里的 `Base URL`，跟实际后端对比
- Superset 通常在 `/api/v1/` 前缀，OpenAPI 里的 path 已经包含 — **base-url 一定是根 URL**（如 `https://superset.example.com`，**不要**带 `/api/v1`）

### `-32010` workflow 不存在

**现象**：

```json
{"error":{"code":-32010,"message":"workflow not found: 买宠物并查询订单"}}
```

**原因**：`workflow.run` 的 `workflow` 字段拼写错 / 跟 workflow.yaml 的 `name` 不一致。

**修复**：

- 看 `<skill>/workflows/<name>.yaml` 的 `name` 字段
- 中文名敏感，**包含空格**（不是下划线）

### `-32011` workflow step 失败

**现象**：

```json
{"error":{"code":-32011,"message":"step `create_pet` HTTP failed: ..."}}
```

**`data` 字段含完整信息**：

```json
{"data":{"step":"create_pet","endpoint":"pet__post__pets","status":500,"body":{...}}}
```

**原因**：某 step 收到 4xx/5xx。**整个 workflow 立即停止**，后续 step 不跑。

**修复**：

- 看 `data.status` + `data.body` 知道后端返回什么
- 常见：422（参数校验失败）/ 401（鉴权失效）/ 500（后端 bug）
- 单独跑那个 endpoint（用 `call` method）能更精准定位是 workflow 输入错还是后端错

### `-32012` workflow 缺外部输入

**现象**：

```json
{"error":{"code":-32012,"message":"workflow 买宠物 requires inputs: [petName]"}}
```

**原因**：`workflow.run` 的 `inputs` 字段缺 workflow.yaml 里定义的 input。

**修复**：

```yaml
# workflow.yaml 定义
inputs:
  - name: petName
    type: string
```

```json
// workflow.run 必须传
{"params":{"workflow":"买宠物","inputs":{"petName":"fluffy"}}}
```

**注意**：有 `default` 的 input 不传也会用默认值，不会报 -32012。

## 按现象（不是错误码）

### 现象：401 Unauthorized 但 token 是对的

**优先检查 `--base-url`**（SUPERSET.md 明确说这个）：

```bash
# 错：带了路径前缀
x serve --skill ./generated/skill --base-url https://api.example.com/v1 --auth-bearer "$TOKEN"

# 对：根 URL，path 里的 /v1 让 IR 自己拼
x serve --skill ./generated/skill --base-url https://api.example.com --auth-bearer "$TOKEN"
```

如果 base-url 对了还是 401：

- token 过期（重新拿）
- 后端期待 `Authorization: JWT <token>` 而不是 `Bearer`（x-cli 写死 Bearer，**不支持切换**）
- 后端用 query param `?token=xxx`（v0.1 不支持，要走 `--auth-header` 间接：但其实不行，因为 query param 不算 header）

### 现象：endpoint 列表对不上

**原因**：OpenAPI 用了 `oneOf` / `anyOf` 复杂结构，x-cli 0.16 的 `oas3` 库可能丢字段。

**修复**：

- 跑 `x parse <file>` 看 IR 里实际有什么
- 看是不是 OAS 3.0 但没自动转换（v0.1 转换只覆盖 `parameters[].content` → `parameters[].schema`）
- 极端情况：手改 OpenAPI 把复杂结构展平

### 现象：`$ref` 没解析

**原因**：cycle detection 标记了 `recursive: true`，agent 看到的是 "循环引用，名字在这"。

**修复**：

- 这是**预期行为**（不爆栈）
- 业务侧应该把循环结构展平（推荐改成 named type + 引用）

### 现象：emit 0.19 秒但 serve 启动慢

**原因**：serve 启动时加载 `ir.json` + 解析所有 workflow.yaml。**1.27MB / 276 endpoint** 通常 100ms 内。

**修复**：

- 用 `markdown` 格式时 `endpoints/<id>.md` 数量 = endpoint 数量，**文件系统 I/O** 可能是瓶颈
- 大文档建议 `anthropic` 格式（单文件），启 serve 更快

### 现象：测试好慢

**违反 x-cli 约定**：测试必须 0 网络、< 0.5s 跑完。

**原因**：

- 测试写真实 HTTP 调用
- 测试 sleep
- fixture 文件太大

**修复**：

- 本地 mock server（`tokio::net::TcpListener`）
- 已有 helper：`x-cli-runtime/tests/workflow_executor.rs` 的 `spawn_local_server()`

### 现象：CI 挂 `cargo test` / `cargo build`

**原因**：

- 有 warning（CI 有 `RUSTFLAGS: -D warnings`）
- 测试需要网络
- fmt 没跑

**修复**：

- `cargo fmt --all` 提交前必跑
- `cargo build --workspace --all-targets 2>&1 | grep warning` 看哪里漏
- 跑 `cargo test --workspace --all-targets` 本地验证

### 现象：agent 用了 curl 而不是 x

**原因**：meta-skill 描述不够强 / 触发条件不匹配。

**修复**：

- 确认 agent **装了** meta-skill（`~/.claude/skills/` 或 `\.claude\skills\` 下能看到 x-cli-meta-skill 目录）
- 确认 meta-skill 的 SKILL.md **description** 包含"调 OpenAPI 后端 HTTP 接口"信号
- 确认 agent 接到的是"调 X 后端的 Y 接口"而不是"curl 一下 Z 看看" —— 后者不是 OpenAPI 场景
- 重启 agent session（有些平台 skill 是启动时加载的）

## 401 决策树（auth.yaml 模式）

**前提**：业务 skill 用了 auth.yaml (`token.kind: login`)。

```
业务请求返回 401
  ↓
[1] x-cli 自动调 Session::handle_401() 重试 1 次
  ↓
重试后还是 401?
  ├─ YES → 检查 re-login 用的 creds 还是不是有效
  │         ↓
  │         启动 serve 时报 "login 端点返回 401"?
  │         ├─ YES → auth.yaml 的 body 里 username/password 错
  │         │        或 login URL 错(404/405)
  │         └─ NO  → creds 对,但 login 后立刻过期了(极少见)
  │                  ↓
  │                  检查 expires_in 字段;可能是 refresh_token 模式问题
  │
  └─ NO  → x-cli 内部已经处理,agent 完全感知不到
```

**关键不变量**(scope.md 也讲过):

- agent **不**看到 401 后自己重试(让 x-cli 做)
- agent **不**读 auth.yaml
- agent **不**手写 curl 拿 token

**老习惯会踩的坑**:

- ❌ agent 在 workflow.yaml 里写 token → 进 git,泄漏风险
- ❌ agent 解析 401 后调 `call auth.refresh` → v0.1 不支持这个 method,会 -32601
- ❌ agent 重启 serve → 进程级 session 隔离,重启就掉 token

**怎么确认 401 retry 真的发生过**:

启动 serve 时打开 `RUST_LOG=info,x_cli_runtime=debug`:
- 看不到 "re-login" 日志 → 没 retry,直接 401 透传
- 看到 "re-login" → x-cli 在重试,但 backend 还认这次失败

如果 retry 后还是 401,且 backend 没改 creds 大概率是 **base-url 错**(很多后端 401/404 实际是路径错):
- Superset: base-url 必须是根 URL(如 `https://superset.example.com`,**不带 /api/v1**)

## 排不到怎么办

1. 跑 `x parse <file>` 看 IR 长什么样
2. 跑 `x emit <file> --out generated/test --format anthropic` 看是否能 emit 出来
3. 用 `curl` 手动调一个 endpoint 确认后端本身 OK
4. 看 `<skill>/endpoints/<id>.md` 的 Python 调用示例，对比自己的请求
5. 如果是 x-cli 的 bug，**最小复现 + OpenAPI 样本**报 issue（项目 GitHub: `myg133/x-cli`）
