/**
 * 常用 HTTP 请求头，供 Headers 页 KeyValueEditor 的 datalist 自动补全使用。
 *
 * 这份列表刻意短，并按*意图*而非 spec 字母序分组——用户敲 `a` 时要看到
 * `Authorization`，而不是被 `Accept-CH` 埋在后面。数组内的顺序就是 datalist 的
 * 显示顺序（Chrome/Firefox 遵守，Safari 按字母排——接受之）。
 *
 * 条目字段：
 *   - `name`：字面 header 名，会填进 key 字段。
 *   - `description`：单行 tooltip / 下拉副标题。编辑器渲染更丰富的 datalist 时
 *     展示；基础 `<datalist>` 只显示 `name`。
 *   - `placeholderValue?`：可选的「值该长什么样」提示，显示在 description 里
 *     （如 Authorization 的 `Bearer xxx`）。不自动填——那太魔法——但提示把用户
 *     往正确格式上引。
 *   - `source?`：内置列表是 `builtin`，从用户自己 endpoint 里收来的名字是
 *     `recent`。暴露出来，好让编辑器在渲染比裸 `<datalist>` 更丰富的 UI 时标注
 *     「最近用过」。
 */
export interface CommonHeader {
  name: string;
  description: string;
  placeholderValue?: string;
  source?: 'builtin' | 'recent';
}

/**
 * 构造编辑器消费的合并建议列表。
 *
 *   1. 遍历每个 endpoint 的 headers，收集**启用**且非空的 key。它们成为 `recent`
 *      桶——即用户自己的「全局」header，与 Apifox 全局 header 自动补全展示的一致。
 *   2. 把不在内置列表里的 recent 名字提到最前，保持首次出现顺序（最近用的优先）。
 *   3. 追加内置列表，按 header 名不分大小写去重。
 *
 * 纯函数——`endpoints` 是 store 的原始 `endpoints` 切片；过滤与排序都在这里做，
 * 调用方保持简单。
 */
export function buildHeaderSuggestions(
  endpoints: ReadonlyArray<{ headers: ReadonlyArray<{ key: string; enabled: boolean }> }>,
): CommonHeader[] {
  const seenLower = new Set<string>();
  const recent: CommonHeader[] = [];
  for (const ep of endpoints) {
    for (const h of ep.headers) {
      if (!h.enabled) continue;
      const name = h.key.trim();
      if (!name) continue;
      const lower = name.toLowerCase();
      if (seenLower.has(lower)) continue;
      seenLower.add(lower);
      recent.push({
        name,
        description: '最近使用过的 header',
        source: 'recent',
      });
    }
  }

  const builtinTail = COMMON_HEADERS.filter((h) => !seenLower.has(h.name.toLowerCase())).map(
    (h) => ({ ...h, source: 'builtin' as const }),
  );

  return [...recent, ...builtinTail];
}

export const COMMON_HEADERS: readonly CommonHeader[] = [
  // ---------- 身份 / 认证 ----------
  {
    name: 'Authorization',
    description: '凭据令牌(Bearer / Basic / 自定义方案)',
    placeholderValue: 'Bearer eyJhbGciOi...',
  },
  {
    name: 'Cookie',
    description: '发送到服务器的 Cookie 字符串',
    placeholderValue: 'session=abc; user=42',
  },
  {
    name: 'X-API-Key',
    description: 'API Key(部分厂商约定头)',
  },
  {
    name: 'X-Auth-Token',
    description: '认证 token(部分厂商约定头)',
  },

  // ---------- 内容协商 ----------
  {
    name: 'Content-Type',
    description: '请求体媒体类型',
    placeholderValue: 'application/json',
  },
  {
    name: 'Accept',
    description: '期望的响应媒体类型',
    placeholderValue: 'application/json',
  },
  {
    name: 'Accept-Language',
    description: '期望的语言(q 值可加权)',
    placeholderValue: 'zh-CN,zh;q=0.9,en;q=0.8',
  },
  {
    name: 'Accept-Encoding',
    description: '允许的压缩编码',
    placeholderValue: 'gzip, br, deflate',
  },
  {
    name: 'Accept-CH',
    description: '客户端提示,告知服务端可发送的偏好字段',
  },

  // ---------- 缓存 / 新鲜度 ----------
  {
    name: 'Cache-Control',
    description: '缓存指令',
    placeholderValue: 'no-cache',
  },
  {
    name: 'If-Match',
    description: '条件请求:要求资源 ETag 匹配',
  },
  {
    name: 'If-None-Match',
    description: '条件请求:要求资源 ETag 不匹配(常用于 304)',
  },
  {
    name: 'If-Modified-Since',
    description: '条件请求:要求资源在该时间之后被修改',
  },
  {
    name: 'ETag',
    description: '资源标识符',
  },

  // ---------- CORS / 安全 ----------
  {
    name: 'Origin',
    description: '请求来源(浏览器跨域时自动加)',
  },
  {
    name: 'Referer',
    description: '上一个页面的 URL',
  },
  {
    name: 'X-Requested-With',
    description: '常用于 AJAX 标识,部分后端据此判断是否走 JSON 分支',
    placeholderValue: 'XMLHttpRequest',
  },
  {
    name: 'X-Forwarded-For',
    description: '代理链中的原始客户端 IP',
  },
  {
    name: 'X-Real-IP',
    description: '网关设置的原始客户端 IP',
  },

  // ---------- User-Agent ----------
  {
    name: 'User-Agent',
    description: '客户端标识',
    placeholderValue: 'simple-post/0.1',
  },

  // ---------- 追踪 / 可观测性 ----------
  {
    name: 'X-Request-Id',
    description: '请求唯一 ID,便于服务端日志关联',
  },
  {
    name: 'X-Correlation-Id',
    description: '分布式追踪 ID,跨服务关联',
  },
  {
    name: 'Traceparent',
    description: 'W3C Trace Context 父 span 信息',
  },

  // ---------- 杂项 ----------
  {
    name: 'Host',
    description: '目标主机(由 URL 推导,通常无需手动设置)',
  },
  {
    name: 'Range',
    description: '请求资源的字节范围',
    placeholderValue: 'bytes=0-1023',
  },
];
