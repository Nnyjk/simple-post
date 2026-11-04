/**
 * 落单代理项的编码 —— 渲染进程侧的那条规则。
 *
 * 代理项（UTF-16 的 D800–DFFF）成对时是一个增补平面字符，落单时在 UTF-8 里没有对应字节。
 * 平台的 `encodeURIComponent` 遇到落单的那一个直接抛 `URIError`；请求是在渲染期组装的
 * （`HttpTester` 的 `useMemo`），仓库里没有 React 错误边界，异常一路到 React 根节点，
 * 整棵工作区树被卸载。
 *
 * 这里钉住两件事：
 *
 *   - 百分号编码这个原语不抛异常，落单的代理项按 U+FFFD 编码（`%EF%BF%BD`）。
 *   - 组装出来的请求里没有一个字符串带落单代理项，故它能原样进到后端 —— 后端的
 *     serde_json 拒绝 JSON 里的落单代理项转义。
 *
 * 共享样本 `fixtures/body-parity.json` 装不下这个用例：那份样本是 JSON，JSON 里的
 * `\uD800` 转义 serde_json 直接拒绝，两侧的读取器读不了它。故这条规则由本文件与
 * `crates/simple-post-core/src/body_test.rs` 里那条用例分别钉住。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { percentEncode } from '../percent-encode';
import { wellFormed } from '../well-formed';
import { resolveBody } from '../body';
import { appendQueryString } from '../http-build/query';
import { substitutePathParams } from '../path-params';
import { prepareRequest } from '../http-build';
import { createMissingTracker } from '../http-build/pool';
import { kv, prep } from './http-build-fixtures';

/** 落单的高位代理项（D800）。 */
const LONE_HIGH = String.fromCharCode(0xd800);
/** 落单的低位代理项（DC00）。 */
const LONE_LOW = String.fromCharCode(0xdc00);
/** 配成一对的代理项：一个 emoji。 */
const PAIR = String.fromCodePoint(0x1f600);
/** 落单代理项的正则，与 `src/lib/well-formed.ts` 里那一条同形。 */
const LONE = /\p{Surrogate}/u;

describe('wellFormed', () => {
  it('把落单的高位代理项换成 U+FFFD', () => {
    assert.equal(wellFormed('a' + LONE_HIGH + 'b'), 'a\uFFFDb');
  });

  it('把落单的低位代理项换成 U+FFFD', () => {
    assert.equal(wellFormed('a' + LONE_LOW), 'a\uFFFD');
  });

  it('配成一对的代理项原样保留', () => {
    // 一对代理项是一个增补平面字符，它在 UTF-8 里有对应的字节，故不能被替换掉
    assert.equal(wellFormed(PAIR), PAIR);
  });

  it('普通文本不变', () => {
    assert.equal(wellFormed('a b & 中文'), 'a b & 中文');
  });
});

describe('percentEncode', () => {
  it('字符集与 encodeURIComponent 相同', () => {
    const samples = ['a b', "!~*'()", 'a&b+c=d', '中文', '/?#[]'];
    for (const sample of samples) {
      assert.equal(percentEncode(sample), encodeURIComponent(sample), sample);
    }
  });

  it('空格编码成 %20，不是 form 编码的 +', () => {
    assert.equal(percentEncode('a b'), 'a%20b');
  });

  it('遇到落单的代理项不抛异常，按 U+FFFD 编码', () => {
    assert.doesNotThrow(() => percentEncode(LONE_HIGH));
    assert.equal(percentEncode(LONE_HIGH), '%EF%BF%BD');
    assert.equal(percentEncode(LONE_LOW), '%EF%BF%BD');
  });

  it('配成对的代理项按 UTF-8 字节编码', () => {
    assert.equal(percentEncode(PAIR), '%F0%9F%98%80');
  });
});

describe('请求体：form 模式的字段值带落单代理项', () => {
  it('组装正文时不抛异常', () => {
    const content = JSON.stringify([{ key: 'q', value: LONE_HIGH, enabled: true }]);
    assert.doesNotThrow(() => resolveBody({ mode: 'form', content }, {}));
  });

  it('落单的代理项按 U+FFFD 编码进正文', () => {
    const content = JSON.stringify([{ key: 'q', value: LONE_HIGH, enabled: true }]);
    assert.equal(resolveBody({ mode: 'form', content }, {}).text, 'q=%EF%BF%BD');
  });
});

describe('查询串与路径参数带落单代理项', () => {
  it('查询参数的值不抛异常，按 U+FFFD 编码', () => {
    const missing = createMissingTracker();
    let url = '';
    assert.doesNotThrow(() => {
      url = appendQueryString('/x', [kv('q', LONE_HIGH)], {}, missing);
    });
    assert.equal(url, '/x?q=%EF%BF%BD');
  });

  it('路径参数的值不抛异常，按 U+FFFD 编码', () => {
    let url = '';
    assert.doesNotThrow(() => {
      url = substitutePathParams('/u/{id}', [['id', LONE_HIGH]]).url;
    });
    assert.equal(url, '/u/%EF%BF%BD');
  });
});

describe('组装请求：每一个字符串都是良构的', () => {
  it('form 体的字段值带落单代理项时不抛异常', () => {
    const content = JSON.stringify([{ key: 'q', value: LONE_HIGH, enabled: true }]);
    assert.doesNotThrow(() => prepareRequest(prep({ body: { mode: 'form', content } })));
  });

  it('请求的 URL、header、鉴权与正文里都不带落单代理项', () => {
    const prepared = prepareRequest(
      prep({
        urlPath: '/u/' + LONE_HIGH,
        params: [kv('q', LONE_HIGH)],
        headers: [kv('X-Trace', LONE_HIGH)],
        body: { mode: 'json', content: '{"u":"' + LONE_HIGH + '"}' },
        auth: { kind: 'bearer', bearer: LONE_HIGH },
      }),
    );
    const texts = [prepared.url, ...prepared.headers.flat(), prepared.body ?? ''];
    for (const text of texts) {
      assert.doesNotMatch(text, LONE, '落单的代理项进了请求：' + JSON.stringify(text));
    }
  });

  it('json 体的落单代理项在组装后是 U+FFFD', () => {
    const prepared = prepareRequest(
      prep({ body: { mode: 'json', content: '{"u":"' + LONE_HIGH + '"}' } }),
    );
    assert.equal(prepared.body, '{"u":"' + '\uFFFD"}');
  });
});
