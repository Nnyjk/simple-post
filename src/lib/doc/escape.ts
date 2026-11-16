/**
 * HTML 转义 —— 文档渲染的**唯一**出口。
 *
 * 文档里的每一段文字（项目名、接口名、接口说明、参数名、schema 文本、Markdown 产物）都可能来自
 * 别人写的定义，而这份 HTML 是要被转发出去的：一个漏转的 `<` 就是一次脚本执行。故渲染层不直接
 * 拼裸字符串，所有插值都过这个函数（`<` `>` `&` `"` `'` 五个字符全转，属性值与文本用同一套——
 * 属性里少了 `"` 就能闭合属性注入事件处理器）。
 *
 * 已知限制：它只做转义，不做清洗。若某处确实要插「已经是 HTML」的内容（例如 Markdown 产物），
 * 那必须由调用方保证那段 HTML 自己也是转义过的，绝不能把用户原文当 HTML 塞进来。
 */
export function escapeHtml(value: string): string {
  let out = '';
  for (const ch of value) {
    switch (ch) {
      case '&':
        out += '&amp;';
        break;
      case '<':
        out += '&lt;';
        break;
      case '>':
        out += '&gt;';
        break;
      case '"':
        out += '&quot;';
        break;
      case "'":
        out += '&#39;';
        break;
      default:
        out += ch;
    }
  }
  return out;
}

/**
 * 链接协议白名单：`http` / `https` / `mailto`、相对路径、`#` 锚点，其余（`javascript:`、`data:`、
 * `file:`、`vbscript:`……）一律不认。
 *
 * 判定前先剥掉 ASCII 控制字符与空格 —— 浏览器解析 URL 时会忽略它们，`java\tscript:` 在地址栏里
 * 就是 `javascript:`，不先规范化就等于给绕过留了门。
 *
 * 注意 `//host/path`（协议相对地址）走的是「没有显式协议」那条分支，按 `http(s)` 对待，属白名单。
 *
 * 数字字符引用要先还原再判定：`[x](javascript&#58;alert(1))` 在我们这里输出时 `&` 会被再转义、
 * 变成字面量，但 Markdown 解析器也可能已经把 `&#58;` 解成 `:`；两种来路都要在判定前归一。
 */
export function isSafeHref(href: string): boolean {
  const normalized = stripUrlNoise(decodeEntities(href));
  if (normalized === '') return false;
  const scheme = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(normalized);
  if (!scheme) return true;
  const name = scheme[1].toLowerCase();
  return name === 'http' || name === 'https' || name === 'mailto';
}

/**
 * `href` 属性值的转义。Markdown 解析器交下来的地址**已经做过一次**实体编码
 * （`?a=1&b=2` 到这里是 `?a=1&amp;b=2`），再套 `escapeHtml` 会变成 `&amp;amp;`，接收方点开
 * 就是查询参数 `amp;b`。故先还原实体再转一次：输出里只有一层编码，浏览器读到的正是原地址。
 */
export function escapeHref(href: string): string {
  return escapeHtml(decodeEntities(href));
}

/** 浏览器解析 URL 时忽略 ASCII 控制字符与空格（`java\tscript:` 就是 `javascript:`），故先全部剥掉。 */
function stripUrlNoise(value: string): string {
  let out = '';
  for (const ch of value) {
    const code = ch.codePointAt(0) ?? 0;
    if (code <= 0x20 || code === 0x7f) continue;
    out += ch;
  }
  return out;
}

/**
 * 还原实体引用。白名单判定必须做这一步：`jav&#97;script:`、`javascript&colon;` 都是浏览器认的
 * 写法。命名实体只收常见的几个——其余保持字面量时协议判定不成立，链接会退化成一个打不开的相对
 * 地址，既不是脚本也不是漏网的跳板。
 */
function decodeEntities(value: string): string {
  const named = value.replace(
    /&(amp|lt|gt|quot|apos|colon|tab|newline|sol|period|nbsp);/gi,
    (match, name: string) => NAMED_ENTITIES[name.toLowerCase()] ?? match,
  );
  return named.replace(/&#(x[0-9a-fA-F]+|\d+);/g, (_match, body: string) => {
    const code =
      body.startsWith('x') || body.startsWith('X')
        ? Number.parseInt(body.slice(1), 16)
        : Number(body);
    return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : '';
  });
}

/** 只收会影响协议判定的那几个：冒号、制表/换行、斜杠、点，其余还原出来也没意义。 */
const NAMED_ENTITIES: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  colon: ':',
  tab: '\t',
  newline: '\n',
  sol: '/',
  period: '.',
  nbsp: '\u00a0',
};
