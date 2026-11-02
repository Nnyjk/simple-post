/**
 * 响应按内容类型渲染的判据 —— `src/features/response-viewer/responseKind.ts`。
 *
 * 这些规则是**看不见**的：判据写错时页面照样渲染，只是把一段正文画成乱码、把一张图片
 * 画成一串 base64、把一份 XML 当成二进制给出下载卡片。故这里把每一条判据都钉住。
 *
 * 后端那侧同样有这些规则（`crates/simple-post-core/src/http_executor.rs` 的
 * `split_response_payload` 与 `is_textual_content_type`），那边的形状由
 * `crates/simple-post-core/src/http_executor_test.rs` 钉住。两边必须给出同一个答案：
 * 后端说「这是二进制」，界面就下载；后端说「这是文本」，界面就按文本画。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  classifyResponse,
  responseBytes,
  responseDataUrl,
  responseFileName,
} from '../responseKind';
import type { ApiResponse } from '../../../types/domain';

/** 造一份响应。只写与判据有关的那几个字段。 */
function makeResponse(overrides: Partial<ApiResponse> = {}): ApiResponse {
  return {
    status: 200,
    statusText: 'OK',
    durationMs: 12,
    sizeBytes: 2,
    headers: {},
    body: '',
    contentType: '',
    ...overrides,
  };
}

describe('classifyResponse 按内容类型决定怎么渲染', () => {
  it('JSON 走 JSON 视图', () => {
    assert.equal(
      classifyResponse(makeResponse({ contentType: 'application/json', body: '{}' })),
      'json',
    );
    // `+json` 后缀也是 JSON。
    assert.equal(
      classifyResponse(makeResponse({ contentType: 'application/problem+json', body: '{}' })),
      'json',
    );
  });

  it('其它文本走文本视图', () => {
    assert.equal(classifyResponse(makeResponse({ contentType: 'text/plain', body: 'hi' })), 'text');
    assert.equal(classifyResponse(makeResponse({ contentType: 'text/html', body: '<p>' })), 'text');
    // 没有 Content-Type 时按文本看：后端给了文本形态就是这样一份响应。
    assert.equal(classifyResponse(makeResponse({ body: 'hi' })), 'text');
  });

  it('图片按 Content-Type 判，与字节无关', () => {
    // PNG 走 base64 那一份。
    assert.equal(
      classifyResponse(makeResponse({ contentType: 'image/png', bodyBase64: 'iVBORw0K' })),
      'image',
    );
    // SVG 是文本（后端把它放进文本形态），但它仍然是一张图。
    assert.equal(
      classifyResponse(makeResponse({ contentType: 'image/svg+xml', body: '<svg/>' })),
      'image',
    );
  });

  it('二进制走下载', () => {
    assert.equal(
      classifyResponse(
        makeResponse({ contentType: 'application/octet-stream', bodyBase64: 'AAEC' }),
      ),
      'binary',
    );
    assert.equal(
      classifyResponse(makeResponse({ contentType: 'application/pdf', bodyBase64: 'JVBE' })),
      'binary',
    );
  });

  it('后端给了 base64 就是二进制，哪怕 Content-Type 说自己是文本', () => {
    // 文本形态的后端不给 base64；给了就说明它判的是二进制。界面以那份为准，
    // 否则用户会看到一串 base64 文本而不是一个下载按钮。
    assert.equal(
      classifyResponse(makeResponse({ contentType: 'text/plain', bodyBase64: 'AAEC' })),
      'binary',
    );
  });
});

describe('responseFileName 取下载用的文件名', () => {
  it('Content-Disposition 里的 filename* 优先', () => {
    // RFC 5987 的 `filename*` 能带非 ASCII 名字，而 `filename="…"` 按规范只能是 ASCII。
    const response = makeResponse({
      contentType: 'application/octet-stream',
      headers: {
        'content-disposition':
          'attachment; filename="fallback.bin"; filename*=UTF-8\'\'%E6%8A%A5%E5%91%8A.pdf',
      },
    });
    assert.equal(responseFileName(response), '报告.pdf');
  });

  it('没有 filename* 时用 filename', () => {
    const response = makeResponse({
      contentType: 'application/octet-stream',
      headers: { 'content-disposition': 'attachment; filename="report.bin"' },
    });
    assert.equal(responseFileName(response), 'report.bin');
  });

  it('两者都没有时按 Content-Type 猜扩展名', () => {
    assert.equal(
      responseFileName(makeResponse({ contentType: 'application/json' })),
      'response.json',
    );
    assert.equal(responseFileName(makeResponse({ contentType: 'image/png' })), 'response.png');
    assert.equal(
      responseFileName(makeResponse({ contentType: 'application/x-thing' })),
      'response.bin',
    );
    assert.equal(responseFileName(makeResponse({})), 'response.bin');
  });
});

describe('responseBytes 给的是原始字节', () => {
  it('base64 形态解出原始字节', () => {
    // 0x00 0x01 0xFF —— 含非法 UTF-8 的字节，正是二进制该有的样子。
    const bytes = responseBytes(makeResponse({ bodyBase64: 'AAH/' }));
    assert.deepEqual(Array.from(bytes), [0x00, 0x01, 0xff]);
  });

  it('文本形态给的是 UTF-8 字节', () => {
    const bytes = responseBytes(makeResponse({ body: '你好' }));
    assert.deepEqual(Array.from(bytes), [0xe4, 0xbd, 0xa0, 0xe5, 0xa5, 0xbd]);
  });
});

describe('responseDataUrl 给图片预览用的 data URL', () => {
  it('图片给 data URL', () => {
    const url = responseDataUrl(makeResponse({ contentType: 'image/png', bodyBase64: 'AAH/' }));
    assert.equal(url, 'data:image/png;base64,AAH/');
  });

  it('非图片给 null', () => {
    assert.equal(responseDataUrl(makeResponse({ contentType: 'text/plain', body: 'hi' })), null);
  });
});
