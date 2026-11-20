/**
 * 源码哨兵的共用助手：把 `src/` 下的 .ts/.tsx 文件列出来，供「某条写法只许出现在某处」的
 * 不变式按行扫。
 *
 * 文件名以一个下划线开头、不带 `.test.ts`，所以 `npm test` 不会把它当测试文件跑。
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

/** 仓库的 `src/` 绝对路径 —— 也是报错里相对路径的基准。 */
export const SRC = join(process.cwd(), 'src');

/** `src/` 下每个 .ts/.tsx 文件（跳过测试目录——那里的调用是故意的对照组）。 */
export function collectSourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) {
      if (entry === '__tests__') continue;
      collectSourceFiles(p, out);
    } else if (/\.tsx?$/.test(entry) && !entry.endsWith('.d.ts')) {
      out.push(p);
    }
  }
  return out;
}

/**
 * 在 `file` 里按行找 `re`，返回「相对路径:行号: 命中那一行」——哨兵失败时直接指出改哪里。
 *
 * `re` 不要带 `g`：带状态的正则在同一行上重复 `test` 会从上次的 `lastIndex` 接着找。
 */
export function hits(file: string, re: RegExp): string[] {
  return readFileSync(file, 'utf8')
    .split('\n')
    .flatMap((line, i) =>
      re.test(line) ? [`${file.slice(SRC.length + 1)}:${i + 1}: ${line.trim()}`] : [],
    );
}
