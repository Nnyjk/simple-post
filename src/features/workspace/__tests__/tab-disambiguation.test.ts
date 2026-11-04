/**
 * `src/features/workspace/tab-disambiguation.ts` 的纯单测 —— 标签条上同名接口的消歧。
 *
 * 标签的身份是 id，标题是接口名，而接口名在项目里**不唯一**：不同模块、不同集合下可以有
 * 同名接口，标签条于是摆出两个一字不差的标签，用户无从分辨哪个是哪个。这里钉住三条规则：
 *
 *   1. 只有**同名**的接口标签才需要可见的提示段落。没有冲突时标签与原来一模一样——
 *      那一段要占掉标题右侧约 40px，平时留着就是白花掉标题的空间。
 *   2. 可见的那一段是路径的最后一段（最靠近接口的容器），完整路径另有一份，供悬停提示用。
 *   3. 找不到容器时路径是空的，标签维持原样——不写「（无）」这类占位文字。
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

/** 两个模块，各带一个集合；两个同名接口分别挂在两个模块下。 */
const TREE: ResourceNode[] = [
  makeNode('m1', null, 'module', '支付'),
  makeNode('c1', 'm1', 'collection', '订单'),
  makeNode('m2', null, 'module', '账户'),
  makeNode('c2', 'm2', 'collection', '订单'),
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

  it('treats an unsaved draft as one of the same-named endpoints', () => {
    // 草稿的标签标题也是接口名，故它与同名接口的冲突同样要消歧——
    // 否则用户看到两个一模一样的标签，分不出哪个是刚新建的。
    const sources: EndpointTabSource[] = [
      { id: 'saved', title: '用户列表', parentId: 'c1' },
      { id: 'draft', title: '用户列表', parentId: 'c2' },
    ];

    const paths = endpointTabPaths(sources, TREE);

    assert.equal(paths.get('saved')?.hint, '订单');
    assert.equal(paths.get('draft')?.hint, '订单');
    // 两个集合同名，但完整路径不同——悬停提示靠它区分。
    assert.equal(paths.get('saved')?.path, '支付 › 订单');
    assert.equal(paths.get('draft')?.path, '账户 › 订单');
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
