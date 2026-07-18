# x-cli

把后端 OpenAPI 接口转成 agent 可加载的 skill。

## 它解决什么

AI agent 通常靠 skill 调用外部工具，skill 内部又通过 CLI 落地执行。
x-cli 想做的事：**把后端 OpenAPI 文档当作单一来源，自动生成这一类 skill**。

- 作者侧：给一份 OpenAPI（yaml/json），x-cli 解析、整理、归类、生成 skill 文档
- 运行侧：agent 加载 skill 后，通过 stdio 上的 JSON-RPC 调 x-cli，x-cli 转发到后端 HTTP

## 当前状态（v0.1，A 阶段）

- ✅ OpenAPI 3 解析（`oas3` 库）
- ✅ IR 数据模型（`ApiSpec` / `Domain` / `Endpoint` / `Schema`）
- ✅ JSON-RPC 2.0 over stdio ABI（`call` + `ping` method）
- ✅ HTTP 客户端（reqwest + auth profile 占位）
- ✅ Markdown skill emitter（`SKILL.md` + 每个 endpoint 一份 md）
- ✅ 端到端跑通：OpenAPI → emit → serve → JSON-RPC call → HTTP

未来要做（B 阶段）：

- 流程建模：`workflow.yaml` 描述多步工作流，LLM 辅助起草
- 多 emitter：Anthropic skill 格式、OpenAI function calling、纯 tool-calling JSON
- `$ref` 解析、循环检测、allOf/oneOf 处理
- sidecar 模式：x-cli 与 agent 进程共享内存
- 鉴权 profile 多套切换、连接池配置、mock 模式

## 用法

### 1. 把 OpenAPI 转成 skill

```bash
x emit examples/petstore.yaml --out ./out/petstore-skill
```

产物：

```
out/petstore-skill/
├── SKILL.md                        # 总索引（域、接口列表、调用约定）
├── endpoints/
│   ├── pet__get__pets.md           # 每个接口一份
│   ├── pet__post__pets.md
│   └── ...
└── .x-cli/
    └── ir.json                     # 内部 IR，serve 时读它
```

### 2. 启动 JSON-RPC 服务

```bash
x serve --skill ./out/petstore-skill
```

走 stdio：每行一个 JSON 请求，每行一个 JSON 响应。

### 3. Agent 调一次 call

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"call","params":{"endpoint_id":"echo__get__anything_path","path_params":{"path":"hello"},"query":{},"headers":{},"body":null}}' \
  | x serve --skill ./out/httpbin-skill
```

返回：

```json
{"jsonrpc":"2.0","id":1,"result":{"status":200,"headers":{...},"body":{...}}}
```

### 4. 在 agent 代码里

每个 `endpoints/<id>.md` 都自带调用示例。基本模式：

```python
import json, subprocess
req = {
    "jsonrpc": "2.0", "id": 1, "method": "call",
    "params": {
        "endpoint_id": "<id>",
        "path_params": {...}, "query": {...},
        "headers": {...}, "body": {...}
    }
}
proc = subprocess.run(["x", "serve", "--skill", "<skill_dir>"],
                     input=json.dumps(req), capture_output=True, text=True)
resp = json.loads(proc.stdout.strip())
```

## 工程结构

```
crates/
├── x-cli/             # 主二进制
├── x-cli-core/        # IR + OpenAPI 解析 + 协议 schema（独立 crate，便于其他 emitter 复用）
├── x-cli-runtime/     # JSON-RPC over stdio + HTTP 客户端
└── x-cli-emitter-md/  # markdown skill emitter（实现 SkillEmitter trait）
```

依赖方向：`x-cli` → `{runtime, core, emitter-md}`，emitter 和 runtime 都基于 `core` 的 IR/协议 schema。

## AB 兼容性纪律

A 阶段就守住的两个不变量：

1. **IR 独立 crate，emitter 用 trait 抽象** — 加 emitter 不改 core
2. **skill ↔ x-cli ABI 是 JSON-RPC over stdio，预留 sidecar** — 不改成 CLI 参数串

按这个节奏，后面 B 阶段做多 emitter / 多平台 / 工作流，都不需要动这条线。

## 开发

```bash
cargo build --release
cargo test
cargo check --workspace
```

## License

MIT OR Apache-2.0
