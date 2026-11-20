/**
 * `copyText` 同步回退路径的单测。
 *
 * 本 runner 跑在 Node 里，没有 DOM 也没有剪贴板，故手写一个只实现所需最小面的假
 * `Document`：`createElement` / `body.appendChild` / `body.removeChild` / `execCommand` /
 * `activeElement`。测的是行为（走哪条分支、返回值、清理与焦点），不是源码文本。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { copyText } from '../clipboard';

interface FakeState {
  appended: number;
  removed: number;
  execCalls: number;
  focused: number;
}

interface FakeOptions {
  /** `execCommand('copy')` 的返回值。 */
  execResult?: boolean;
  /** 提供它就模拟「异步剪贴板可用」的宿主。 */
  clipboard?: { writeText: (text: string) => Promise<void> };
}

/** 假 Document：`defaultView.navigator` 显式给出，回退分支不依赖 Node 有没有全局 navigator。 */
function fakeDoc(options: FakeOptions = {}): { doc: Document; state: FakeState } {
  const state: FakeState = { appended: 0, removed: 0, execCalls: 0, focused: 0 };
  const doc = {
    activeElement: {
      focus() {
        state.focused += 1;
      },
    },
    body: {
      appendChild(node: unknown) {
        state.appended += 1;
        return node;
      },
      removeChild(node: unknown) {
        state.removed += 1;
        return node;
      },
    },
    defaultView: { navigator: { clipboard: options.clipboard } },
    createElement(tag: string) {
      assert.equal(tag, 'textarea');
      return {
        value: '',
        readOnly: false,
        style: {} as Record<string, string>,
        select() {},
      };
    },
    execCommand(command: string) {
      assert.equal(command, 'copy');
      state.execCalls += 1;
      return options.execResult ?? false;
    },
  };
  return { doc: doc as unknown as Document, state };
}

describe('copyText', () => {
  it('execCommand 返回 false 时返回 false，且 textarea 已被移除', async () => {
    const { doc, state } = fakeDoc({ execResult: false });
    assert.equal(await copyText('hello', doc), false);
    assert.equal(state.execCalls, 1);
    assert.equal(state.appended, 1);
    assert.equal(state.removed, 1);
  });

  it('execCommand 返回 true 时返回 true，并把焦点还给调用前的元素', async () => {
    const { doc, state } = fakeDoc({ execResult: true });
    assert.equal(await copyText('hello', doc), true);
    assert.equal(state.execCalls, 1);
    assert.equal(state.removed, 1);
    assert.equal(state.focused, 1);
  });

  it('navigator.clipboard 不存在时确实走 execCommand 分支', async () => {
    const { doc, state } = fakeDoc({ execResult: true });
    await copyText('hello', doc);
    assert.equal(state.execCalls, 1);
    assert.equal(state.appended, 1);
  });

  it('异步剪贴板可用时直接返回 true，不碰 execCommand', async () => {
    const written: string[] = [];
    const { doc, state } = fakeDoc({
      clipboard: {
        writeText: async (text: string) => {
          written.push(text);
        },
      },
    });
    assert.equal(await copyText('hello', doc), true);
    assert.deepEqual(written, ['hello']);
    assert.equal(state.execCalls, 0);
    assert.equal(state.appended, 0);
  });

  it('异步剪贴板被拒时回退到 execCommand，不抛给调用方', async () => {
    const { doc, state } = fakeDoc({
      execResult: true,
      clipboard: {
        writeText: async () => {
          throw new Error('NotAllowedError');
        },
      },
    });
    assert.equal(await copyText('hello', doc), true);
    assert.equal(state.execCalls, 1);
  });
});
