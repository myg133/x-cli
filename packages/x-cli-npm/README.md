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

`@myg133/x-cli` 本身不含二进制文件，而是通过 `optionalDependencies` 引用平台特定的子包：

| 子包 | 适用平台 |
|---|---|
| `@myg133/x-cli-win32-x64` | Windows x64 |
| `@myg133/x-cli-linux-x64` | Linux x64 |
| `@myg133/x-cli-darwin-arm64` | macOS ARM64 (Apple Silicon) |

`install.js` 检测 `process.platform` + `process.arch`，找到对应子包的二进制并 spawn。

`os` + `cpu` 字段让 npm 只安装匹配平台的子包。Windows 用户不会装上 Linux 二进制，反之亦然。

## 跟 x-cli 主项目的关系

| 路径 | 角色 |
|---|---|
| `crates/x-cli/` | Rust 源码（`cargo build --release` 产出二进制） |
| `packages/x-cli-npm/` | **本目录**，npm 主分发包 |
| `packages/x-cli-win32-x64/` | Windows x64 平台子包 |
| `packages/x-cli-linux-x64/` | Linux x64 平台子包 |
| `packages/x-cli-darwin-arm64/` | macOS ARM64 平台子包 |
| `out/x-cli-meta-skill/` | meta-skill 文档（教 agent 怎么用 x） |
| `out/superset-skill/` | 业务 skill（用 `x emit` 生成的） |

## 发布流程

版本号以 `packages/x-cli-npm/package.json` 为**单一事实源**。

手动发布（首次或紧急）:

```bash
# 1. 按需更新版本号
#    修改 packages/x-cli-npm/package.json 中的 version 字段
#    同步修改 Cargo.toml workspace version

# 2. 构建二进制
cd crates/x-cli
cargo build --release

# 3. 复制到各平台包目录
cp ../../target/release/x.exe ../../packages/x-cli-win32-x64/bin/x.exe
cp ../../target/release/x      ../../packages/x-cli-linux-x64/bin/x
cp ../../target/release/x      ../../packages/x-cli-darwin-arm64/bin/x

# 4. 同步平台包版本号（与主包保持一致）
cd ../../packages/x-cli-win32-x64
npm version <same-version>
cd ../x-cli-linux-x64
npm version <same-version>
cd ../x-cli-darwin-arm64
npm version <same-version>

# 5. 发布平台包（必须先发）
cd ../x-cli-win32-x64
npm publish --access public
cd ../x-cli-linux-x64
npm publish --access public
cd ../x-cli-darwin-arm64
npm publish --access public

# 6. 最后发主包
cd ../x-cli-npm
npm publish --access public
```

**自动化发布**: 推 `v*` tag → GitHub Actions ([release.yml](/.github/workflows/release.yml)) 自动构建 3 平台 + 发布 npm。

## 卸载

```bash
pnpm remove -g @myg133/x-cli
# 或
npm uninstall -g @myg133/x-cli
```

## License

MIT OR Apache-2.0