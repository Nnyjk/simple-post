/**
 * 快照文件名：`<项目名>-接口文档-<YYYYMMDD>.html`。
 *
 * 项目名会进文件名，而它来自用户输入，可能带 Windows 上非法的路径字符（`/` `\` `:` 等），也可能
 * 只剩点与空格——那样写盘会失败或落成怪名字。这里把非法字符直接删掉（不是替换成 `_`：一个
 * 「A/B」项目叫 `AB-接口文档-…` 比 `A_B-…` 好读），空白折叠成一个，首尾的点与空格去掉。全被删光
 * 时退回「项目」，保证扩展名之前始终有名字。
 */
import { formatDocDateStamp } from './model';

/** Windows 上文件名不能出现的字符；控制字符另按码位剥。 */
const ILLEGAL_FILE_CHARS = new Set(['<', '>', ':', '"', '/', '\\', '|', '?', '*']);

function stripIllegalChars(value: string): string {
  let out = '';
  for (const ch of value) {
    const code = ch.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) continue;
    if (ILLEGAL_FILE_CHARS.has(ch)) continue;
    out += ch;
  }
  return out;
}

export function docFileName(projectName: string, generatedAt: Date): string {
  const stem = stripIllegalChars(projectName)
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^[.\s]+|[.\s]+$/g, '');
  return `${stem || '项目'}-接口文档-${formatDocDateStamp(generatedAt)}.html`;
}
