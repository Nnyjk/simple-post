import { fromBuildInput, prepareRequest } from './prepare';
import { parseBodyFile, parseMultipartParts } from '../body';
import type { BodyFile, MultipartPart } from '@/types/domain';
import type { BuildInput, PreparedRequest } from './types';

/** POSIX shell 单引号转义：`it's` → `'it'\''s'`。 */
function shellQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}

/**
 * cURL 的 `-F` 参数值里的一个子参数（`filename=…` / `type=…`）。
 *
 * cURL 自己那套规则：值里有 `;` 或空白时用双引号包起来，而 `"` 与 `\` 要反斜杠转义。
 * CR/LF 整个去掉——留着就是把一个 `-F` 拆成两条参数。
 */
function formParamValue(value: string): string {
  const clean = value
    .replace(/[\r\n]/g, '')
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"');
  return /[;\s]/.test(clean) ? `"${clean}"` : clean;
}

/**
 * 一个分片 → 一条 `-F`。
 *
 * 文件分片用 `@路径`：**cURL 自己读那个文件**，故命令不会随文件变大，复现的仍是线上那份
 * 字节。文本分片给原文。
 */
function multipartField(part: MultipartPart): string {
  if (!part.path) return `  -F ${shellQuote(`${part.name}=${part.value}`)}`;

  const params = [
    `filename=${formParamValue(part.filename || part.path.replace(/^.*[\\/]/, ''))}`,
    `type=${formParamValue(part.contentType || 'application/octet-stream')}`,
  ];
  return `  -F ${shellQuote(`${part.name}=@${part.path};${params.join(';')}`)}`;
}

/**
 * binary 模式的文件路径。内容读不出文件描述时返回 `null`：那时请求体本来就是空的，
 * cURL 里也不该出现一条指向不存在文件的 `--data-binary`。
 */
function binaryPath(body: string): string | null {
  const file: BodyFile | null = parseBodyFile(body);
  return file && file.path ? file.path : null;
}

/**
 * 把已 prepare 的请求渲染成可粘贴的 cURL 命令。
 *
 * **全应用唯一拼 cURL 字符串的地方。** `buildCurlCommand` 只是它在 `BuildInput` 之上的适配器：
 * 预览若自己重新推导 URL、header 与 body，就是在描述一个没人发过的请求。
 *
 * 这里读到的一切都已是终态：`url` 已 join 且已解析 `{{var}}`，`headers` 是含 auth 派生
 * `Authorization` 的有序线上列表。
 *
 * **文件按路径引用，不把字节塞进命令行**：`binary` 用 `--data-binary @路径`，`multipart` 的
 * 文件分片用 `-F 名字=@路径`。cURL 自己读盘，故这条命令复现的就是线上那一份字节，而命令
 * 本身不会随文件变大。
 *
 * `multipart` 的 Content-Type 不在这里出现：boundary 由 cURL 自己生成，用户写过的 Content-Type
 * 在执行层会被装配时生成的那条替换，故 `-H 'Content-Type: …'` 只会让命令与真实请求不一致。
 */
export function preparedToCurl(prepared: PreparedRequest): string {
  const lines: string[] = [`curl -X ${prepared.method.toUpperCase()} ${shellQuote(prepared.url)}`];
  // `preparedToCurl` 用列表而非 record：`prepared.headers` 保留重名，线上合法，且每行一个
  // `-H` 正好复现。
  for (const [k, v] of prepared.headers) {
    if (prepared.bodyMode === 'multipart' && k.toLowerCase() === 'content-type') continue;
    lines.push(`  -H ${shellQuote(`${k}: ${v}`)}`);
  }

  if (prepared.bodyMode === 'multipart') {
    for (const part of parseMultipartParts(prepared.body ?? '')) {
      if (!part.enabled) continue;
      lines.push(multipartField(part));
    }
    return lines.join(' \\\n');
  }

  if (prepared.bodyMode === 'binary') {
    const path = binaryPath(prepared.body ?? '');
    if (path !== null) lines.push(`  --data-binary @${shellQuote(path)}`);
    return lines.join(' \\\n');
  }

  if (prepared.body !== null) lines.push(`  --data-raw ${shellQuote(prepared.body)}`);
  return lines.join(' \\\n');
}

/**
 * 为同一请求生成可粘贴的 cURL 命令。`prepareRequest` → `preparedToCurl` 之上的薄适配器，
 * 故发出的 URL、header、body 按构造就是真实发送会用的那套。
 */
export function buildCurlCommand(input: BuildInput): string {
  return preparedToCurl(prepareRequest(fromBuildInput(input)));
}
