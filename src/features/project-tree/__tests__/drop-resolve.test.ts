/**
 * 树拖放的落点规则 —— `src/features/project-tree/drop-resolve.ts` 的单元测试。
 *
 * 这些规则决定「这一次拖放落在哪里」，且规则全部来自父子类型矩阵。矩阵本身由
 * `fixtures/placement-parity.json` 钉住（Rust 与 TS 两侧各一个读取器）；这一份测的是
 * 「矩阵怎么变成落点」。
 *
 * 运行：
 *   npx tsx --test src/features/project-tree/__tests__/drop-resolve.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { bandsFor, resolveDrop, zoneFor, zoneFromRowPosition } from '../drop-resolve';
import type { DropBands, DropTarget } from '../drop-resolve';
import type { ResourceNode, ResourceNodeType } from '@/types/domain';

// ---------- fixtures ----------

/** 高 100px、位于视口下方 200px 起的一行。 */
const ROW = { top: 200, height: 100 };

/** `ROW` 内给定纵向比例时的光标 Y。 */
function yAt(ratio: number): number {
  return ROW.top + ratio * ROW.height;
}

function node(
  id: string,
  kind: ResourceNodeType,
  parentId: string | null,
  sortOrder: number,
  rootId = 'p1',
): ResourceNode {
  return { id, rootId, parentId, kind, name: id, sortOrder, createdAt: 0, updatedAt: 0 };
}

/**
 * 一棵树：
 *
 *   m1  m2                       两个模块，项目顶层
 *   └─ m1
 *      ├─ c1                     集合，m1 之下
 *      │  ├─ c2                  集合，c1 之下
 *      │  └─ e1 e2 e3            接口，c1 之下，序号 1/2/3
 *      ├─ e0                     接口，m1 之下
 *      └─ c3                     集合，m1 之下
 *         └─ f1 f2 f3            接口，c3 之下，序号 0/1/2
 *
 * `c3` 底下只有接口 —— 「在容器内重排接口」那组用例的同级序列里没有集合，序号算得清。
 */
const NODES: ResourceNode[] = [
  node('m1', 'module', null, 0),
  node('m2', 'module', null, 1),
  node('c1', 'collection', 'm1', 0),
  node('c2', 'collection', 'c1', 0),
  node('e1', 'endpoint', 'c1', 1),
  node('e2', 'endpoint', 'c1', 2),
  node('e3', 'endpoint', 'c1', 3),
  node('e0', 'endpoint', 'm1', 1),
  node('c3', 'collection', 'm1', 2),
  node('f1', 'endpoint', 'c3', 0),
  node('f2', 'endpoint', 'c3', 1),
  node('f3', 'endpoint', 'c3', 2),
];

function target(id: string): DropTarget {
  const n = NODES.find((x) => x.id === id)!;
  return { kind: n.kind, id: n.id };
}

function bandsOf(dragKind: ResourceNodeType, targetId: string): DropBands {
  const t = target(targetId);
  const row = NODES.find((n) => n.id === targetId)!;
  return bandsFor(dragKind, t, row, NODES);
}

function drop(dragKind: ResourceNodeType, ids: string[], targetId: string, ratio: number) {
  return resolveDrop({
    drag: { kind: dragKind, ids },
    target: target(targetId),
    rect: ROW,
    clientY: yAt(ratio),
    nodes: NODES,
    activeProjectId: 'p1',
  });
}

// ============================================================
// zoneFromRowPosition
// ============================================================

describe('落点分档', () => {
  const both: DropBands = { into: true, beside: true };

  it('两个区都开放时按 25/50/25 分档', () => {
    assert.equal(zoneFromRowPosition(ROW, yAt(0), both), 'above');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.27), both), 'above');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.28), both), 'into');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.5), both), 'into');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.72), both), 'into');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.73), both), 'below');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.99), both), 'below');
  });

  it('三个区都可达 —— 答案恒定就说明有一个区是死代码', () => {
    const zones = new Set([0.05, 0.5, 0.95].map((r) => zoneFromRowPosition(ROW, yAt(r), both)));
    assert.deepEqual([...zones].sort(), ['above', 'below', 'into']);
  });

  it('只开放 beside 时按 50/50 分档，永远不答 into', () => {
    const besideOnly: DropBands = { into: false, beside: true };
    assert.equal(zoneFromRowPosition(ROW, yAt(0), besideOnly), 'above');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.49), besideOnly), 'above');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.5), besideOnly), 'below');
    assert.equal(zoneFromRowPosition(ROW, yAt(0.99), besideOnly), 'below');
    assert.notEqual(zoneFromRowPosition(ROW, yAt(0.5), besideOnly), 'into');
  });

  it('只开放 into 时整行都是 into', () => {
    const intoOnly: DropBands = { into: true, beside: false };
    for (const ratio of [0, 0.1, 0.5, 0.9, 0.99]) {
      assert.equal(zoneFromRowPosition(ROW, yAt(ratio), intoOnly), 'into');
    }
  });

  it('两个区都不开放时这一行不接受这次拖拽', () => {
    const none: DropBands = { into: false, beside: false };
    assert.equal(zoneFromRowPosition(ROW, yAt(0.5), none), null);
  });

  it('矩形缺失或高度为 0 时给一个确定的方向，而不是除以零', () => {
    assert.equal(zoneFromRowPosition(null, yAt(0.5), both), 'above');
    assert.equal(zoneFromRowPosition(undefined, yAt(0.5), both), 'above');
    assert.equal(zoneFromRowPosition({ top: 200, height: 0 }, 200, both), 'above');
    assert.equal(zoneFromRowPosition(null, yAt(0.5), { into: true, beside: false }), 'into');
  });

  it('分的是视口坐标，不是页面坐标', () => {
    const scrolled = { top: 40, height: 100 };
    assert.equal(zoneFromRowPosition(scrolled, 90, both), 'into');
    assert.equal(zoneFromRowPosition(scrolled, 130, both), 'below');
  });
});

// ============================================================
// bandsFor —— 父子类型矩阵怎么变成「这一行开放哪几块」
// ============================================================

describe('每一行对每一种拖拽开放的放置区', () => {
  it('模块行对模块拖拽只开放 beside —— 模块的兄弟作用域是项目顶层', () => {
    assert.deepEqual(bandsOf('module', 'm1'), { into: false, beside: true });
  });

  it('模块行对集合与接口拖拽只开放 into —— 整行都是「嵌进去」', () => {
    assert.deepEqual(bandsOf('collection', 'm1'), { into: true, beside: false });
    assert.deepEqual(bandsOf('endpoint', 'm1'), { into: true, beside: false });
  });

  it('集合行对集合与接口拖拽两个区都开放', () => {
    assert.deepEqual(bandsOf('collection', 'c1'), { into: true, beside: true });
    assert.deepEqual(bandsOf('endpoint', 'c1'), { into: true, beside: true });
  });

  it('集合行与接口行都不接受模块拖拽 —— 模块没有节点父级', () => {
    assert.deepEqual(bandsOf('module', 'c1'), { into: false, beside: false });
    assert.deepEqual(bandsOf('module', 'e1'), { into: false, beside: false });
  });

  it('接口行只接受接口拖拽', () => {
    assert.deepEqual(bandsOf('endpoint', 'e1'), { into: false, beside: true });
    assert.deepEqual(bandsOf('collection', 'e1'), { into: false, beside: false });
  });
});

// ============================================================
// resolveDrop
// ============================================================

describe('解析一次放置', () => {
  it('模块拖到模块行上重排，父级是项目顶层', () => {
    // m1 落到 m2 之下：项目顶层的同级序列去掉 m1 之后是 [m2]，m2 在那一份里的下标是 0，
    // 「在它下面」就是下标 1。
    const r = drop('module', ['m1'], 'm2', 0.8);
    assert.deepEqual(r, { ok: true, parentId: null, index: 1, zone: 'below' });
  });

  it('集合拖到模块行上嵌进去，落在该模块的子列表末尾', () => {
    // m1 的子节点是 c1、e0、c3 三个，故新槽位是 3。
    const r = drop('collection', ['c2'], 'm1', 0.5);
    assert.deepEqual(r, { ok: true, parentId: 'm1', index: 3, zone: 'into' });
  });

  it('接口拖到集合行上嵌进去', () => {
    // c1 的子节点是 c2、e1、e2、e3 四个，故新槽位是 4。
    const r = drop('endpoint', ['e0'], 'c1', 0.5);
    assert.deepEqual(r, { ok: true, parentId: 'c1', index: 4, zone: 'into' });
  });

  it('模块拖到集合或接口行上被拒 —— 模块没有节点父级', () => {
    assert.deepEqual(drop('module', ['m1'], 'c1', 0.5), { ok: false, reason: 'type' });
    assert.deepEqual(drop('module', ['m1'], 'e1', 0.5), { ok: false, reason: 'type' });
  });

  it('集合拖到接口行上被拒 —— 接口行的手势是「在容器内重排接口」', () => {
    assert.deepEqual(drop('collection', ['c1'], 'e1', 0.5), { ok: false, reason: 'type' });
  });

  it('接口拖到接口行上成为它的兄弟，落在同一父级里', () => {
    // f1 落到 f2 之下：c3 的子节点去掉 f1 之后是 [f2, f3]，f2 的下标是 0。
    const r = drop('endpoint', ['f1'], 'f2', 0.8);
    assert.deepEqual(r, { ok: true, parentId: 'c3', index: 1, zone: 'below' });
  });

  it('往下拖时落在目标后面一格，而不是再往后一格', () => {
    // 槽位按「去掉被移动者之后」的同级序列算，故 f2 在那一份序列里的下标是 0，
    // 「在它下面」就是 1。若拿含被移动者的序列算下标，这里会答 2，落到 f3 后面去。
    const r = drop('endpoint', ['f1'], 'f2', 0.8);
    assert.deepEqual(r, { ok: true, parentId: 'c3', index: 1, zone: 'below' });
  });

  it('往上拖时落在目标前面一格', () => {
    const r = drop('endpoint', ['f3'], 'f1', 0.2);
    assert.deepEqual(r, { ok: true, parentId: 'c3', index: 0, zone: 'above' });
  });

  it('落回自己被拒', () => {
    assert.deepEqual(drop('endpoint', ['e1'], 'e1', 0.5), { ok: false, reason: 'self' });
  });

  it('落进自己的后代被拒 —— 那会绕成环', () => {
    // c1 之下有 c2。把 c1 拖到 c2 里就是把自己嵌进自己的后代。
    assert.deepEqual(drop('collection', ['c1'], 'c2', 0.5), { ok: false, reason: 'cycle' });
  });

  it('拖到别的项目去被拒', () => {
    const other = node('x1', 'endpoint', 'm1', 9, 'p2');
    const r = resolveDrop({
      drag: { kind: 'endpoint', ids: ['x1'] },
      target: { kind: 'endpoint', id: 'e1' },
      rect: ROW,
      clientY: yAt(0.5),
      nodes: [...NODES, other],
      activeProjectId: 'p1',
    });
    assert.deepEqual(r, { ok: false, reason: 'cross-project' });
  });

  it('载荷声明的类型与节点自己的类型对不上时被拒', () => {
    // c3 是集合，而载荷声明自己是接口拖拽。按声明的类型算，这个集合会借着接口的身份
    // 绕过「接口行只接受接口」那条限制，被挪到 e1 旁边。
    const r = resolveDrop({
      drag: { kind: 'endpoint', ids: ['c3'] },
      target: { kind: 'endpoint', id: 'e1' },
      rect: ROW,
      clientY: yAt(0.5),
      nodes: NODES,
      activeProjectId: 'p1',
    });
    assert.deepEqual(r, { ok: false, reason: 'kind-mismatch' });
  });

  it('目标行不在树里时报「目标不存在」', () => {
    const r = resolveDrop({
      drag: { kind: 'endpoint', ids: ['e1'] },
      target: { kind: 'endpoint', id: 'nope' },
      rect: ROW,
      clientY: yAt(0.5),
      nodes: NODES,
      activeProjectId: 'p1',
    });
    assert.deepEqual(r, { ok: false, reason: 'unknown-target' });
  });

  it('没有拖拽载荷时报「没有载荷」', () => {
    const r = resolveDrop({
      drag: null,
      target: { kind: 'endpoint', id: 'e1' },
      rect: ROW,
      clientY: yAt(0.5),
      nodes: NODES,
      activeProjectId: 'p1',
    });
    assert.deepEqual(r, { ok: false, reason: 'no-drag' });
  });
});

// ============================================================
// zoneFor —— 悬停提示与落点用的是同一套判定
// ============================================================

describe('悬停提示与落点用的是同一套判定', () => {
  it('模块行上悬停集合时整行都是「嵌进去」', () => {
    for (const ratio of [0.05, 0.5, 0.95]) {
      assert.equal(
        zoneFor({ kind: 'collection', ids: ['c2'] }, target('m1'), NODES, ROW, yAt(ratio), 'p1'),
        'into',
      );
    }
  });

  it('集合行上悬停模块时没有可放的地方 —— 提示不画', () => {
    assert.equal(
      zoneFor({ kind: 'module', ids: ['m1'] }, target('c1'), NODES, ROW, yAt(0.5), 'p1'),
      null,
    );
  });

  it('接口行上悬停集合时没有可放的地方', () => {
    assert.equal(
      zoneFor({ kind: 'collection', ids: ['c1'] }, target('e1'), NODES, ROW, yAt(0.5), 'p1'),
      null,
    );
  });

  it('悬停自己那一行时不画提示 —— 落回自己会被拒', () => {
    assert.equal(
      zoneFor({ kind: 'endpoint', ids: ['e1'] }, target('e1'), NODES, ROW, yAt(0.5), 'p1'),
      null,
    );
  });

  it('悬停自己的后代时不画提示 —— 落进后代会绕成环', () => {
    // c1 之下有 c2。把 c1 拖到 c2 上就是把自己嵌进自己的后代。
    assert.equal(
      zoneFor({ kind: 'collection', ids: ['c1'] }, target('c2'), NODES, ROW, yAt(0.5), 'p1'),
      null,
    );
  });

  it('悬停提示与最终落点必须是同一个结论', () => {
    // 提示画出来了却放不下，是最难自查的一类缺陷：悬停时一切正常，松手时什么也没发生，
    // 用户只会觉得「这个软件偶尔抽风」。故这里把每一组（拖拽类型、目标行、光标位置）
    // 都跑一遍，要求两个函数给出同一个答案。
    const drags: { kind: ResourceNodeType; ids: string[] }[] = [
      { kind: 'module', ids: ['m1'] },
      { kind: 'collection', ids: ['c2'] },
      { kind: 'collection', ids: ['c1'] },
      { kind: 'endpoint', ids: ['e0'] },
      { kind: 'endpoint', ids: ['e1'] },
    ];
    for (const drag of drags) {
      for (const id of ['m1', 'm2', 'c1', 'c2', 'c3', 'e0', 'e1', 'f1']) {
        for (const ratio of [0.05, 0.5, 0.95]) {
          const decision = resolveDrop({
            drag,
            target: target(id),
            rect: ROW,
            clientY: yAt(ratio),
            nodes: NODES,
            activeProjectId: 'p1',
          });
          const zone = zoneFor(drag, target(id), NODES, ROW, yAt(ratio), 'p1');
          assert.equal(
            zone,
            decision.ok ? decision.zone : null,
            `把 ${drag.kind} ${drag.ids.join(',')} 拖到 ${id} 的 ${ratio} 处：提示与落点给了两个答案`,
          );
        }
      }
    }
  });
});
