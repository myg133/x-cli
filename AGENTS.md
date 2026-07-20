# AGENTS.md

> 给在这个 repo 里干活的 AI / 人看的协作手册。**已存在的 `README.md` / `ARCHITECTURE.md` / `EXAMPLE.md` / `SUPERSET.md` 是给人读的主文档**——这个文件只放 AI 协作专用的 cheat sheet，不重复。
>
> 任何 AI agent 进入这个 repo 后，**第一件事**是读这个文件。

## 项目速览

| 项 | 值 |
|---|---|
| 是什么 | `x` CLI —— 把后端 OpenAPI 文档转成 agent 可加载的 skill |
| Repo | `git@github.com:myg133/x-cli.git`（branch: `main`）|
| 当前版本 | v0.1.0（已发 tag: v0.1-A → v0.1-H2，共 11 个）|
| License | MIT OR Apache-2.0 |
| Stack | Rust 1.75+（`rust-version = "1.75"`），edition 2021，tokio 异步运行时 |

## 工程结构

Cargo workspace，4 个 crate。**依赖方向只向下指**（`x-cli` 是叶节点，`x-cli-core` 不依赖任何内部 crate）。

```
crates/
├── x-cli/                # 二进制 x，CLI 入口（parse / emit / serve）
├── x-cli-core/           # IR + OpenAPI 解析 + workflow 解析 + JSON-RPC schema
├── x-cli-runtime/        # stdio JSON-RPC + HTTP 客户端 + workflow executor + auth
└── x-cli-emitter-md/     # SkillEmitter trait + 3 个 impl
```

| crate | 角色 | 关键文件 |
|---|---|---|
| `x-cli` | CLI 入口（`clap` 子命令）| `src/main.rs` |
| `x-cli-core` | 语义层，IR 全在这里 | `src/ir.rs`（数据模型）/ `openapi.rs`（OAS 解析 + 3.0→3.1 转换）/ `workflow.rs`（DAG 校验）/ `protocol.rs`（JSON-RPC + 错误码）/ `error.rs` |
| `x-cli-runtime` | 传输 + 执行 | `src/transport.rs`（`serve_stdio` + generic `serve<R,W>`）/ `http.rs`（`HttpCaller`）/ `workflow_executor.rs`（拓扑执行 + `InputRef` 解析）/ `auth.rs`（`AuthProfile`）|
| `x-cli-emitter-md` | skill 渲染 | `src/lib.rs`（`SkillFormat` enum + `SkillEmitter` trait + `MarkdownEmitter`）|

`lib.rs` 的 re-export 模式：`x-cli-core` 的 `lib.rs` 把所有公开类型 re-export（`pub use ir::ApiSpec`），**外部 crate 写 `use x_cli_core::ApiSpec` 而非 `x_cli_core::ir::ApiSpec`**。新加模块保持这个约定。

## 跑命令

```bash
# 构建
cargo build --release                  # 产物 target/release/x(.exe)
cargo build --workspace --all-targets  # 含测试

# 测试（**必须** 0 网络，~0.04s 跑完）
cargo test --workspace --all-targets
cargo test -p x-cli-core
cargo test -p x-cli-emitter-md
cargo test -p x-cli-runtime

# 跑单个测试
cargo test -p x-cli-core parse_oas_3_0

# Lint / format
cargo fmt --all                        # 自动改
cargo fmt --all -- --check             # CI 跑这个
cargo clippy --workspace --all-targets # 当前 CI 是 warning-only
```

CI 配置在 `.github/workflows/ci.yml`，矩阵 `ubuntu-latest` + `windows-latest`，job 步骤：fmt check → build → test → clippy。env 里有 `RUSTFLAGS: -D warnings` —— **任何 warning 都会挂 CI**。

## ABI 不变量（**不要破**）

1. **IR 是独立 crate，emitter 用 trait 抽象** —— 加新 emitter = 加 `impl SkillEmitter`，不动 core / runtime。
2. **skill ↔ x-cli 是 JSON-RPC over stdio** —— 每行一条消息，stdout 是数据 / stderr 是 logging，关闭 stdin = serve 退出。

外加几个隐式不变量（破一个 = 已发布的 skill 全部失效）:

- `Endpoint.id` 稳定 —— 格式 `<Domain>__<method>__<sanitized_path>`，文件可改名，**id 不能改**。agent 调接口全靠这个 id。
- JSON-RPC 错误码稳定 —— `-32700` / `-32600` / `-32601` / `-32602` / `-32001` / `-32002` / `-32010` / `-32011` / `-32012`，定义在 `protocol.rs::error_code`。agent 端 hardcode 这些码。
- `.x-cli/ir.json` 是 serve 加载 IR 的唯一入口 —— emit 阶段必须写出（任何 format 都要，因为 serve 跑 workflow 需要）。
- `workflows/<name>.yaml` 是 serve 启动时按文件加载 workflow 的约定位置。

## 代码风格

### 必须遵守

- **中文优先** —— doc comment、用户可见错误信息、CLI help、commit message 全用中文。**代码标识符用英文**。
- **不滥用 emoji** —— 注释 / log 里基本没有；CLI 输出里 `✓` / `✗` 是约定。
- **新加的 public item 必须有 `///` doc** —— `x-cli-core` 和 `x-cli-emitter-md` 开了 `#![warn(missing_docs)]`。
- **`#![warn(missing_docs)]` 之外不要新加 lint 开关** —— 配合 CI 的 `-D warnings`，新 lint 容易把全员挂掉。
- 用 `anyhow::Result` / `x_cli_core::Result` + `.with_context(...)`，**非测试代码不用 `unwrap()`**。
- `tracing` 记日志（runtime / core）；CLI 的进度输出用 `println!` 走 stdout（约定）。

### 模块组织

- 公开 API 在 `lib.rs` re-export（`pub use ...::...;`）。
- 内部模块用 `pub(crate)` 而非 `pub` —— 减少 API surface。
- 一个模块一个文件，别堆 `mod foo { ... }` 在 `lib.rs` 里。

## 测试约定（**硬性**）

| 规则 | 原因 |
|---|---|
| 集成测试放 `tests/` 目录（**不**放 `#[cfg(test)] mod tests`）| 跟公开 API 解耦，模拟外部用户视角 |
| **0 网络依赖** —— CI 跑 ~0.04s | 不能让 `cargo test` 受网络影响 |
| fixture 用 `include_str!("fixtures/<name>.yaml")` 内联 | 跨平台相对路径会挂 |
| fixture 放 `tests/fixtures/` | 别散落 |
| 加新功能 = 至少 1 正向 + 1 反向测试 | 没测试的 PR 别合 |

### 已有测试 helper

- `temp_out()` —— 临时目录（每个 crate 自己有，用 `SystemTime::nanos` 命名保证唯一），见 `x-cli-emitter-md/tests/emit.rs`。
- `tokio::io::duplex(...)` + `serve()` 模拟 stdio JSON-RPC —— 见 `x-cli-runtime/tests/transport.rs` 的 `round_trip`。
- 本地 HTTP mock server —— 见 `x-cli-runtime/tests/workflow_executor.rs`。

## 任务速查

### 加新 emitter（Cursor / MCP / Gemini / …）

1. `x-cli-emitter-md/src/lib.rs` 加新 `impl SkillEmitter for XxxEmitter`（依赖很重就新建 crate）。
2. `SkillFormat` enum 加新 variant + 调好 `Default`。
3. `x-cli/src/main.rs` 的 `SkillFormatArg` + `From` impl 同步加。
4. README "三种输出格式" 章节加示例。
5. `x-cli-emitter-md/tests/emit.rs` 加测试。

### 加新 JSON-RPC method

1. `x-cli-core/src/protocol.rs` 加 `RpcMethod` 变体 + `*Params` / `*Result` schema。
2. `x-cli-runtime/src/transport.rs` 的 `handle_line` 加 match 分支。
3. 新错误码 → 在 `protocol.rs::error_code` 加常量。
4. 测试 → `x-cli-runtime/tests/transport.rs` 的 `round_trip` 加 case。

### 加新 workflow 特性（并行 / retry / timeout / …）

1. `WorkflowStep` 加字段（`ir.rs`）+ `workflow.rs` 解析逻辑。
2. `workflow_executor.rs` 的执行循环读新字段。
3. 校验规则（环、未知引用、自依赖）跟新字段一起加。
4. 至少 1 正向 + 1 反向测试。
5. README workflow 章节 + `EXAMPLE.md` 同步。

### 加新 CLI 子命令

1. `x-cli/src/main.rs` 的 `Cmd` enum 加变体 + `#[arg(...)]`。
2. `main()` 的 match 同步加。
3. README 命令表格 + 一行。

### 升级 OpenAPI spec 解析（3.2 / 3.3 / …）

- `x-cli-core/src/openapi.rs` 加新转换函数（仿 `convert_oas_3_0_to_3_1`），入口调用。
- 加 fixture 到 `tests/fixtures/`（用真实世界的 spec）。
- 先看 `oas3` crate 版本支不支持再决定写不写自定义转换。

## 提交 / 分支约定

- commit message 格式: `v0.1-X: <一句话描述>`（看 `git log --oneline`）。
- 描述中文，**首字母不大写**，动词开头（"解析"、"启动"、"加固"……）。
- 一个 commit 一个 feature。**不要**把 fmt / 改 typo 跟 feature 混一起。
- 工作分支命名: `v0.1-X-desc` 或按 issue 号。
- **CI 必须绿** —— `cargo fmt --check` + `cargo build` + `cargo test` 三件套都过。

## 调试速查

```bash
# 看 IR（debug）
.\target\release\x.exe parse examples/petstore.yaml | Select-Object -First 50

# 用 petstore 跑端到端
.\target\release\x.exe emit examples/petstore.yaml --out .\out/petstore-skill `
    --workflow examples/petstore-workflow.yaml
echo '{"jsonrpc":"2.0","id":1,"method":"ping"}' | .\target\release\x.exe serve --skill .\out/petstore-skill

# 真实大文档（Superset 1.27MB / 276 endpoint）
.\target\release\x.exe emit examples/superset.json --out .\out/superset-skill --format anthropic

# workflow 端到端
echo '{"jsonrpc":"2.0","id":1,"method":"workflow.run","params":{
  "workflow":"买宠物并查询订单","inputs":{"petName":"fluffy"}
}}' | .\target\release\x.exe serve --skill .\out/petstore-skill
```

## "不要做" 清单

- ❌ 改 `Endpoint.id` 生成规则 —— 已发布 skill 全部依赖。
- ❌ 给 `x-cli-core` 加 `tokio` / `reqwest` 依赖 —— core 是同步纯计算层。
- ❌ 改 `protocol.rs` 的错误码数值 —— agent 端 hardcode。
- ❌ emoji 当 commit message 主体。
- ❌ 让 `cargo test` 跑超过 0.5s —— 慢测试 = 测试写得不对。
- ❌ 在非测试代码里用 `unwrap()`。
- ❌ 破坏 `RUSTFLAGS: -D warnings`。
- ❌ 在测试里写真实网络调用。
- ❌ 提交前不跑 `cargo fmt --all`（CI 会挂）。

## 进一步阅读

按场景挑:

- 第一次进来 → `README.md`
- 架构 / 约束细节 → `ARCHITECTURE.md`
- 端到端怎么用 → `EXAMPLE.md`
- 真实 OpenAPI（Superset）对接 → `SUPERSET.md`
- 公开 API 形状 → `crates/<crate>/src/lib.rs`

## 产物目录速查

`out/` 是 .gitignore 排除的目录，**生成的 skill 不进 git**。目前项目里有两类产物：

| 路径 | 性质 | 怎么用 |
|---|---|---|
| `out/superset-skill/` | 业务 skill（用 `x emit` 生成的，276 endpoint）| agent 加载它调 Superset API |
| `out/x-cli-meta-skill/` | **手写的 meta-skill**（`x-cli skill factory`），**自包含**：`bin/x.exe`（9.4 MB Windows binary）+ docs + 4 个 examples | agent 加载它来**造**业务 skill（教 agent 用 x-cli） |

`out/x-cli-meta-skill/` 是本仓库**独有的** — 不是 x-cli 自带，是项目作者为了"让 agent 自动化造 skill"额外写的。它的 frontmatter 用 `name: x-cli skill factory`，**当 agent 收到"用 x-cli 把 OpenAPI 转成 skill"的请求时自动加载**。

业务 skill 改完要重新 emit；meta-skill 改完直接提交（它不进 gitignore 的白名单外，但属于手写产物）。

meta-skill 的命令用 `.\bin\x.exe`（Windows）/ `./bin/x`（POSIX），业务 skill 默认输出到 `./generated/<name>/`（meta-skill 内）。**详见 `out/x-cli-meta-skill/distribution.md`**。
