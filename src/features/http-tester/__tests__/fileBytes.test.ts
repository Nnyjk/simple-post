/**
 * 文件读取：分块编码与「添加到请求」的进度。
 *
 * 重要的是**逐字节等价**：分块读出的 base64 必须与一次读完的结果完全相同，否则同一份文件
 * 会因为体积不同而发出不同的内容——而 base64 的 4 字符一组不能跨块拆开，正是这件事最容易
 * 悄悄出错的地方。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  decodedBase64ByteLength,
  readFileAsBase64,
  type ReadFileProgress,
} from '../model/fileBytes';
import { bytesToBase64 } from '@/lib/base64';

/** 一份可预测的字节。 */
function bytes(length: number): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(length);
  for (let i = 0; i < length; i++) out[i] = (i * 37 + 11) % 256;
  return out;
}

function fileOf(length: number): File {
  return new File([bytes(length)], 'sample.bin');
}

/** 一次读完的基准答案。 */
function oneShot(size: number): string {
  return bytesToBase64(bytes(size));
}

describe('readFileAsBase64', () => {
  it('分块读与一次读完给出同一份 base64（含不足 3 字节的尾巴）', async () => {
    // 3 的倍数与非倍数都覆盖：跨块补位就是在这条边界上出错的。
    for (const size of [0, 1, 2, 3, 4, 5, 7, 64, 1000, 192 * 1024 + 1]) {
      assert.equal(await readFileAsBase64(fileOf(size)), oneShot(size), `size=${size}`);
    }
  });

  it('块大小不是 3 的倍数时也对', async () => {
    for (const chunkSize of [1, 2, 4, 5, 7, 1000]) {
      assert.equal(
        await readFileAsBase64(fileOf(1031), { chunkSize }),
        oneShot(1031),
        `chunkSize=${chunkSize}`,
      );
    }
  });

  it('进度从 0 起、单调不减，最后落在总字节数上', async () => {
    const seen: ReadFileProgress[] = [];
    await readFileAsBase64(fileOf(1000), { chunkSize: 100, onProgress: (p) => seen.push(p) });

    assert.deepEqual(seen[0], { loaded: 0, total: 1000 });
    for (let i = 1; i < seen.length; i++) {
      assert.ok(seen[i]!.loaded >= seen[i - 1]!.loaded, '进度不能回退');
      assert.equal(seen[i]!.total, 1000);
    }
    assert.equal(seen.at(-1)!.loaded, 1000);
  });

  it('空文件也报一次进度', async () => {
    const seen: ReadFileProgress[] = [];
    assert.equal(await readFileAsBase64(fileOf(0), { onProgress: (p) => seen.push(p) }), '');
    assert.deepEqual(seen, [{ loaded: 0, total: 0 }]);
  });

  it('已中止的信号在开始前就抛 AbortError', async () => {
    const ctrl = new AbortController();
    ctrl.abort();
    await assert.rejects(
      readFileAsBase64(fileOf(10), { signal: ctrl.signal }),
      (err: Error) => err.name === 'AbortError',
    );
  });

  it('读到一半被中止时不返回半份内容', async () => {
    const ctrl = new AbortController();
    await assert.rejects(
      readFileAsBase64(fileOf(1000), {
        chunkSize: 100,
        signal: ctrl.signal,
        onProgress: ({ loaded }) => {
          if (loaded >= 300) ctrl.abort();
        },
      }),
      (err: Error) => err.name === 'AbortError',
    );
  });
});

describe('decodedBase64ByteLength', () => {
  it('与真实解码长度一致', () => {
    for (const size of [0, 1, 2, 3, 4, 255, 1000]) {
      const encoded = bytesToBase64(bytes(size));
      assert.equal(decodedBase64ByteLength(encoded), size, `size=${size}`);
    }
  });

  it('空串是 0', () => {
    assert.equal(decodedBase64ByteLength(''), 0);
  });
});
