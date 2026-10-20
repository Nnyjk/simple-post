/**
 * `multipart/form-data` 的装配与发送单测。
 *
 * 覆盖两条承重的性质：boundary 在 body 与 Content-Type 里是**同一个字符串**
 * （对不上时请求看起来成功、服务端解析失败），以及既有行为没有被牵连：
 * `form` 仍是 `application/x-www-form-urlencoded`、用户已设的 Content-Type 不被覆盖、
 * `{{var}}` 模板化照旧作用在 `content` 上。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildFetchInit, buildCurlCommand, prepareRequest } from '../http-build';
import { parseMultipartParts, buildMultipartBody, newMultipartBoundary } from '../multipart';
import { headerMap, input, prep, ENV } from './http-build-fixtures';

const PARTS = JSON.stringify([
  { name: 'field', value: 'v1' },
  { name: 'file', value: 'BINARY', filename: 'a.txt' },
]);

/** 从 Content-Type 里取出声明的 boundary。 */
function declaredBoundary(contentType: string): string {
  const m = /^multipart\/form-data; boundary=(.+)$/.exec(contentType);
  assert.ok(m, `not a multipart Content-Type: ${contentType}`);
  return m![1]!;
}

// ============================================================
// boundary 一致性
// ============================================================

describe('http-build: multipart boundary', () => {
  it('the Content-Type boundary is the one the body is framed with', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'multipart', content: PARTS } }));
    const body = init.body as string;
    const b = declaredBoundary((init.headers as Record<string, string>)['Content-Type']!);

    assert.ok(body.startsWith(`--${b}\r\n`), 'body must open with the declared boundary');
    assert.ok(body.endsWith(`--${b}--\r\n`), 'body must close with the declared boundary');
  });

  it('the same holds on the prepareRequest / cURL path', () => {
    const prepared = prepareRequest(
      prep({ method: 'POST', body: { mode: 'multipart', content: PARTS } }),
    );
    assert.ok(prepared.body !== null);
    const b = declaredBoundary(headerMap(prepared.headers)['Content-Type']!);
    assert.ok(prepared.body!.startsWith(`--${b}\r\n`));
    assert.ok(prepared.body!.endsWith(`--${b}--\r\n`));
  });

  it('cURL preview frames its body with the boundary it declares', () => {
    // `buildCurlCommand` 自己跑一遍 prepare，所以比对的是**它自己的** Content-Type
    // 与它自己的 body —— boundary 每请求现生成，两次调用本就不该相同
    const curl = buildCurlCommand(
      input({ method: 'POST', body: { mode: 'multipart', content: PARTS } }),
    );
    const m = /Content-Type: multipart\/form-data; boundary=([A-Za-z0-9-]+)/.exec(curl);
    assert.ok(m, `no multipart Content-Type in cURL:\n${curl}`);
    const b = m![1]!;

    assert.ok(curl.includes(`--${b}\r\n`));
    assert.ok(curl.includes(`--${b}--\r\n`));
  });

  it('is generated per request, so two sends never share a boundary', () => {
    const one = buildFetchInit(input({ body: { mode: 'multipart', content: PARTS } }));
    const two = buildFetchInit(input({ body: { mode: 'multipart', content: PARTS } }));
    const ct = (init: RequestInit) =>
      declaredBoundary((init.headers as Record<string, string>)['Content-Type']!);

    assert.notEqual(ct(one.init), ct(two.init));
  });

  it('newMultipartBoundary is a legal RFC 2046 token within 70 chars', () => {
    const b = newMultipartBoundary();
    assert.ok(b.length <= 70);
    assert.match(b, /^[A-Za-z0-9-]+$/);
  });
});

// ============================================================
// 分片装配
// ============================================================

describe('http-build: multipart body shape', () => {
  it('frames a plain field and a file part with CRLF', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'multipart', content: PARTS } }));
    const b = declaredBoundary((init.headers as Record<string, string>)['Content-Type']!);

    assert.equal(
      init.body,
      `--${b}\r\nContent-Disposition: form-data; name="field"\r\n\r\nv1\r\n` +
        `--${b}\r\nContent-Disposition: form-data; name="file"; filename="a.txt"\r\n` +
        `Content-Type: application/octet-stream\r\n\r\nBINARY\r\n` +
        `--${b}--\r\n`,
    );
  });

  it('gives a file part no Content-Type when the user set one', () => {
    const content = JSON.stringify([
      { name: 'f', value: 'd', filename: 'a.png', contentType: 'image/png' },
    ]);
    const { init } = buildFetchInit(input({ body: { mode: 'multipart', content } }));
    assert.match(init.body as string, /Content-Type: image\/png/);
    assert.ok(!(init.body as string).includes('application/octet-stream'));
  });

  it('leaves a plain field without a Content-Type', () => {
    const { init } = buildFetchInit(
      input({ body: { mode: 'multipart', content: JSON.stringify([{ name: 'a', value: '1' }]) } }),
    );
    assert.ok(!(init.body as string).includes('Content-Type:'));
  });

  it('escapes quotes and strips CRLF out of header params', () => {
    const content = JSON.stringify([
      { name: 'a"b\r\nX-Evil: 1', value: 'v', filename: 'c\\d.txt' },
    ]);
    const { init } = buildFetchInit(input({ body: { mode: 'multipart', content } }));
    const body = init.body as string;

    assert.ok(body.includes('name="a\\"bX-Evil: 1"'));
    assert.ok(body.includes('filename="c\\\\d.txt"'));
    // CR/LF 留着就是往请求头里注入一个新头
    assert.ok(!body.includes('X-Evil: 1\r\n'));
  });
});

// ============================================================
// 非法输入：零分片，仍是格式合法的 multipart
// ============================================================

describe('http-build: malformed multipart content', () => {
  for (const [label, content] of [
    ['not JSON', 'definitely not json'],
    ['an object rather than an array', '{"name":"a"}'],
    ['a wrongly typed field', '[{"name":"a","value":10}]'],
    ['a non-object element', '["a"]'],
  ] as const) {
    it(`yields zero parts for ${label}, and still sends a legal body`, () => {
      const { init } = buildFetchInit(input({ body: { mode: 'multipart', content } }));
      const b = declaredBoundary((init.headers as Record<string, string>)['Content-Type']!);
      assert.equal(init.body, `--${b}--\r\n`);
    });
  }

  it('parseMultipartParts mirrors that leniency', () => {
    assert.deepEqual(
      parseMultipartParts(PARTS).map((p) => p.name),
      ['field', 'file'],
    );
    assert.deepEqual(parseMultipartParts('nope'), []);
  });

  it('an explicit null optional field is the same as omitting it', () => {
    // Rust 侧的 `Option` 把 JSON `null` 读成 `None`——与「不写这个键」同义。
    // 两侧必须同形：否则 agent 写 `"filename": null` 时，MCP 发一个分片、
    // UI 发零分片，同一个 body 成了两份。
    const parsed = parseMultipartParts(
      '[{"name":"f","value":"v","filename":null,"contentType":null}]',
    );
    assert.equal(parsed.length, 1);
    assert.equal(parsed[0].name, 'f');
    assert.equal(parsed[0].filename, undefined);
    assert.equal(parsed[0].contentType, undefined);
  });

  it('empty content sends no body at all (hasBody is false)', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'multipart', content: '' } }));
    assert.equal(init.body, undefined);
  });
});

// ============================================================
// 未被牵连的既有行为
// ============================================================

describe('http-build: multipart does not disturb the other modes', () => {
  it('form is still application/x-www-form-urlencoded', () => {
    const { init } = buildFetchInit(input({ body: { mode: 'form', content: 'a=1' } }));
    assert.equal(
      (init.headers as Record<string, string>)['Content-Type'],
      'application/x-www-form-urlencoded',
    );
    assert.equal(init.body, 'a=1');
  });

  it('a user-set Content-Type is not overwritten in the hand-written modes', () => {
    for (const mode of ['json', 'form', 'raw'] as const) {
      const content = mode === 'form' ? 'a=1' : mode === 'raw' ? 'x' : '{"a":1}';
      const { init } = buildFetchInit(
        input({
          headers: [
            { id: 'h1', key: 'Content-Type', value: 'application/vnd.me+json', enabled: true },
          ],
          body: { mode, content },
        }),
      );
      assert.equal(
        (init.headers as Record<string, string>)['Content-Type'],
        'application/vnd.me+json',
        `mode=${mode} must keep the user's Content-Type`,
      );
    }
  });

  it('but multipart overrides it, because that body is ours to frame', () => {
    // 其余模式的 body 是用户手写的，用户那条 Content-Type 有机会描述它；multipart
    // 的 body 是我们用 boundary 框出来的，用户那条值描述不了它。留着就是「发得错」。
    const { init } = buildFetchInit(
      input({
        headers: [{ id: 'h1', key: 'content-type', value: 'application/json', enabled: true }],
        body: { mode: 'multipart', content: PARTS },
      }),
    );
    const cts = Object.entries(init.headers as Record<string, string>).filter(
      ([k]) => k.toLowerCase() === 'content-type',
    );
    assert.equal(cts.length, 1, 'the header is replaced, not duplicated');

    // header 名保留用户的大小写，值换成生成的
    assert.equal(cts[0]![0], 'content-type');
    const b = declaredBoundary(cts[0]![1]);
    assert.ok(
      (init.body as string).startsWith(`--${b}\r\n`),
      "the overridden Content-Type must still declare the body's own boundary",
    );
  });

  it('resolves {{var}} in multipart part values, reporting the missing ones', () => {
    const content = JSON.stringify([
      { name: 'a', value: '{{userId}}' },
      { name: 'b', value: '{{nope}}' },
    ]);
    const prepared = prepareRequest(
      prep({ method: 'POST', body: { mode: 'multipart', content }, env: ENV }),
    );
    assert.match(prepared.body!, /\r\n\r\nu_demo\r\n/);
    assert.deepEqual(prepared.missingVars, ['nope']);
  });

  it('produces the same bytes as the Rust implementation', () => {
    // 跨语言 golden：`mcp/http_ops/multipart_test.rs` 里的
    // the_body_matches_the_typescript_implementation_byte_for_byte 断言同一份期望值。
    //
    // 两套实现是各自写一遍的规则，而它们的产物一旦不同，两侧会各自信任自己那份、
    // 服务端只认其中一个。同一份 golden 被两侧断言，任何一边改动都会红，
    // 不必靠人记得同步。
    const B = '----SimplePostFormBoundaryFIXED';
    const content = JSON.stringify([
      { name: 'title', value: '报告' },
      { name: 'file', value: 'binary-ish', filename: 'a.txt' },
      { name: 'note', value: '', contentType: 'text/plain' },
      // 分片名里的引号与反斜杠被转义，CR/LF 整个被剥掉——留着就是往头里注入一个新头。
      { name: 'we"ird\\name\r\ninjected', value: 'v' },
      { name: 'emptyFile', value: 'x', filename: 'b.bin' },
      // 显式 null 是缺省：分片留着，那两个字段没有值。Rust 侧 serde 的 `Option`
      // 是同一个语义——两侧对 null 的判断若不同，同一个 body 会发成两份。
      { name: 'nullFields', value: 'v', filename: null, contentType: null },
    ]);

    const expected =
      [
        `--${B}\r\nContent-Disposition: form-data; name="title"\r\n\r\n报告`,
        `--${B}\r\nContent-Disposition: form-data; name="file"; filename="a.txt"\r\nContent-Type: application/octet-stream\r\n\r\nbinary-ish`,
        `--${B}\r\nContent-Disposition: form-data; name="note"\r\nContent-Type: text/plain\r\n\r\n`,
        `--${B}\r\nContent-Disposition: form-data; name="we\\"ird\\\\nameinjected"\r\n\r\nv`,
        `--${B}\r\nContent-Disposition: form-data; name="emptyFile"; filename="b.bin"\r\nContent-Type: application/octet-stream\r\n\r\nx`,
        `--${B}\r\nContent-Disposition: form-data; name="nullFields"\r\n\r\nv`,
      ].join('\r\n') + `\r\n--${B}--\r\n`;

    assert.equal(buildMultipartBody(parseMultipartParts(content), B), expected);
  });
});
