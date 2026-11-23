/**
 * 测试运行器：自动发现 `src/` 下所有 `*.test.ts` 并交由 `tsx --test` 执行。
 *
 * 替代 package.json 中硬编码的文件列表，新增测试文件无需手动更新脚本即可被 `npm test` 拾取。
 *
 * 用法：
 *   node scripts/run-tests.mjs           # 单次运行
 *   node scripts/run-tests.mjs --watch   # 监听模式（对应 npm run test:watch）
 *
 * 注意：`--watch` 模式在启动时收集文件列表，新增或重命名测试文件后需重启监听进程。
 * 输出相对仓库根目录的路径（无空格与 shell 特殊字符），便于 Windows 下安全执行。
 */
import { spawn } from 'node:child_process';
import { readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SRC = path.join(root, 'src');
const SKIP_DIRS = new Set(['node_modules', 'dist', 'target', 'build']);

/** @param {string} dir 扫描目录 @param {string[]} out 结果收集数组 @returns {string[]} 测试文件路径列表 */
function collect(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out; // src/ 不存在时回落至下方的空列表错误
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) collect(full, out);
    } else if (entry.isFile() && entry.name.endsWith('.test.ts')) {
      out.push(path.relative(root, full).replaceAll(path.sep, '/'));
    }
  }
  return out;
}

const files = collect(SRC).sort();
if (files.length === 0) {
  console.error('[test] no *.test.ts files found under src/');
  process.exit(1);
}

const watch = process.argv.includes('--watch');
const flags = watch ? ['--watch', '--test'] : ['--test'];
console.log(`[test] ${files.length} test file(s):`);
for (const f of files) console.log(`  - ${f}`);

// 拼接完整命令字符串替代 args 数组 + shell: true 组合，避免 Node DEP0190 弃用告警。
// 所有参数均为无空格与元字符的相对路径，安全可拼接。
const command = ['tsx', ...flags, ...files].join(' ');
const child = spawn(command, {
  cwd: root,
  stdio: 'inherit',
  shell: true, // Windows 下执行 tsx.cmd 需通过 shell
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 1);
  }
});
child.on('error', (err) => {
  console.error(`[test] failed to spawn tsx: ${err.message}`);
  process.exit(1);
});
