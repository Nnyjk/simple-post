/**
 * 良构字符串 —— 里面没有落单的代理项。
 *
 * 代理项（UTF-16 的 D800–DFFF）是 UTF-16 用来给增补平面字符占位的两个码元。两个代理项配成
 * 一对才是一个合法字符（一个 emoji 就是一个高位代理项加一个低位代理项）；落单的那一个在
 * UTF-8 里没有对应字节。
 *
 * WHATWG 的 UTF-8 编码把落单的代理项编码成 U+FFFD（替换字符），Rust 的
 * `String::from_utf8_lossy` 是同一套取舍。故本模块与浏览器、后端对「表示不出来的字符」的
 * 处理一致。
 *
 * **为什么渲染进程需要这一步。** JS 字符串按 UTF-16 存，用户在编辑器里粘进一个落单的代理项
 * 是可能的。平台的 `encodeURIComponent` 遇到它直接抛 `URIError`，而请求是在渲染期组装的，
 * 异常一路到 React 根节点，整棵工作区树被卸载。
 *
 * **为什么后端不需要。** Rust 的 `String` 恒为合法 UTF-8，收不到落单的代理项；JSON 里的
 * `\uD800` 转义 serde_json 也直接拒绝（见 `crates/simple-post-core/src/body_test.rs`
 * 里那条用例）。
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
 * 它一次覆盖掉所有字段，故新增一个带用户文本的字段时不必记得再补一次归一。
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
