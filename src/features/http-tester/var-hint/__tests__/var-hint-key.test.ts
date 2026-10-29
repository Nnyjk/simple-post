/**
 * 变量提示的按键规则。
 *
 * 规则只有一份（`varHint.ts::varHintKeyAction`），两条路径各调它：普通输入框走
 * `useVarHint`，CodeMirror 编辑器走 `BodyEditor` 自己的处理器。
 *
 * 这里钉的是**该拦的拦、该放的放**：拦错了会吞掉用户的中文输入与 Tab，
 * 放错了会插进一个不该插的变量。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { varHintKeyAction, type VarHintKeyEvent } from '../varHint';

/** 一次按键，缺省那几项都按「普通按键」填。 */
function key(overrides: Partial<VarHintKeyEvent> = {}): VarHintKeyEvent {
  return {
    key: 'Enter',
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    isComposing: false,
    keyCode: 0,
    ...overrides,
  };
}

describe('变量提示：按键该做什么', () => {
  it('浮层没开时按键不归提示管', () => {
    assert.equal(varHintKeyAction(key(), { open: false, hasEntry: true }), 'none');
    assert.equal(
      varHintKeyAction(key({ key: 'ArrowDown' }), { open: false, hasEntry: true }),
      'none',
    );
  });

  it('输入法组合期间的 Enter 与方向键留给输入法', () => {
    // 组合期间的 Enter 是「确认候选」，方向键是「选候选」。提示抢走它们，用户的中文
    // 就插不进变量名里。
    for (const k of ['Enter', 'ArrowDown', 'ArrowUp', 'Tab']) {
      assert.equal(
        varHintKeyAction(key({ key: k, isComposing: true }), { open: true, hasEntry: true }),
        'none',
        `${k} 在输入法组合期间不能被提示接管`,
      );
    }
  });

  it('老浏览器上 isComposing 取不到时靠 keyCode 229 兜底', () => {
    assert.equal(
      varHintKeyAction(key({ key: 'Enter', keyCode: 229 }), { open: true, hasEntry: true }),
      'none',
    );
  });

  it('带修饰键的组合留给全局快捷键', () => {
    // Ctrl+Enter 是发送，Ctrl+S 是保存。提示抢走它们，用户就没法发请求了。
    assert.equal(
      varHintKeyAction(key({ key: 'Enter', ctrlKey: true }), { open: true, hasEntry: true }),
      'none',
    );
    assert.equal(
      varHintKeyAction(key({ key: 'Enter', metaKey: true }), { open: true, hasEntry: true }),
      'none',
    );
    assert.equal(
      varHintKeyAction(key({ key: 'Enter', altKey: true }), { open: true, hasEntry: true }),
      'none',
    );
  });

  it('方向键换高亮', () => {
    assert.equal(
      varHintKeyAction(key({ key: 'ArrowDown' }), { open: true, hasEntry: true }),
      'next',
    );
    assert.equal(varHintKeyAction(key({ key: 'ArrowUp' }), { open: true, hasEntry: true }), 'prev');
  });

  it('有候选时 Enter 与 Tab 插入变量', () => {
    assert.equal(varHintKeyAction(key({ key: 'Enter' }), { open: true, hasEntry: true }), 'insert');
    assert.equal(varHintKeyAction(key({ key: 'Tab' }), { open: true, hasEntry: true }), 'insert');
  });

  it('没有候选时不拦 Enter 与 Tab', () => {
    // 拦下来键盘用户就出不去了：既插不进变量，也走不出这一栏。
    assert.equal(varHintKeyAction(key({ key: 'Tab' }), { open: true, hasEntry: false }), 'none');
    assert.equal(varHintKeyAction(key({ key: 'Enter' }), { open: true, hasEntry: false }), 'none');
  });

  it('Esc 关掉浮层', () => {
    assert.equal(varHintKeyAction(key({ key: 'Escape' }), { open: true, hasEntry: true }), 'close');
  });

  it('不相关的键不归提示管', () => {
    assert.equal(varHintKeyAction(key({ key: 'a' }), { open: true, hasEntry: true }), 'none');
    assert.equal(
      varHintKeyAction(key({ key: 'ArrowLeft' }), { open: true, hasEntry: true }),
      'none',
    );
  });
});
