# 分发与打包

> meta-skill 目录是**纯文档**（不包含 binary）。binary 通过 npm 分发（`@myg133/x-cli`）。
> 本文档说明 meta-skill 怎么打包 / 怎么分发 / 跟 npm 包的关系 / 业务 skill 输出位置约定。

## 关键：meta-skill ≠ binary

**两份资产，独立分发**：

| 资产 | 路径 | 分发方式 | 安装方式 |
|---|---|---|---|
| meta-skill 文档 | `out/x-cli-meta-skill/` | git 仓库 / zip / 拷贝 | 装到 `~/.claude/skills/` 等 |
| x-cli binary | `packages/x-cli-npm/` | npm publish | `pnpm install -g @myg133/x-cli` |

**agent 加载 meta-skill 后**，meta-skill 会**自动**引导 agent 跑 `pnpm install -g @myg133/x-cli` 装 binary。

## 自包含边界（meta-skill 目录）

**包含**（必须一起分发）：

- `SKILL.md`（入口）
- `references/commands.md` / `references/auth-references.md` / `references/workflow-references.md` / `references/troubleshooting.md` / `references/scope.md`
- `distribution.md`（本文档）
- `examples/*.md`（5 个端到端范例）

**不包含**：

- `bin/` —— binary 不在 meta-skill 里！装 `@myg133/x-cli` npm 包
- `generated/` —— 业务 skill 产物目录，运行时由 `x emit` 写入
- 任何用户提供的 OpenAPI 文档

**约定**：

- 业务 skill 默认输出到 `<meta-skill>/generated/<name>/`
- 用户可用 `--out <任意路径>` 覆盖
- `generated/` 永远不应该被提交（git 管理时加 `.gitignore` 排除 `generated/`）

## meta-skill 打包

### 打成 zip（跨平台通用）

```bash
# POSIX
zip -r x-cli-skill.zip out/x-cli-meta-skill/ -x "out/x-cli-meta-skill/generated/*"

# Windows PowerShell
Compress-Archive -Path out/x-cli-meta-skill -DestinationPath x-cli-skill.zip
```

**排除 `generated/`**——里面可能是几百 MB 的业务 skill（Superset 1.27MB OpenAPI emit 出来 ~3.2MB skill），不应该进分发包。

### 散文件直接拷

```bash
# 给 Claude Code 注入（POSIX）
cp -r out/x-cli-meta-skill ~/.claude/skills/x-cli-skill-factory

# Windows
Copy-Item -Recurse out\x-cli-meta-skill "$env:USERPROFILE\.claude\skills\x-cli-skill-factory"
```

### 校验完整性

```bash
# POSIX
test -f out/x-cli-meta-skill/SKILL.md && \
test -f out/x-cli-meta-skill/references/commands.md && \
test -f out/x-cli-meta-skill/distribution.md && \
echo "OK: meta-skill 完整"

# Windows PowerShell
$files = "SKILL.md","references\commands.md","distribution.md"
$missing = $files | Where-Object { -not (Test-Path "out\x-cli-meta-skill\$_") }
if ($missing) { "缺失: $missing" } else { "OK: meta-skill 完整" }
```

## 安装 binary（npm）

**首次使用前**，agent / 用户必须装 binary：

```bash
# 推荐
pnpm install -g @myg133/x-cli

# 或
npm install -g @myg133/x-cli
```

**当前只 Windows x64**。POSIX 用户：

- 本机有 Rust 工具链：`cargo install x-cli` 或自己 build
- 没 Rust：等 cross-compile CI

## 平台说明

| 平台 | meta-skill 文档 | binary 怎么用 |
|---|---|---|
| **Windows PowerShell** | 装到 `~\.claude\skills\` | `pnpm install -g @myg133/x-cli` → 裸 `x` |
| **POSIX bash** | 装到 `~/.claude/skills/` | `cargo install` 或等 cross-compile → 裸 `x` |

**meta-skill 文档跨平台通用**。binary 跨平台要等 npm 包扩展（看 `packages/x-cli-npm/README.md`）。

## 业务 skill 输出位置

### 默认（推荐）

```bash
x emit examples/petstore.yaml --out ./generated/petstore-skill
x emit examples/superset.json  --out ./generated/superset-skill
```

**优点**：

- meta-skill 目录 = 完整工具链（文档 + generated/ 业务 skill 产物）
- `generated/` 是约定位置，agent 一看就知道是产物
- 不污染项目根 `out/`

**注意**：

- `generated/` 在 meta-skill 内是约定的，**不进 git**
- meta-skill 目录总大小 = ~50 KB docs + N × 业务 skill 体积

### 覆盖到任意位置

```bash
# Windows
x emit examples/petstore.yaml --out C:\Users\me\skills\petstore

# POSIX
x emit examples/petstore.yaml --out /opt/skills/petstore
```

**适用场景**：

- 想把多个 meta-skill 实例的产物汇总到一处
- 想跟项目根的 `out/` 兼容（旧习惯）
- 业务 skill 想被其他 agent 直接看到（比如放在 `~/.claude/skills/`）

### 跟 serve 的关系

`x serve --skill <DIR>` 不在乎 skill 目录在哪——绝对 / 相对路径都行。

```bash
# 业务 skill 在 meta-skill 内
x serve --skill ./generated/petstore-skill

# 业务 skill 在其他地方
x serve --skill C:\Users\me\skills\petstore
```

## meta-skill 分发清单

最小清单（**这个一定要有**）：

1. `out/x-cli-meta-skill/SKILL.md`
2. `out/x-cli-meta-skill/references/commands.md`
3. `out/x-cli-meta-skill/references/auth-references.md`
4. `out/x-cli-meta-skill/references/workflow-references.md`
5. `out/x-cli-meta-skill/references/troubleshooting.md`
6. `out/x-cli-meta-skill/distribution.md`

可选清单（**强烈建议带上**）：

7. `out/x-cli-meta-skill/examples/*.md`（4 个）

不要发（**会污染**）：

- `out/x-cli-meta-skill/generated/`（运行时产物）

**binary 不在 meta-skill 里**，单独从 npm 包装。

## 排错

### 拷过去跑不了

- 检查 `SKILL.md` 是否完整（UTF-8 编码、frontmatter 完整）
- 检查 `examples/` 目录是否完整（4 个 md 都在）
- meta-skill 拷走后**还要装 binary**：`pnpm install -g @myg133/x-cli`

### agent 找不到 x

- 没装 binary：跑 `pnpm install -g @myg133/x-cli` 后**重开 shell**
- 装了但 `x --version` 找不到 platform 错误：当前只 Windows x64
- agent 沙箱禁止执行 binary：把 `node_modules/.bin/` 加到沙箱白名单或用 `require_escalated` 跑

### generated/ 越来越大

- 业务 skill 不会被自动清理——meta-skill 不管这块
- 手动清：

  ```bash
  # POSIX
  rm -rf ./generated/old-skill

  # Windows
  Remove-Item .\generated\old-skill -Recurse -Force
  ```

### 想升级 binary

```bash
# 升级 npm 包
pnpm update -g @myg133/x-cli

# 卸载重装
pnpm remove -g @myg133/x-cli
pnpm install -g @myg133/x-cli
```

binary 升级**不影响** meta-skill 文档（它们独立）。
