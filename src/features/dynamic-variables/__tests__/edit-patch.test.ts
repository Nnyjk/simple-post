/**
 * `buildEditPatch` 的三态语义。
 *
 * 后端的 patch 是双 Option：**缺席 = 不动**、`null` = 清除、值 = 写入。故这里钉三件事：
 *   - 没动过的字段必须真的缺席（否则两个窗口一起编辑时，一个只改名的保存会把别人的改动
 *     覆盖回去）；
 *   - 清空一个原本有值的可空字段要发 `null`，不是缺席；
 *   - 「本来就空、现在还是空」什么也不发（发 `null` 会撞后端「必须有一个数据源」的校验）。
 *
 * 运行：
 *   npx tsx --test src/features/dynamic-variables/__tests__/edit-patch.test.ts
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import type { DynamicVariable } from '@/types/domain';
import type { DynamicVariableDraft } from '@/stores/dynamic-slice';

import { buildEditPatch, ttlSecondsOf } from '../edit-patch';

/** 一条引用接口的定义。 */
function definition(over: Partial<DynamicVariable> = {}): DynamicVariable {
  return {
    id: 'd-sns',
    projectId: 'p1',
    name: 'sns',
    description: null,
    sourceEndpointId: 'ep-1',
    sourceEnvironmentId: null,
    sourceInlineJson: null,
    extractPath: '$.data',
    transformScript: null,
    valueMode: 'auto',
    ttlSeconds: 60,
    staleFallback: false,
    enabled: true,
    sortOrder: 0,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

/** 与 `definition()` 一一对应的草稿：默认表示「一个字都没改」。 */
function draftOf(initial: DynamicVariable, over: Partial<DynamicVariableDraft> = {}) {
  return {
    creating: false,
    editingId: initial.id,
    openedWith: initial,
    name: initial.name,
    description: initial.description ?? '',
    sourceKind: initial.sourceEndpointId ? 'endpoint' : 'inline',
    sourceEndpointId: initial.sourceEndpointId ?? '',
    sourceProjectId: 'p1',
    sourceEndpointName: initial.sourceEndpointName ?? '',
    sourceInlineJson: initial.sourceInlineJson ?? '',
    environmentId: initial.sourceEnvironmentId ?? '',
    extractPath: initial.extractPath,
    transformScript: initial.transformScript ?? '',
    valueMode: initial.valueMode,
    ttlSeconds: String(initial.ttlSeconds),
    staleFallback: initial.staleFallback,
    enabled: initial.enabled,
    ...over,
  } satisfies DynamicVariableDraft;
}

describe('buildEditPatch', () => {
  it('一个字都没改时 patch 是空的', () => {
    const initial = definition();
    assert.deepEqual(buildEditPatch(initial, draftOf(initial)), {});
  });

  it('只改了名字：patch 里只有 name，别人的改动不会被覆盖回去', () => {
    const initial = definition();
    const patch = buildEditPatch(initial, draftOf(initial, { name: 'points' }));

    assert.deepEqual(patch, { name: 'points' });
    assert.equal('sourceEndpointId' in patch, false, '数据源没动过就必须缺席');
    assert.equal('sourceInlineJson' in patch, false);
    assert.equal('sourceEnvironmentId' in patch, false);
    assert.equal('description' in patch, false);
    assert.equal('transformScript' in patch, false);
  });

  it('清空一个有说明的定义：description 发 null，不是缺席', () => {
    const initial = definition({ description: '点数的接口' });
    assert.deepEqual(buildEditPatch(initial, draftOf(initial, { description: '' })), {
      description: null,
    });
    // 只剩空白也算清空：那不是内容。
    assert.deepEqual(buildEditPatch(initial, draftOf(initial, { description: '   ' })), {
      description: null,
    });
  });

  it('本来就没有说明、现在仍为空：不发 null', () => {
    const initial = definition({ description: null });
    const patch = buildEditPatch(initial, draftOf(initial, { description: '' }));

    assert.equal('description' in patch, false);
  });

  it('清空处理脚本发 null，只写空白的脚本按清空算', () => {
    const initial = definition({ transformScript: 'return pm.data;' });
    const patch = buildEditPatch(initial, draftOf(initial, { transformScript: '  \n ' }));

    assert.deepEqual(patch, { transformScript: null });
  });

  it('缓存时长清空或写坏时按 60 秒算，只有真的变了才发', () => {
    const initial = definition({ ttlSeconds: 60 });
    assert.equal(
      'ttlSeconds' in buildEditPatch(initial, draftOf(initial, { ttlSeconds: '' })),
      false,
    );
    assert.equal(
      'ttlSeconds' in buildEditPatch(initial, draftOf(initial, { ttlSeconds: 'abc' })),
      false,
    );
    assert.deepEqual(buildEditPatch(initial, draftOf(initial, { ttlSeconds: '0' })), {
      ttlSeconds: 0,
    });
    assert.equal(ttlSecondsOf(''), 60);
    assert.equal(ttlSecondsOf('0'), 0);
  });

  it('换成内联请求：接口与取数环境一起清成 null，内联正文写进去', () => {
    const initial = definition({ sourceEnvironmentId: 'env-1' });
    const patch = buildEditPatch(
      initial,
      draftOf(initial, {
        sourceKind: 'inline',
        sourceInlineJson: '{"method":"GET","url":"https://example.test/points"}',
      }),
    );

    assert.equal(patch.sourceEndpointId, null);
    assert.equal(patch.sourceEnvironmentId, null);
    assert.equal(patch.sourceInlineJson, '{"method":"GET","url":"https://example.test/points"}');
  });

  it('换掉引用的接口：只发 sourceEndpointId，环境那一格不动', () => {
    const initial = definition({ sourceEndpointId: 'ep-1' });
    const patch = buildEditPatch(initial, draftOf(initial, { sourceEndpointId: 'ep-2' }));

    assert.deepEqual(patch, { sourceEndpointId: 'ep-2' });
  });

  it('数据源本来为空、现在仍为空：三格都不发（否则撞「必须有一个数据源」）', () => {
    const initial = definition({
      sourceEndpointId: null,
      sourceInlineJson: null,
      sourceEnvironmentId: null,
    });
    const patch = buildEditPatch(
      initial,
      draftOf(initial, { sourceKind: 'inline', sourceInlineJson: '  ' }),
    );

    assert.deepEqual(patch, {});
  });
  it('定义的脚本原本只有空白：只改名字时不发 transformScript', () => {
    // 两边不过同一个归一时，「空白」与「null」不等，只改个名字也会把脚本一起清成 null。
    const initial = definition({ transformScript: '   ' });
    const patch = buildEditPatch(
      initial,
      draftOf(initial, { name: 'points', transformScript: '  ' }),
    );

    assert.deepEqual(patch, { name: 'points' });
  });

  it('把一段非空的脚本清掉：发 null', () => {
    const initial = definition({ transformScript: 'return pm.data;' });
    const patch = buildEditPatch(initial, draftOf(initial, { transformScript: '   ' }));

    assert.deepEqual(patch, { transformScript: null });
  });
});
