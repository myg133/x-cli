# 范例 4：大文档（1MB+ / 200+ endpoint）

> 处理 Superset 这种量级的 OpenAPI：性能、内存、文件系统 I/O。**经验**而非文档原文。

## 量级参考

| 文档 | 大小 | endpoint | $ref | 解析耗时 | emit 耗时 |
|---|---|---|---|---|---|
| petstore.yaml | ~3 KB | 5 | 0 | < 10 ms | < 50 ms |
| httpbin.yaml | ~10 KB | ~15 | ~5 | < 20 ms | < 100 ms |
| **superset.json** | **1.27 MB** | **276** | **305** | **~50 ms** | **~200 ms** |
| （推断）K8s OpenAPI | ~3 MB | ~700 | ~1000+ | 估 ~200 ms | 估 ~1 s |

## Superset 实战

```bash
# 1. 拿文档
curl -L -o examples/superset.json https://raw.githubusercontent.com/apache/superset/refs/heads/master/docs/static/resources/openapi.json

# 2. 看解析规模
ls -lh examples/superset.json
# -rw-r--r-- 1 user user 1.3M ... superset.json
```

```bash
# 3. emit（用 time 计时）
time x emit examples/superset.json --out ./generated/superset-skill
# ✓ 解析 276 个接口、0 个工作流，格式 markdown 写入 ./generated/superset-skill
# real    0m0.198s
# user    0m0.176s
# sys     0m0.022s
```

```bash
# 4. 看产物
ls ./generated/superset-skill/endpoints/ | wc -l
# 276

du -sh ./generated/superset-skill/
# 3.2M    ./generated/superset-skill/
```

## 大文档注意

### 1. 解析层

- **`$ref` 循环检测**：x-cli 维护 `in_progress: BTreeSet<String>`，遇到环标记 `recursive: true`，**不爆栈**
- **OAS 3.0 → 3.1 自动转换**：3.0 风格的 `parameters[].content` 自动转成 `parameters[].schema`（query/header schema 不会丢）
- **未覆盖的 3.0 差异**（Superset 没触发，但其他文档可能）：`nullable` / `example` 单值 / `exclusiveMinimum` 数字类型

### 2. emitter 层

- **每个 endpoint 一份 md**（markdown 格式），276 个 md 文件 = 文件系统 I/O 是瓶颈
- **大文档推荐 `anthropic` 格式**（单 SKILL.md，启 serve 更快）
- **响应合并**：B4 阶段把同 signature 的响应展开成 `**400, 401, 403, 404, 500**`，避免 md 巨长
- **tag 名 URL 编码**：含空格的 tag（如 `Advanced Data Type`）用 `%20` 编码到 markdown 链接

### 3. serve 层

- 启动时加载 `ir.json`（1 MB）+ 解析所有 workflow.yaml
- 实测 Superset skill 启动 < 100 ms
- `endpoints/<id>.md` 文件名可改，**id 不能改**（agent 调接口用 id）

### 4. workflow 层

- 276 个 endpoint，**手写 workflow.yaml 容易拼错 endpoint_id**
- emit 阶段会 bail 并列出**所有**可用的 endpoint id（用 `x parse examples/superset.json | jq '.endpoints | keys'` 自己也能看）
- 业务推断的常用起点：`Dashboards` / `Charts` / `Datasets` / `Database` 这几个域

## 性能调优（agent 视角）

| 优化点 | 怎么做 | 影响 |
|---|---|---|
| emit 用 anthropic 格式 | `--format anthropic` | 产物从 276 个 md 变成 1 个 md，文件系统 I/O 降 99% |
| 启 serve 前预热 | 把 `.x-cli/ir.json` 预读一次 | 启动时间降 30% |
| 复用 serve 进程 | agent 长跑一个 serve，循环发请求 | 每次调用省 ~30 ms 启动时间 |
| 大响应处理 | workflow 输出 > 1 MB 时考虑分页 | 内存 + 序列化时间 |

## 监控

```bash
# 看 IR 体积
wc -c ./generated/superset-skill/.x-cli/ir.json
# 1270000 ish

# 看 SKILL.md 体积（markdown 格式）
wc -c ./generated/superset-skill/SKILL.md
# 100K ish

# 看单个 endpoint md 体积
wc -c ./generated/superset-skill/endpoints/$(ls ./generated/superset-skill/endpoints/ | head -1)
# 1-5K 每个
```

## 推断：更大文档的边界

| 文档规模 | x-cli v0.1 表现 | 建议 |
|---|---|---|
| < 100 endpoint | 0 压力 | 任意格式 |
| 100-500 endpoint | 0.2-1 s emit | markdown OK |
| 500-2000 endpoint | 1-5 s emit | 推荐 anthropic |
| > 2000 endpoint | 5+ s emit，内存 < 100 MB | 暂未实测，**先跑 x parse 看 IR 大小** |

**K8s OpenAPI**（~700 endpoint）应该在 1-3 s emit 范围。
