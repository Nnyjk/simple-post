import { fromBuildInput, prepareRequest } from './prepare';
import type { BuildInput, PreparedRequest } from './types';

/** POSIX shell 单引号转义：`it's` → `'it'\''s'`。 */
function shellQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}

/**
 * 把已 prepare 的请求渲染成可粘贴的 cURL 命令。
 *
 * **全应用唯一拼 cURL 字符串的地方。** `buildCurlCommand` 只是它在 `BuildInput`
 * 之上的适配器：预览若自己重新推导 URL、header 与 body，就是在描述一个没人发过的请求。
 *
 * 这里读到的一切都已是终态：`url` 已 join 且已解析 `{{var}}`，`headers` 是含 auth
 * 派生 `Authorization` 的有序线上列表，`body` 是真正会写进 socket 的那个字符串。
 *
 * **二进制 body 走 `--data-binary`**：命令行里塞的是 base64 的文本，而 cURL 会把
 * `--data-raw` 的内容原样发出去（那就把 base64 文本发出去了）。`@<(…)` 是 bash 的
 * 进程替换：管道先把 base64 解成字节，再交给 cURL 发出去，故这条命令复现的就是线上
 * 那一份字节。
 */
export function preparedToCurl(prepared: PreparedRequest): string {
  const lines: string[] = [`curl -X ${prepared.method.toUpperCase()} ${shellQuote(prepared.url)}`];
  // `preparedToCurl` 用列表而非 record：`prepared.headers` 保留重名，线上合法，且每行一个
  // `-H` 正好复现
  for (const [k, v] of prepared.headers) {
    lines.push(`  -H ${shellQuote(`${k}: ${v}`)}`);
  }
  if (prepared.bodyBase64 !== null) {
    lines.push(`  --data-binary @<(printf '%s' ${shellQuote(prepared.bodyBase64)} | base64 -d)`);
  } else if (prepared.body !== null) {
    lines.push(`  --data-raw ${shellQuote(prepared.body)}`);
  }
  return lines.join(' \\\n');
}

/**
 * 为同一请求生成可粘贴的 cURL 命令。`prepareRequest` → `preparedToCurl` 之上的
 * 薄适配器，所以发出的 URL、header、body 按构造就是真实发送会用的那套。
 */
export function buildCurlCommand(input: BuildInput): string {
  return preparedToCurl(prepareRequest(fromBuildInput(input)));
}
