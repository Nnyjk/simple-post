/**
 * 分片字节区间（`multipartPartByteRanges`）与真正发出去的 body 是否对得上。
 *
 * 进度条按这些区间把「已写出 N 字节」折算到某一行文件上。区间一旦偏了一格，进度就会画在
 * 别人的行上——而它在界面上看起来**完全正常**，只是标错了对象。故这里不看区间本身的数值，
 * 而是按区间去切真实 body，断言切出来的正是那个分片的内容。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { prepareRequest } from '../http-build';
import { base64ToBytes, bytesToBase64 } from '../base64';
import { prep, headerMap } from './http-build-fixtures';
import type { MultipartPart } from '@/types/domain';

const FILE_BYTES = new Uint8Array([0x00, 0xff, 0x10, 0x20, 0x7f, 0x80]);

function parts(list: MultipartPart[]): string {
  return JSON.stringify(list);
}

/** 跑一次 prepareRequest，把 body 的字节与声明的 boundary 一起取回来。 */
function prepared(list: MultipartPart[]) {
  const result = prepareRequest(
    prep({ method: 'POST', body: { mode: 'multipart', content: parts(list) } }),
  );
  const boundary = /boundary=(.+)$/.exec(headerMap(result.headers)['Content-Type'] ?? '')?.[1];
  assert.ok(boundary, 'multipart 的 Content-Type 必须带 boundary');
  // 字节是合法 UTF-8 时走文本形态，否则走 base64 形态（与 `AssembledBody` 一致）。
  const body =
    result.body !== null
      ? new TextEncoder().encode(result.body)
      : base64ToBytes(result.bodyBase64 ?? '');
  return { result, boundary, body };
}

describe('multipartPartByteRanges', () => {
  it('按区间切出来的正是该分片的内容', () => {
    const list: MultipartPart[] = [
      { name: 'field', value: 'v1', enabled: true },
      {
        name: 'file',
        value: '',
        filename: 'a.bin',
        base64: bytesToBase64(FILE_BYTES),
        enabled: true,
      },
      { name: 'tail', value: 'v2', enabled: true },
    ];
    const { result, body } = prepared(list);
    const ranges = result.multipartPartRanges;
    assert.ok(ranges);

    assert.deepEqual(
      ranges.map((r) => r.index),
      [0, 1, 2],
      '下标是原始分片数组里的下标',
    );
    const text = new TextDecoder();
    assert.deepEqual(
      ranges.map((r) => text.decode(body.subarray(r.start, r.end))),
      ['v1', text.decode(FILE_BYTES), 'v2'],
      '每一段切出来正好是分片内容，不含把框界算进去',
    );
  });

  it('停用的分片不出现，剩下的下标仍是原始下标', () => {
    const list: MultipartPart[] = [
      { name: 'a', value: 'A', enabled: false },
      { name: 'b', value: 'B', enabled: true },
      { name: 'c', value: 'C', enabled: false },
      { name: 'd', value: 'D', enabled: true },
    ];
    const { result, body } = prepared(list);
    const ranges = result.multipartPartRanges ?? [];

    assert.deepEqual(
      ranges.map((r) => r.index),
      [1, 3],
    );
    const text = new TextDecoder();
    assert.deepEqual(
      ranges.map((r) => text.decode(body.subarray(r.start, r.end))),
      ['B', 'D'],
    );
  });

  it('区间落在 body 之内、互不重叠，且按次序推进', () => {
    const list: MultipartPart[] = [
      { name: 'one', value: 'x'.repeat(50), enabled: true },
      {
        name: 'two',
        value: '',
        filename: 'b.bin',
        base64: bytesToBase64(FILE_BYTES),
        enabled: true,
      },
      { name: 'three', value: '', enabled: true },
    ];
    const { result, body } = prepared(list);
    const ranges = result.multipartPartRanges ?? [];

    let previousEnd = 0;
    for (const range of ranges) {
      assert.ok(range.start >= previousEnd, '区间不重叠且按 body 次序');
      assert.ok(range.end <= body.length, '区间不越出 body');
      assert.ok(range.end >= range.start);
      previousEnd = range.end;
    }
  });

  it('文本分片末尾的 CRLF 不算进区间', () => {
    const list: MultipartPart[] = [{ name: 'only', value: 'VALUE', enabled: true }];
    const { result, body } = prepared(list);
    const range = (result.multipartPartRanges ?? [])[0];
    assert.ok(range);
    // 切出来必须正好是 VALUE，而不是 "VALUE\r\n"。
    assert.equal(new TextDecoder().decode(body.subarray(range.start, range.end)), 'VALUE');
    assert.equal(new TextDecoder().decode(body.subarray(range.end, range.end + 2)), '\r\n');
  });

  it('非 multipart 的 body 没有区间', () => {
    const result = prepareRequest(
      prep({ method: 'POST', body: { mode: 'raw', content: '{"a":1}' } }),
    );
    assert.equal(result.multipartPartRanges, undefined);
  });
});
