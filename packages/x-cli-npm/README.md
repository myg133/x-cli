# @myg133/x-cli

> npm 分发包：把 `x` 二进制装到系统 PATH，支持 Windows / Linux / macOS ARM64。

## 安装

```bash
# npm 会自动安装对应平台的二进制
npm install -g @myg133/x-cli

# 或 pnpm
pnpm install -g @myg133/x-cli

# 或 yarn
yarn global add @myg133/x-cli
```

## 验证

```bash
x --version
# x 0.1.0
```

如果 `x` 找不到，**重开 shell**（PATH 修改对新 shell 才生效）。

## 安装原理

`@myg133/x-cli` 包含三平台的原生二进制文件，`install.js` 检测 `process.platform` + `process.arch` 后启动对应的二进制。

| 文件 | 适用平台 |
|---|---|
| `bin/x-win32-x64.exe` | Windows x64 |
| `bin/x-linux-x64` | Linux x64 |
| `bin/x-darwin-arm64` | macOS ARM64 (Apple Silicon) |

## 跟 x-cli 主项目的关系

| 路径 | 角色 |
|---|---|
| `crates/x-cli/` | Rust 源码（`cargo build --release` 产出二进制） |
| `packages/x-cli-npm/` | **本目录**，npm 分发包 |
| `out/x-cli-meta-skill/` | meta-skill 文档（教 agent 怎么用 x） |
| `out/superset-skill/` | 业务 skill（用 `x emit` 生成的） |

## 发布流程

版本号以 `packages/x-cli-npm/package.json` 为**单一事实源**。

手动发布（首次或紧急）:

```bash
# 1. 按需更新版本号
#    修改 packages/x-cli-npm/package.json 中的 version 字段
#    同步修改 Cargo.toml workspace version

# 2. 构建三平台二进制
cd crates/x-cli
cargo build --release

# 3. 复制到 bin/ 目录（带平台后缀）
mkdir -p ../packages/x-cli-npm/bin
cp ../../target/release/x.exe ../../packages/x-cli-npm/bin/x-win32-x64.exe
cp ../../target/release/x      ../../packages/x-cli-npm/bin/x-linux-x64
cp ../../target/release/x      ../../packages/x-cli-npm/bin/x-darwin-arm64

# 4. 发布
cd ../../packages/x-cli-npm
npm publish --access public
```

**自动化发布**: 推 `v*` tag → GitHub Actions ([ci.yml](/.github/workflows/ci.yml)) 自动构建 3 平台 + 发布到 npm + 创建 GitHub Release。

## 卸载

```bash
pnpm remove -g @myg133/x-cli
# 或
npm uninstall -g @myg133/x-cli
```

## License

MIT OR Apache-2.0