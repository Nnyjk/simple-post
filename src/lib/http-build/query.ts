import type { KeyValue } from '@/types/domain';
import { resolveVarsWithMissing } from '../url';
import type { MissingTracker } from './pool';

/**
 * 把启用的非空参数拼成 `?k=v&...` 查询串。
 *
 * 参数值与其他字段一样是模板：`{{userId}}` 在编码前对当前环境解析。与 Rust 的
 * `resolve_endpoint` 一致，只替换**值**、`key` 原样透传——后端不解析 key 里的变量，
 * 这里若解析，cURL 预览与真实发送就会不一致。
 *
 * 未启用的参数在替换前就跳过：它不会发出，其中的变量不算这次请求的问题。
 */
export function appendQueryString(
  url: string,
  params: readonly KeyValue[],
  pool: Record<string, string>,
  missing: MissingTracker,
): string {
  const parts: string[] = [];
  for (const p of params) {
    if (!p.enabled) continue;
    // `appendQueryString` 跳过空 key 的参数，其中的变量也不计入 `missing`：
    // 它不会发出，报出来是误报。判 key 排在解析值之前，与 Rust 的
    // `variable_fill::resolve_endpoint` 同序。
    if (!p.key) continue;
    const value = resolveVarsWithMissing(p.value, pool);
    missing.add(value.missing);
    // 空值判断跑在*解析后*的值上：解析为 "" 的变量与手输的空值同样丢掉该参数
    if (value.value === '') continue; // spec: skip empty keys/values
    // 空格编码成 `%20`（与 JS 的 `encodeURIComponent` 同字符集，不是 form 编码的 `+`）
    parts.push(`${encodeURIComponent(p.key)}=${encodeURIComponent(value.value)}`);
  }
  if (parts.length === 0) return url;
  return `${url}${url.includes('?') ? '&' : '?'}${parts.join('&')}`;
}
