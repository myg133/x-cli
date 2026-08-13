#!/usr/bin/env node
// x-cli 平台检测 + 二进制执行
// npm install -g 后，bin 链接指向这个文件，它从同包 bin/ 目录下找到对应平台的原生二进制并运行

const { spawn } = require('child_process');
const path = require('path');

// 平台 → 二进制文件名映射
const BINARY_NAMES = {
  'win32-x64': 'x-win32-x64.exe',
  'linux-x64': 'x-linux-x64',
  'darwin-arm64': 'x-darwin-arm64',
};

const key = `${process.platform}-${process.arch}`;
const binName = BINARY_NAMES[key];

if (!binName) {
  console.error(
    `x-cli 不支持当前平台 ${key}。支持：win32-x64, linux-x64, darwin-arm64`
  );
  process.exit(1);
}

const binary = path.join(__dirname, 'bin', binName);

const child = spawn(binary, process.argv.slice(2), {
  stdio: 'inherit',
});

child.on('exit', (code) => process.exit(code ?? 1));
child.on('error', (err) => {
  console.error(`x-cli 启动失败: ${err.message}`);
  process.exit(1);
});