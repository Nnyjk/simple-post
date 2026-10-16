/**
 * 保存失败的人话化。
 *
 * 只有一条分支值得钉：门禁拒绝与磁盘故障在插件里长得很像，而两者的用户动作完全相反
 *（换个目录 vs 看看磁盘）。若这里的匹配放宽到所有错误，真正的 IO 故障也会被劝去
 * 「换个目录试试」，于是无从排查。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { humanizeSaveError } from '@/lib/transfer-file-save';

describe('humanizeSaveError', () => {
  it('门禁拒绝换成人话，且指向用户真正能选的位置', () => {
    const raw = 'path not allowed by scope';
    const out = humanizeSaveError(new Error(raw));
    assert.notEqual(out.message, raw, '必须换掉原始措辞');
    assert.match(out.message, /文档|下载|桌面/);
  });

  it('同一族措辞都命中：permission / forbidden / scope', () => {
    for (const raw of [
      'permission denied',
      'FORBIDDEN: path outside allowed scope',
      'not allowed to write',
    ]) {
      assert.match(humanizeSaveError(new Error(raw)).message, /不允许写入/, raw);
    }
  });

  it('普通 IO 故障原样放行——换成人话会把用户指向错误的排查方向', () => {
    const raw = 'ENOSPC: no space left on device';
    assert.equal(humanizeSaveError(new Error(raw)).message, raw);
  });

  it('非 Error 的抛出物也变成 Error，而不是把值原样抛出去', () => {
    const out = humanizeSaveError('disk gone');
    assert.ok(out instanceof Error);
    assert.equal(out.message, 'disk gone');
  });

  it('已经在 `try` 外面被 catch 成字符串的门禁错误同样能被识别', () => {
    // Tauri 的插件错误经 IPC 序列化后可能是字符串而不是 Error 实例。
    assert.match(humanizeSaveError('Path scope not allowed').message, /不允许写入/);
  });
});
