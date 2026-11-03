/**
 * `crud-slice` 的 `moveNodes`：乐观移动与它发出的请求。
 *
 * 两件事被这里钉住：
 *
 *   1. **守卫拦住的那几种移动，一次都不该发生。** 守卫与后端的 `db::nodes::move_many`
 *      同一套规则（父子类型矩阵、项目边界、成环）。被守卫拦下时既不改本地状态，也不发
 *      请求 —— 发了只会被后端按同一套规则拒绝，而本地已经把状态画上去了。
 *   2. **槽位按「去掉被移动者之后」的同级序列算。** 插入动作发生在那一份序列上（被移动者
 *      要先从原位摘掉），故槽位也得按那一份算，否则往下拖时会多落一格。
 *
 * 每条守卫用例都挑了**只有那一条守卫拦得住**的组合 —— 否则去掉其中一条，另外一条
 * 照样会拦，用例就分不清是谁拦的。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { installIpcMock } from './_browser-shim';
import { makeCrudHarness } from './crud-slice-harness';
import type { ResourceNode } from '@/types/domain';

function node(
  id: string,
  kind: ResourceNode['kind'],
  parentId: string | null,
  sortOrder: number,
  rootId = 'p1',
): ResourceNode {
  return { id, rootId, parentId, kind, name: id, sortOrder, createdAt: 0, updatedAt: 0 };
}

/**
 * 一棵树：
 *
 *   m1  m2                     两个模块，项目 p1 的顶层
 *   └─ m1
 *      ├─ c1                  集合
 *      │  ├─ c2               集合，c1 之下
 *      │  ├─ e1  e2           两个接口，c1 之下
 *      ├─ e0                  接口，m1 之下
 *      └─ c3                  集合，m1 之下
 *         └─ e3 e4 e5         三个接口，c3 之下
 *
 *   x1                          另一个项目 p2 的接口
 *
 * `c3` 底下只有接口 —— 「在容器内重排接口」那组用例的同级序列里没有集合，
 * 序号算得清。
 */
function makeNodes(): ResourceNode[] {
  return [
    node('m1', 'module', null, 0),
    node('m2', 'module', null, 1),
    node('c1', 'collection', 'm1', 0),
    node('c2', 'collection', 'c1', 0),
    node('e1', 'endpoint', 'c1', 1),
    node('e2', 'endpoint', 'c1', 2),
    node('e0', 'endpoint', 'm1', 1),
    node('c3', 'collection', 'm1', 2),
    node('e3', 'endpoint', 'c3', 0),
    node('e4', 'endpoint', 'c3', 1),
    node('e5', 'endpoint', 'c3', 2),
    node('x1', 'endpoint', null, 0, 'p2'),
  ];
}

/** 记录移动动作发出的每一次 IPC 调用。 */
function recordMoves() {
  const calls: { cmd: string; args: any }[] = [];
  installIpcMock(async (cmd, args) => {
    calls.push({ cmd, args });
    return undefined;
  });
  return calls;
}

function byId(harness: { bag: { nodes: ResourceNode[] } }, id: string) {
  return harness.bag.nodes.find((n) => n.id === id)!;
}

describe('crud-slice: moveNodes 的守卫', () => {
  it('合法移动改本地状态并发出 nodes_move', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    // 接口挂到模块下 —— 类型矩阵允许。
    await harness.slice.moveNodes(['e1'], 'm1', 0);

    assert.equal(byId(harness, 'e1').parentId, 'm1', 'e1 的父级应当变成 m1');
    assert.equal(byId(harness, 'e1').sortOrder, 0, 'e1 插在 m1 的子列表第 0 位');
    assert.equal(byId(harness, 'c1').sortOrder, 1, 'c1 被挤到后面一位');
    assert.equal(calls.length, 1, '应当发出一次 nodes_move');
    assert.match(calls[0].cmd, /move/, '发出的命令应当是移动');
  });

  it('模块移到集合下被拦下 —— 模块没有节点父级', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    // 用 m2 而不是 m1：m1 是 c1 的祖先，成环守卫会先把它挡下，那样这条用例就
    // 分不清是谁拦的。m2 与 c1 无关，只有类型矩阵拦得住它。
    await harness.slice.moveNodes(['m2'], 'c1', 0);

    assert.equal(byId(harness, 'm2').parentId, null, 'm2 的父级不能变成集合');
    assert.equal(calls.length, 0, '被守卫拦下的移动不该发请求');
  });

  it('集合移到项目顶层被拦下 —— 那里只放得下模块', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    await harness.slice.moveNodes(['c1'], null, 0);

    assert.equal(byId(harness, 'c1').parentId, 'm1', 'c1 的父级不能变成项目顶层');
    assert.equal(calls.length, 0, '被守卫拦下的移动不该发请求');
  });

  it('跨项目移动被拦下', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    await harness.slice.moveNodes(['x1'], 'm1', 0);

    assert.equal(byId(harness, 'x1').parentId, null, '别的项目的节点不能被拖进本项目');
    assert.equal(calls.length, 0, '被守卫拦下的移动不该发请求');
  });

  it('把节点拖进自己的后代被拦下 —— 那会绕成环', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    // c1 之下有 c2。把 c1 拖到 c2 里就是把自己嵌进自己的后代。用集合套集合而不是
    // 集合套接口：集合挂集合类型矩阵是允许的，故这里只有成环守卫拦得住它。
    await harness.slice.moveNodes(['c1'], 'c2', 0);

    assert.equal(byId(harness, 'c1').parentId, 'm1', 'c1 不能被拖进自己的后代');
    assert.equal(calls.length, 0, '被守卫拦下的移动不该发请求');
  });

  it('请求的 id 有一部分不在树里时整次移动都不发生', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    // ghost 不在树里。匹配到的那一个（e1）类型上是合法的（接口挂模块下允许），
    // 故只有「请求的 id 必须全都在树里」这条守卫拦得住它。
    await harness.slice.moveNodes(['e1', 'ghost'], 'm1', 0);

    assert.equal(byId(harness, 'e1').parentId, 'c1', 'e1 不该被移动 —— 请求里有不认识的 id');
    assert.equal(calls.length, 0, '被守卫拦下的移动不该发请求');
  });

  it('请求的 id 全都不在树里时这次移动不发生', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    await harness.slice.moveNodes(['ghost'], 'm1', 0);

    assert.equal(calls.length, 0, '被守卫拦下的移动不该发请求');
  });
});

describe('crud-slice: moveNodes 的槽位', () => {
  it('往下拖时槽位按去掉被移动者之后的序列算', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    // e3 拖到 e4 下面。c3 的子节点是 e3、e4、e5；去掉 e3 之后是 [e4, e5]，
    // e4 在那一份序列里的下标是 0，「在它下面」就是 1。
    await harness.slice.moveNodes(['e3'], 'c3', 1);

    assert.equal(calls[0].args.index, 1, '槽位应当是 1，而不是 2');
    assert.equal(byId(harness, 'e3').sortOrder, 1, 'e3 应当排在 e4 之后');
    assert.equal(byId(harness, 'e4').sortOrder, 0, 'e4 应当排在前面');
  });

  it('模块重排的父级是项目顶层', async () => {
    const harness = makeCrudHarness({}, [], makeNodes());
    const calls = recordMoves();

    // m1 拖到 m2 下面。项目顶层的同级序列去掉 m1 之后是 [m2]，
    // m2 的下标是 0，「在它下面」就是 1。
    await harness.slice.moveNodes(['m1'], null, 1);

    assert.equal(calls[0].args.parentId, null, '项目顶层的父级是 null');
    assert.equal(calls[0].args.index, 1, '槽位应当是 1');
    assert.equal(byId(harness, 'm1').sortOrder, 1, 'm1 应当排在 m2 之后');
  });
});
