/**
 * `src/features/workspace/tab-disambiguation.ts` 的纯单测 —— 标签条上同名接口的消歧。
 *
 * 标签的身份是 id，标题是接口名，而接口名在项目里**不唯一**：不同模块、不同集合下可以有
 * 同名接口，标签条于是摆出两个一字不差的标签，用户无从分辨哪个是哪个。这里钉住四条规则：
 *
 *   1. 只有**同名**的接口标签才需要可见的提示段落。没有冲突时标签与原来一模一样——
 *      那一段要占掉标题右侧约 40px，平时留着就是白花掉标题的空间。
 *   2. 提示按级别往外退：先试直接父容器的名字，不足以区分这一组就退到模块名，再不够退到
 *      完整路径。三个级别都区分不了时不写提示。
 *   3. 分组按**视觉上**的标题走：连续的空白压成一个空格、首尾空白去掉，故「用户列表」与
 *      「用户列表 」算同名。
 *   4. 找不到容器时路径是空的，标签维持原样——不写「（无）」这类占位文字。
 *
 * 运行：npx tsx --test src/features/workspace/__tests__/tab-disambiguation.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { endpointTabPaths, type EndpointTabSource } from '../tab-disambiguation';
import type { ResourceNode, ResourceNodeType } from '@/types/domain';

/** 最小化的领域形状节点；路径计算只读 `id` / `parentId` / `kind` / `name`。 */
function makeNode(
  id: string,
  parentId: string | null,
  kind: ResourceNodeType,
  name: string,
): ResourceNode {
  return {
    id,
    rootId: 'p1',
    parentId,
    kind,
    name,
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
  };
}

/** 两个模块，各带一个同名集合；两个同名接口分别挂在两个模块下。 */
const TREE: ResourceNode[] = [
  makeNode('m1', null, 'module', '支付'),
  makeNode('c1', 'm1', 'collection', '订单'),
  makeNode('m2', null, 'module', '账户'),
  makeNode('c2', 'm2', 'collection', '订单'),
  makeNode('c3', 'm1', 'collection', '售后'),
];

/** 更深的一棵树：三个容器下各挂一个同名集合「明细」。 */
const DEEP_TREE: ResourceNode[] = [
  makeNode('m1', null, 'module', '支付'),
  makeNode('c1', 'm1', 'collection', '订单'),
  makeNode('m2', null, 'module', '账户'),
  makeNode('c2', 'm2', 'collection', '订单'),
  makeNode('c3', 'm1', 'collection', '售后'),
  makeNode('c4', 'c1', 'collection', '明细'),
  makeNode('c5', 'c3', 'collection', '明细'),
  makeNode('c6', 'c2', 'collection', '明细'),
];

describe('tab-disambiguation: a name that no one else uses needs no visible hint', () => {
  it('leaves the hint empty but still computes the path', () => {
    const sources: EndpointTabSource[] = [{ id: 'e1', title: '用户列表', parentId: 'c1' }];

    const info = endpointTabPaths(sources, TREE).get('e1');

    assert.equal(info?.hint, '', '没有同名标签时，标题右侧那一段必须空着');
    assert.equal(info?.path, '支付 › 订单', '完整路径照算，它不占地方');
  });
});

describe('tab-disambiguation: same-named endpoints tell each other apart by path', () => {
  it('gives both of them the container they sit in', () => {
    const sources: EndpointTabSource[] = [
      { id: 'e1', title: '用户列表', parentId: 'c1' },
      { id: 'e2', title: '用户列表', parentId: 'm2' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('e1')?.hint, '订单');
    assert.equal(paths.get('e1')?.path, '支付 › 订单');
    // 一个挂在集合下、一个直接挂在模块下：可见的那一段各是它自己那一层的容器。
    assert.equal(paths.get('e2')?.hint, '账户');
    assert.equal(paths.get('e2')?.path, '账户');
  });

  it('falls back to the module name when the collections themselves share a name', () => {
    // 两个集合同名时，只写「订单」的两个标签还是分不出谁是谁。此时退到模块名——
    // 模块名在这一组里互不相同，且比完整路径短得多。
    const sources: EndpointTabSource[] = [
      { id: 'saved', title: '用户列表', parentId: 'c1' },
      { id: 'draft', title: '用户列表', parentId: 'c2' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('saved')?.hint, '支付');
    assert.equal(paths.get('draft')?.hint, '账户');
    // 完整路径照旧是各自的完整路径——悬停提示靠它区分。
    assert.equal(paths.get('saved')?.path, '支付 › 订单');
    assert.equal(paths.get('draft')?.path, '账户 › 订单');
  });

  it('falls back to the whole path when neither the container nor the module tells them apart', () => {
    // 三个同名接口：父容器同名，模块也有一对同名。父容器名与模块名都区分不了这一组，
    // 只有完整路径能——它在这一组里互不相同。
    const sources: EndpointTabSource[] = [
      { id: 'e1', title: '用户列表', parentId: 'c4' },
      { id: 'e2', title: '用户列表', parentId: 'c5' },
      { id: 'e3', title: '用户列表', parentId: 'c6' },
    ];

    const paths = endpointTabPaths(sources, DEEP_TREE);

    assert.equal(paths.get('e1')?.hint, '支付 › 订单 › 明细');
    assert.equal(paths.get('e2')?.hint, '支付 › 售后 › 明细');
    assert.equal(paths.get('e3')?.hint, '账户 › 订单 › 明细');
  });

  it('treats an unsaved draft as one of the same-named endpoints', () => {
    // 草稿的标签标题也是接口名，故它与同名接口的冲突同样要消歧——
    // 否则用户看到两个一模一样的标签，分不出哪个是刚新建的。
    const sources: EndpointTabSource[] = [
      { id: 'saved', title: '用户列表', parentId: 'c1' },
      { id: 'draft', title: '用户列表', parentId: 'c3' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('saved')?.hint, '订单');
    assert.equal(paths.get('draft')?.hint, '售后');
  });

  it('writes no hint when even the paths are identical', () => {
    // 同名模块下的同名集合里各有一个同名接口：路径完全一样，任何提示都区分不了它们。
    // 此时不写提示——写一段看起来有区别的文字，比不写更让人误解。
    const sources: EndpointTabSource[] = [
      { id: 'e1', title: '用户列表', parentId: 'c1' },
      { id: 'e2', title: '用户列表', parentId: 'c1' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('e1')?.hint, '');
    assert.equal(paths.get('e2')?.hint, '');
    assert.equal(paths.get('e1')?.path, '支付 › 订单');
  });
});

describe('tab-disambiguation: titles are compared the way they are drawn', () => {
  it('treats a trailing space as the same name', () => {
    // 草稿的名字在保存之前没有被 trim 过，标签上那个尾随空格看不出来，
    // 但两个标签看起来一模一样——它们必须算同名，否则谁也不给提示。
    const sources: EndpointTabSource[] = [
      { id: 'e1', title: '用户列表', parentId: 'c1' },
      { id: 'e2', title: '用户列表 ', parentId: 'm2' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('e1')?.hint, '订单');
    assert.equal(paths.get('e2')?.hint, '账户');
  });

  it('treats a run of inner spaces as one space', () => {
    // HTML 把连续的空白渲染成一个空格，故这两个标签看起来一样。
    const sources: EndpointTabSource[] = [
      { id: 'e1', title: '用户 列表', parentId: 'c1' },
      { id: 'e2', title: '用户  列表', parentId: 'm2' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('e1')?.hint, '订单');
    assert.equal(paths.get('e2')?.hint, '账户');
  });

  it('keeps the drawn title as it is', () => {
    // 分组用的键是归一化后的，标签上显示的仍是原始标题。
    const sources: EndpointTabSource[] = [
      { id: 'e1', title: '用户列表 ', parentId: 'c1' },
      { id: 'e2', title: '用户列表', parentId: 'm2' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('e1')?.path, '支付 › 订单');
    assert.equal(paths.get('e2')?.path, '账户');
  });
});

describe('tab-disambiguation: an endpoint without a container gets no hint at all', () => {
  it('reports an empty path instead of a placeholder', () => {
    const sources: EndpointTabSource[] = [
      { id: 'e1', title: '用户列表', parentId: 'gone' },
      { id: 'e2', title: '用户列表', parentId: 'gone' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('e1')?.path, '');
    assert.equal(paths.get('e1')?.hint, '');
    assert.equal(paths.get('e2')?.hint, '');
  });
});
