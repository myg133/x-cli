#!/usr/bin/env node
// x-cli 平台检测 + 二进制执行
// npm install -g 后，bin 链接指向这个文件，它找到对应平台的原生二进制并运行

const { spawn } = require('child_process');
const path = require('path');

// 平台 → 平台包名映射
const PLATFORM_PACKAGES = {
  'win32-x64': 'x-cli-win32-x64',
  'linux-x64': 'x-cli-linux-x64',
  'darwin-arm64': 'x-cli-darwin-arm64',
};

const key = `${process.platform}-${process.arch}`;
const pkgName = PLATFORM_PACKAGES[key];

if (!pkgName) {
  console.error(
    `x-cli 不支持当前平台 ${key}。支持：win32-x64, linux-x64, darwin-arm64`
  );
  process.exit(1);
}

const binName = process.platform === 'win32' ? 'x.exe' : 'x';

// resolve 路径：这个脚本在 <prefix>/lib/node_modules/@myg133/x-cli/install.js
// 平台包在 <prefix>/lib/node_modules/@myg133/<pkgName>/bin/<binName>
const binary = path.join(__dirname, '..', '..', '@myg133', pkgName, 'bin', binName);

const child = spawn(binary, process.argv.slice(2), {
  stdio: 'inherit',
});

child.on('exit', (code) => process.exit(code ?? 1));
child.on('error', (err) => {
  console.error(`x-cli 启动失败: ${err.message}`);
  process.exit(1);
});