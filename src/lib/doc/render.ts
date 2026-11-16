/**
 * 文档模型 → 自包含 HTML。纯函数：不读 DOM、不读时钟、不读 store。
 *
 * 「自包含」是**承诺**，不是样式偏好：产物里没有 `<script>`、没有 `<link>`、没有外部字体与图片，
 * 只有一段 inline `<style>`。接收方双击就能看，放进内网 wiki 的 iframe 也不会拉第三方资源——
 * 对一份会被转发的文件来说，这两点都要紧。
 *
 * 「不可更改」同样落在结构上：页面里没有任何编辑入口，所有内容都是生成那一刻的字节。工作区之后
 * 怎么改都动不了它。
 *
 * 深浅两套由 `prefers-color-scheme` 切换，用 CSS 变量，没有第二份样式表。
 */
import { escapeHtml as esc } from './escape';
import { renderMarkdownDoc } from './markdown';
import type { ApiDocModel, DocEndpoint, DocField, DocGroup, DocResponse } from './model';

/** 生成整份快照。同一份模型 + 同一 `generatedAt` ⇒ 逐字节相同。 */
export function renderApiDocHtml(model: ApiDocModel): string {
  const placed: Array<{ group: DocGroup; anchor: string }> = [];
  const endpointAnchors = new Map<string, string>();

  const place = (group: DocGroup): void => {
    placed.push({ group, anchor: `section-${placed.length + 1}` });
    for (const endpoint of group.endpoints) {
      endpointAnchors.set(endpoint.id, `endpoint-${endpointAnchors.size + 1}`);
    }
    for (const child of group.children) place(child);
  };
  place(model.root);

  const anchorByGroup = new Map(placed.map((entry) => [entry.group.id, entry.anchor]));

  const html: string[] = [];
  html.push('<!DOCTYPE html>');
  html.push('<html lang="zh-CN">');
  html.push('<head>');
  html.push('<meta charset="utf-8">');
  html.push('<meta name="viewport" content="width=device-width, initial-scale=1">');
  html.push('<meta name="color-scheme" content="light dark">');
  html.push(`<title>${esc(model.projectName)} · 接口文档</title>`);
  html.push(`<style>${DOC_STYLE}</style>`);
  html.push('</head>');
  html.push('<body>');
  html.push('<div class="doc">');
  html.push('<header class="doc-head">');
  html.push('<p class="kicker">接口文档 · 快照</p>');
  html.push(`<h1>${esc(model.projectName)}</h1>`);
  html.push(
    `<p class="summary">${model.moduleName ? `模块「${esc(model.moduleName)}」 · ` : ''}${model.endpointCount} 个接口 · 生成于 <time datetime="${esc(
      model.generatedAtIso,
    )}">${esc(model.generatedAt)}</time></p>`,
  );
  html.push(
    '<p class="notice">这是某次导出生成的<strong>快照</strong>：内容取自当时<strong>已保存</strong>的接口定义，之后在工作区里做的任何改动都不会出现在这份文件里。</p>',
  );
  html.push(renderEnvironment(model));
  html.push('</header>');
  html.push(renderToc(placed, endpointAnchors));
  html.push('<main>');
  html.push(renderGroup(model.root, anchorByGroup, endpointAnchors));
  html.push('</main>');
  html.push(
    '<footer class="doc-foot"><p>本文件由 simple-post 导出，是静态快照：不含脚本，不含外链。</p></footer>',
  );
  html.push('</div>');
  html.push('</body>');
  html.push('</html>');
  return html.join('\n');
}

/** 环境变量的**名字**清单。值不输出——那是文档里最不该出现的东西。 */
function renderEnvironment(model: ApiDocModel): string {
  const env = model.environment;
  if (!env || env.variables.length === 0) return '';
  const items = env.variables
    .map(
      (v) =>
        `<li><code>${esc(v.name)}</code>${v.disabled ? ' <span class="tag">已停用</span>' : ''}</li>`,
    )
    .join('');
  return `<details class="env"><summary>环境「${esc(env.name)}」的变量名</summary><ul>${items}</ul><p class="hint">只列名字；变量的取值不会写进文档。</p></details>`;
}

function renderToc(
  placed: ReadonlyArray<{ group: DocGroup; anchor: string }>,
  endpointAnchors: ReadonlyMap<string, string>,
): string {
  const items: string[] = [];
  for (const { group, anchor } of placed) {
    items.push(
      `<li class="toc-group"><a href="#${anchor}">${esc(group.name)}</a>${
        group.endpoints.length === 0
          ? ''
          : `<ul>${group.endpoints
              .map(
                (ep) =>
                  `<li class="toc-endpoint"><a href="#${endpointAnchors.get(ep.id) ?? ''}"><span class="verb">${esc(
                    ep.method,
                  )}</span> ${esc(ep.url)}</a></li>`,
              )
              .join('')}</ul>`
      }</li>`,
    );
  }
  return `<nav class="toc"><h2>目录</h2><ul>${items.join('')}</ul></nav>`;
}

function renderGroup(
  group: DocGroup,
  anchorByGroup: ReadonlyMap<string, string>,
  endpointAnchors: ReadonlyMap<string, string>,
): string {
  const parts: string[] = [];
  parts.push(`<section class="group" id="${anchorByGroup.get(group.id) ?? ''}">`);
  parts.push(`<h2>${esc(group.name)}</h2>`);
  for (const endpoint of group.endpoints) {
    const anchor = endpointAnchors.get(endpoint.id);
    parts.push(renderEndpoint(endpoint, anchor ? ` id="${anchor}"` : ''));
  }
  for (const child of group.children)
    parts.push(renderGroup(child, anchorByGroup, endpointAnchors));
  parts.push('</section>');
  return parts.join('\n');
}

function renderEndpoint(endpoint: DocEndpoint, anchorAttr: string): string {
  const parts: string[] = [];
  const verb = endpoint.method.toLowerCase();
  const tone = /^(get|post|put|patch|delete|head|options)$/.test(verb) ? `m-${verb}` : 'm-other';

  parts.push(`<article class="endpoint"${anchorAttr}>`);
  parts.push(
    `<h3 class="signature"><span class="method ${tone}">${esc(endpoint.method)}</span><code class="url">${esc(
      endpoint.url,
    )}</code></h3>`,
  );
  parts.push(`<p class="endpoint-name">${esc(endpoint.name)}</p>`);
  if (endpoint.description) parts.push(`<p class="endpoint-desc">${esc(endpoint.description)}</p>`);
  const docs = renderMarkdownDoc(endpoint.docs);
  if (docs) parts.push(`<div class="prose">${docs}</div>`);

  const hasRequest =
    endpoint.pathParams.length > 0 ||
    endpoint.params.length > 0 ||
    endpoint.headers.length > 0 ||
    endpoint.auth.kind !== 'none' ||
    Boolean(endpoint.body);
  if (hasRequest) {
    parts.push('<h4>请求</h4>');
    parts.push(table('路径参数', endpoint.pathParams));
    parts.push(table('查询参数', endpoint.params));
    parts.push(table('请求头', endpoint.headers));
    if (endpoint.auth.kind !== 'none') {
      parts.push(
        `<p class="field-line">鉴权：<code>${esc(endpoint.auth.kind)}</code>${
          endpoint.auth.detail ? ` · ${esc(endpoint.auth.detail)}` : ''
        }</p>`,
      );
    }
    parts.push(renderBody(endpoint));
  }

  if (endpoint.responses.length > 0) {
    parts.push('<h4>响应定义</h4>');
    for (const response of endpoint.responses) parts.push(renderResponse(response));
  }

  const notes = renderMarkdownDoc(endpoint.notes);
  if (notes) {
    parts.push('<h4>备注</h4>');
    parts.push(`<div class="prose">${notes}</div>`);
  }

  parts.push('</article>');
  return parts.join('\n');
}

/** 名 + 说明两列，没有值。空表不出。 */
function table(title: string, fields: readonly DocField[]): string {
  if (fields.length === 0) return '';
  const rows = fields
    .map((f) => `<tr><td><code>${esc(f.name)}</code></td><td>${esc(f.description)}</td></tr>`)
    .join('');
  return `<h5>${title}</h5><table class="fields"><thead><tr><th>名称</th><th>说明</th></tr></thead><tbody>${rows}</tbody></table>`;
}

function renderBody(endpoint: DocEndpoint): string {
  const body = endpoint.body;
  if (!body) return '';
  const parts: string[] = [];
  parts.push('<h5>请求体</h5>');
  parts.push(
    `<p class="field-line">模式 <code>${esc(body.mode)}</code> · Content-Type <code>${esc(body.contentType)}</code></p>`,
  );
  if (body.schema) {
    parts.push('<p class="label">Schema</p>');
    parts.push(codeBlock(canonicalJsonText(body.schema)));
  }
  return parts.join('');
}

function renderResponse(response: DocResponse): string {
  const parts: string[] = [];
  const bucket = /^[1-5]/.test(response.status) ? `s-${response.status.charAt(0)}xx` : 's-other';
  parts.push('<section class="response">');
  parts.push(
    `<p class="response-head"><span class="status ${bucket}">${esc(response.status)}</span>${
      response.description ? `<span class="response-desc">${esc(response.description)}</span>` : ''
    }<span class="content-type">${esc(response.contentType)}</span></p>`,
  );
  if (response.schema) {
    parts.push('<p class="label">Schema</p>');
    parts.push(codeBlock(JSON.stringify(response.schema, null, 2)));
  }
  if (response.example) {
    parts.push('<p class="label">示例</p>');
    parts.push(codeBlock(canonicalJsonText(response.example)));
  }
  parts.push('</section>');
  return parts.join('');
}

/**
 * schema / example 是用户手写的文本：能解析成 JSON 就重排成两空格缩进（同一份定义换个排列法
 * 也让产物保持一致），解析不了就原样输出——文档不该因为一段坏 JSON 而丢掉用户写的东西。
 */
function canonicalJsonText(raw: string): string {
  const text = raw.trim();
  if (text === '') return '';
  try {
    return JSON.stringify(JSON.parse(text), null, 2) ?? text;
  } catch {
    return text;
  }
}

function codeBlock(text: string): string {
  return `<pre class="code"><code>${esc(text)}</code></pre>`;
}

const DOC_STYLE = `:root{color-scheme:light dark;--bg:#fff;--fg:#17171b;--muted:#5f6068;--faint:#8a8b95;--border:#e4e4ea;--panel:#f7f7f9;--code:#f4f4f7;--accent:#2f5fe0;--notice-bg:#fff9e8;--notice-border:#e9dcb3;--notice-fg:#5a4408}
@media (prefers-color-scheme:dark){:root{--bg:#151519;--fg:#e9e9ee;--muted:#a0a1ad;--faint:#797a85;--border:#2b2b33;--panel:#1c1c22;--code:#1a1a20;--accent:#8aabff;--notice-bg:#251f10;--notice-border:#4d3f1c;--notice-fg:#e6cf94}}
*{box-sizing:border-box}
html{-webkit-text-size-adjust:100%}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.7 -apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}
.doc{max-width:940px;margin:0 auto;padding:40px 24px 72px}
h1{font-size:26px;line-height:1.3;margin:6px 0 8px}
h2{font-size:19px;margin:36px 0 12px;padding-bottom:6px;border-bottom:1px solid var(--border)}
h3{font-size:15px;margin:0;font-weight:600}
h4{font-size:13px;margin:22px 0 8px;color:var(--muted);text-transform:uppercase;letter-spacing:.04em}
h5{font-size:12px;margin:14px 0 6px;color:var(--muted);font-weight:600}
p{margin:8px 0}
a{color:var(--accent)}
code{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:.92em}
.kicker{margin:0;font-size:12px;letter-spacing:.12em;text-transform:uppercase;color:var(--faint)}
.summary{margin:0 0 14px;color:var(--muted);font-size:13px}
.notice{margin:16px 0 0;padding:10px 14px;border:1px solid var(--notice-border);border-left-width:3px;border-radius:6px;background:var(--notice-bg);color:var(--notice-fg);font-size:13px}
.env{margin-top:14px;font-size:13px;color:var(--muted)}
.env summary{cursor:pointer}
.env ul{margin:8px 0 0;padding-left:20px}
.env li{margin:2px 0}
.hint{font-size:12px;color:var(--faint)}
.tag{display:inline-block;margin-left:6px;padding:0 6px;border-radius:4px;background:var(--panel);border:1px solid var(--border);font-size:11px;color:var(--faint)}
.toc{margin:26px 0 0;padding:14px 18px;border:1px solid var(--border);border-radius:8px;background:var(--panel)}
.toc h2{margin:0 0 8px;padding:0;border:0;font-size:13px;color:var(--muted);text-transform:uppercase;letter-spacing:.06em}
.toc ul{margin:0;padding-left:16px;font-size:13px}
.toc>ul{padding-left:0;list-style:none}
.toc li{margin:3px 0}
.toc-group>a{font-weight:600}
.toc-endpoint{color:var(--muted)}
.verb{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:11px}
.group{margin-top:12px}
.endpoint{margin:22px 0 0;padding:18px 20px;border:1px solid var(--border);border-radius:10px;background:var(--bg)}
.signature{display:flex;flex-wrap:wrap;align-items:center;gap:10px;margin:0 0 6px}
.signature .url{font-size:13.5px;word-break:break-all}
.method{display:inline-block;min-width:56px;padding:2px 8px;border-radius:5px;background:var(--panel);border:1px solid var(--border);font:600 11px/1.6 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;text-align:center;letter-spacing:.04em}
.m-get{color:#1a7f4b;border-color:#1a7f4b55;background:#1a7f4b12}
.m-post{color:#2f5fe0;border-color:#2f5fe055;background:#2f5fe012}
.m-put{color:#a06000;border-color:#a0600055;background:#a0600012}
.m-patch{color:#7b3fbf;border-color:#7b3fbf55;background:#7b3fbf12}
.m-delete{color:#b3282d;border-color:#b3282d55;background:#b3282d12}
.m-head,.m-options,.m-other{color:var(--muted)}
.endpoint-name{margin:0;font-weight:600}
.endpoint-desc{margin:6px 0 0;color:var(--muted);white-space:pre-line}
.prose{font-size:14px;margin:10px 0 0}
.prose>:first-child{margin-top:0}
.prose>:last-child{margin-bottom:0}
.prose pre,.prose code{background:var(--code);border-radius:4px}
.prose pre{padding:10px 12px;overflow-x:auto}
.prose code{padding:1px 4px}
.prose pre code{padding:0;background:none}
.prose table{border-collapse:collapse;font-size:13px}
.prose th,.prose td{border:1px solid var(--border);padding:5px 9px;text-align:left}
.prose blockquote{margin:10px 0;padding:2px 14px;border-left:3px solid var(--border);color:var(--muted)}
.prose img{max-width:100%}
.fields{width:100%;border-collapse:collapse;font-size:13px}
.fields th,.fields td{border:1px solid var(--border);padding:6px 10px;text-align:left;vertical-align:top}
.fields th{background:var(--panel);font-weight:600;color:var(--muted)}
.fields td:first-child{width:32%;word-break:break-all}
.field-line{font-size:13px;color:var(--muted);margin:10px 0 0}
.label{margin:12px 0 4px;font-size:11px;letter-spacing:.06em;text-transform:uppercase;color:var(--faint)}
.code{margin:0;padding:10px 12px;border:1px solid var(--border);border-radius:6px;background:var(--code);overflow-x:auto;font-size:12.5px;line-height:1.6}
.response{margin:12px 0 0;padding:12px 14px;border:1px solid var(--border);border-radius:8px;background:var(--panel)}
.response-head{display:flex;flex-wrap:wrap;align-items:center;gap:10px;margin:0}
.status{font:600 12px/1.6 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;padding:1px 8px;border-radius:4px;border:1px solid var(--border);background:var(--bg)}
.s-2xx{color:#1a7f4b}
.s-3xx{color:#a06000}
.s-4xx{color:#b3282d}
.s-5xx{color:#7b3fbf}
.response-desc{font-size:13px}
.content-type{margin-left:auto;font:11px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;color:var(--faint)}
.doc-foot{margin-top:48px;padding-top:14px;border-top:1px solid var(--border);font-size:12px;color:var(--faint)}
@media (max-width:640px){.doc{padding:28px 14px 56px}.endpoint{padding:14px}.fields td:first-child{width:40%}}`;
