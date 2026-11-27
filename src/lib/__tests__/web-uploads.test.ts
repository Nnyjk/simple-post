/** 浏览器上传的暂存与请求体：文件怎么配对、按什么顺序上去、信封在哪一格。 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { uploadFiles, uploadForm, rememberUpload } from '../web-uploads';

function file(name: string, bytes: string): File {
  return new File([bytes], name, { type: 'application/octet-stream' });
}

function parts(list: unknown[]): string {
  return JSON.stringify(list);
}

describe('uploadFiles', () => {
  it('按分片顺序取出文件；有路径的分片不算（字节由执行层读盘）', () => {
    const first = file('a.txt', 'AAAAA');
    const second = file('b.bin', 'BBBB');
    rememberUpload(first);
    rememberUpload(second);

    const body = parts([
      { name: 'note', value: 'hello', enabled: true },
      { name: 'one', value: '', enabled: true, filename: 'a.txt', size: 5 },
      { name: 'disk', value: '', enabled: true, filename: 'c.txt', path: 'C:\\tmp\\c.txt' },
      { name: 'two', value: '', enabled: true, filename: 'b.bin', size: 4 },
      { name: 'off', value: '', enabled: false, filename: 'a.txt', size: 5 },
    ]);

    assert.deepEqual(uploadFiles('multipart', body), [first, second]);
  });

  it('没暂存过字节就说清是哪个文件，而不是发一个空文件', () => {
    const body = parts([{ name: 'x', value: '', enabled: true, filename: 'gone.bin', size: 3 }]);
    assert.throws(() => uploadFiles('multipart', body), /gone\.bin/);
  });

  it('binary 模式：没有路径、字节数非零才上传', () => {
    const blob = file('blob.bin', 'DATA!');
    rememberUpload(blob);

    const described = JSON.stringify({ name: 'blob.bin', size: 5, contentType: '', path: '' });
    assert.deepEqual(uploadFiles('binary', described), [blob]);

    const onDisk = JSON.stringify({ name: 'blob.bin', size: 5, contentType: '', path: '/tmp/a' });
    assert.deepEqual(uploadFiles('binary', onDisk), []);
  });

  it('文本模式没有文件', () => {
    assert.deepEqual(uploadFiles('json', '{"a":1}'), []);
    assert.deepEqual(uploadFiles('multipart', null), []);
  });
});

describe('uploadForm', () => {
  it('第一个分片是 request 信封，其余按顺序是文件', async () => {
    const envelope = { method: 'POST', url: 'http://x/y' };
    const form = uploadForm(envelope, [file('a.txt', 'AAAAA'), file('b.bin', 'BBBB')]);

    const request = form.get('request');
    assert.ok(request instanceof Blob);
    assert.equal(await request.text(), JSON.stringify(envelope));

    assert.deepEqual(
      [...form.keys()],
      ['request', 'a.txt', 'b.bin'],
      '服务端按顺序认文件，名字只用于抓包时对照',
    );
  });
});
