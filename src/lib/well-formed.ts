/**
 * 良构字符串：里面没有落单的代理项（UTF-16 的 D800–DFFF 里没有配对的那一个）。
 *
 * `encodeURIComponent` 遇到落单的代理项直接抛 `URIError`，而请求是在渲染期组装的，
 * 异常一路到 React 根节点，整棵工作区树被卸载。故这里把落单的代理项换成 U+FFFD
 * （WHATWG 的 UTF-8 编码也是这么处理的）。
 */
export function wellFormed(value: string): string {
  // `/u` 模式下正则按码元匹配：配成一对的代理项是一个增补平面字符，不落在代理项区，
  // 故这个模式只命中落单的那一个。
  return value.replace(/\p{Surrogate}/gu, '\uFFFD');
}

/**
 * 递归归一：对象与数组逐层走，字符串逐个换成良构字符串。
 *
 * 只用于纯数据的配置（请求入参、鉴权配置这类），故返回值与入参同形，但不是同一个对象。
 */
export function wellFormedDeep<T>(value: T): T {
  if (typeof value === 'string') return wellFormed(value) as T;
  if (Array.isArray(value)) return value.map((item) => wellFormedDeep(item)) as T;
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    for (const [key, item] of Object.entries(value as Record<string, unknown>)) {
      out[key] = wellFormedDeep(item);
    }
    return out as T;
  }
  return value;
}
