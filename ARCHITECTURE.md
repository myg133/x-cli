# x-cli 架构

## Crate 分层

```
┌────────────────────────────────────────────────────────────┐
│ x-cli (主二进制 x)                                          │
│   ├─ CLI 命令: parse / emit / serve                       │
│   └─ 串起: read OpenAPI → parse → emit → save ir.json      │
└────────────────────────────────────────────────────────────┘
            │              │              │
            ▼              ▼              ▼
┌──────────────────┐ ┌─────────────┐ ┌─────────────────────┐
│ x-cli-emitter-md │ │ x-cli-       │ │ x-cli-core           │
│                  │ │   runtime   │ │                       │
│  SkillEmitter    │ │             │ │  IR 数据模型         │
│  trait impl:     │ │  HTTP       │ │  OpenAPI 解析        │
│  - Markdown      │ │  Workflow   │ │  Workflow 解析       │
│  - Anthropic     │ │  Executor   │ │  JSON-RPC schema     │
│  - OpenAI tools  │ │  JSON-RPC   │ │  (serde types)       │
│                  │ │  transport  │ │                       │
└──────────────────┘ └─────────────┘ └─────────────────────┘
                              │              ▲
                              └──────────────┘
                              (runtime 用 core 的 IR + protocol)
```

依赖方向：**只向下指**。`x-cli` 是叶节点；`x-cli-core` 不依赖任何内部 crate。

## x-cli-core

x-cli 的"语义层"。所有类型在这里定义，emitter 和 runtime 都基于它。

### 主要模块

- `ir` — IR 数据模型（`ApiSpec` / `Domain` / `Endpoint` / `SchemaRef` / `ResolvedSchema` / `Workflow` / `WorkflowStep` / `InputRef`）
- `openapi` — OpenAPI 3 解析（`oas3` 库 + 自动 3.0 → 3.1 兼容转换）
- `workflow` — workflow.yaml 解析 + 校验（依赖、环检测）
- `protocol` — JSON-RPC schema（`RpcRequest` / `RpcResponse` / `RpcMethod` / `WorkflowRunParams` / `WorkflowRunResult` / error codes）
- `error` — 错误类型

### IR 形状

```rust
pub struct ApiSpec {
    pub title: String,
    pub version: String,
    pub description: Option<String>,
    pub base_url: Option<String>,
    pub domains: Vec<Domain>,           // 业务域（按 tag 归类）
    pub endpoints: BTreeMap<String, Endpoint>,  // endpoint id → endpoint
}

pub struct Endpoint {
    pub id: String,                      // "domain.method.path" 稳定 id
    pub domain: String,
    pub method: HttpMethod,
    pub path: String,
    pub params: Vec<Param>,
    pub request_body: Option<RequestBody>,
    pub responses: Vec<Response>,
    pub deprecated: bool,
    ...
}

pub struct SchemaRef {
    pub name: String,                    // schema 类型名（来自 $ref 或 title）
    pub description: Option<String>,
    pub json_schema: serde_json::Value,   // 原始 JSON Schema（runtime 校验备用）
    pub resolved: Option<Box<ResolvedSchema>>,  // B 阶段：解析后结构化树
}

pub struct ResolvedSchema {
    pub kind: SchemaKind,                // Object / Array / Scalar / Any
    pub properties: BTreeMap<String, SchemaRef>,
    pub required: Vec<String>,
    pub items: Option<Box<SchemaRef>>,
    pub recursive: bool,                // 循环引用标记
}

pub struct Workflow {
    pub name: String,
    pub inputs: Vec<WorkflowInput>,      // 外部输入
    pub steps: Vec<WorkflowStep>,
}

pub struct WorkflowStep {
    pub name: String,
    pub endpoint: String,                // 引用 ApiSpec.endpoints 的 key
    pub depends_on: Vec<String>,        // F 阶段：DAG 依赖
    pub inputs: StepInputs,              // path_params / query / headers / body
}

pub enum InputRef {
    Input(String),                      // $input.xxx
    StepOutput { step, path },          // $steps.<name>.<dotted.path>
    Static(String),                     // 其他 = 字面值
}
```

## x-cli-runtime

JSON-RPC over stdio + HTTP 客户端 + workflow 运行时。

### 关键函数

```rust
// stdio JSON-RPC 服务（agent 调这个）
pub async fn serve_stdio(
    spec: Arc<ApiSpec>,
    workflows: BTreeMap<String, Arc<Workflow>>,
    base_url: Option<String>,
    caller: HttpCaller,
);

// 通用 reader/writer 版本（测试 + 未来 sidecar 模式）
pub async fn serve<R, W>(reader: R, writer: W, ...)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin;
```

### WorkflowExecutor（D 阶段）

按 `execution_order(workflow)` 决定的顺序执行 step：
- 无 `depends_on` → 数组顺序
- 有 `depends_on` → Kahn's 拓扑序（同层按数组位置稳定）
- 仍顺序执行（同层不并发，未来可加 `tokio::join!`）

每步构造 `CallParams`，用 `HttpCaller` 调后端，把响应存进 `step_outputs: HashMap<String, Value>`（包装成 `{response: {status, body}}`）供后续 `InputRef::StepOutput` 解析。

错误处理：4xx/5xx 视作 step 失败，错误码 `-32011 WORKFLOW_STEP_FAILED`，`data` 字段含 step name + status + body。

## x-cli-emitter-md

实现 `SkillEmitter` trait，把 IR 渲染到 out_dir。

```rust
pub enum SkillFormat {
    Markdown,    // 默认
    Anthropic,   // Claude skill 格式
    OpenAITools, // function calling JSON
}

#[async_trait]
pub trait SkillEmitter {
    async fn emit(
        &self,
        spec: &ApiSpec,
        workflows: &[Workflow],
        out_dir: &Path,
        format: SkillFormat,
    ) -> Result<()>;
}
```

加新 emitter = `impl SkillEmitter`，不动其他 crate。

## JSON-RPC over stdio ABI

每行一个 JSON 请求，每行一个 JSON 响应。

### Methods

| Method | Params | Result | 错误码 |
|---|---|---|---|
| `ping` | — | `{ "pong": true }` | — |
| `call` | `{ endpoint_id, path_params, query, headers, body }` | `{ status, headers, body }` | -32001 / -32002 |
| `workflow.run` | `{ workflow, inputs }` | `{ status, steps[], outputs }` | -32010 / -32011 / -32012 |

### 不变量

1. **stdout 是数据，stderr 是 logging** — agent 用管道读 stdout 不会污染
2. **每行一条消息**（无分块、无多行 JSON）
3. **关闭 stdin = serve 退出**（无 keep-alive 概念）
4. **响应总是有 `id` 字段**（匹配请求），error 时 `result` 缺省

### 为什么不走 sidecar / 共享内存？

A 阶段选择 stdio JSON-RPC 是因为：
- 跨平台（任何带 stdio 的环境都能用）
- 调试简单（cat | x serve | grep 就能看）
- 沙箱友好（sandbox 通常限制网络/文件，stdio OK）
- 性能足够（每条 1ms 内往返）

未来如果需要更低延迟，可以加 sidecar 模式（x-cli 和 agent 进程共享内存），但 ABI 已经是 JSON-RPC 形式，平移成本低。

## 解析层抗压点

### `$ref` 循环检测

`ResolveCtx` 维护 `in_progress: BTreeSet<String>`（正在解析的 schema 名）。

```rust
if ctx.in_progress.contains(&name) {
    return SchemaRef { recursive: true, ... };  // 标记回填
}
ctx.in_progress.insert(name.clone());
// ... 递归解析 properties
ctx.in_progress.remove(&name);
ctx.cache.insert(name, result);
```

cache 只在解析完成后写入（**不在入口预填**——这是 v0.1-B 修过的 bug）。

### OAS 3.0 → 3.1 自动转换

`parse_openapi_str_json` 入口先把 JSON Value 遍历一遍，把 `parameters[].content` 提到 `parameters[].schema`。`oas3 0.16`（按 3.1 解析）才能正确拿到 schema。

未实现的 3.0 差异（Superset 没触发）：`nullable` / `example` 单值 / `exclusiveMinimum` 数字类型。遇到再加。

## emitter 设计约束

1. **不修改 IR** — emitter 是单向的 read-only transformation
2. **schema 字段渲染必须完整** — `$ref` 已解析为 `ResolvedSchema`，直接展开
3. **路径用 URL 编码**（B4 阶段）— 含空格的 tag 名字在 markdown 链接里用 `%20`
4. **同 signature 的响应合并**（B4 阶段）— 5 个错误响应展开成 `**400, 401, 403, 404, 500**`
5. **id 内部稳定** — 文件名可改、id 不改（agent 通过 id 调 endpoint）

## 测试分层

| 层 | 覆盖 |
|---|---|
| `x-cli-core/tests/parse.rs` | OpenAPI 解析、IR 字段、$ref 解析 |
| `x-cli-core/tests/workflow.rs` | workflow 解析、InputRef、depends_on、环检测 |
| `x-cli-core/tests/superset.rs` | 真实大文档端到端 |
| `x-cli-emitter-md/tests/emit.rs` | 三种格式 + 响应合并 + tag URL 编码 + workflow 渲染 |
| `x-cli-runtime/tests/transport.rs` | JSON-RPC 协议层 |
| `x-cli-runtime/tests/workflow_executor.rs` | workflow.run 端到端（**用本地 mock server，无网络依赖**） |

测试基础设施：
- `temp_out()` — 临时目录 helper
- `spawn_echo_server()` / `spawn_local_server()` — 本地 HTTP server
- `run_rpc()` — duplex 模拟 stdio

**所有测试 0 网络依赖，0.04 秒跑完**。

## 未来扩展点

| 想加什么 | 改哪里 | 难度 |
|---|---|---|
| OpenAPI 3.2 支持 | `openapi.rs` 加 3.2 → 3.1 转换 | 低 |
| 新 emitter（Anthropic tools 格式） | `emitter-md/src/lib.rs` 加 `impl SkillEmitter` | 中 |
| workflow 并行执行 | `executor` 同层 step 用 `tokio::join!` | 中 |
| LLM 起草 workflow | 新增 `x workflow draft` 子命令 + 接 LLM API | 高 |
| sidecar 模式 | `serve` 旁路，进程内 IPC | 中 |
| 鉴权 profile 多套 | `runtime/http.rs` 加 profile 切换 | 低 |
| mock server mode | `x serve --mock` 拦截 HTTP 返回 fixture | 中 |
