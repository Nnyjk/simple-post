/**
 * 覆盖值编辑器的开合判据。
 *
 * 两条入口（「编辑」按钮与「当前值」那行摘要）共用这一个判定：列表里的「当前值」还没换成当前
 * 环境的时候开编辑器，框里就是上一个环境的文本，保存却写进这一格。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { overrideEditor } from '../override-editor';
import { dynamicValueScope } from '@/stores/dynamic-slice';

const scopeOf = (environmentId: string) => dynamicValueScope('p1', environmentId);

describe('overrideEditor', () => {
  it('列表戳与当前环境一致：可编辑，初值是列表给出的那个值', () => {
    const editor = overrideEditor(scopeOf('env1'), 'p1', 'env1', '[a]');

    assert.equal(editor.editable, true);
    assert.equal(editor.initialValue, '[a]');
  });

  it('列表戳是上一个环境的：不接受编辑（框里若是那个值，保存就写进另一格）', () => {
    const editor = overrideEditor(scopeOf('env1'), 'p1', 'env2', '[a]');

    assert.equal(editor.editable, false);
  });

  it('列表还没拉过（戳为 null）：也不接受编辑', () => {
    assert.equal(overrideEditor(null, 'p1', 'env1', '[a]').editable, false);
  });

  it('没有值、也没有环境时，初值是空串而不是 null', () => {
    const editor = overrideEditor(dynamicValueScope('p1', null), 'p1', null, null);

    assert.equal(editor.editable, true);
    assert.equal(editor.initialValue, '');
  });
});
