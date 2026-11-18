/**
 * 预览结果的指纹：改哪一格，手上的旧结果就不再是「当前草稿」的结果。
 *
 * 这条判断决定了界面会不会把上一份响应说成「当前草稿」。旧的实现只比抽取规则与「有没有
 * 脚本」，于是换掉内联 URL、换一个引用接口、换取数环境、改脚本正文之后，上一份响应仍被
 * 标成当前的。指纹序列化的是**实际发给预览命令的那份参数**，故两边不可能走偏。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  previewFailureMessage,
  previewFingerprint,
  previewPayload,
  type PreviewInput,
} from '../preview-input';

function base(over: Partial<PreviewInput> = {}): PreviewInput {
  return {
    usesEndpoint: false,
    sourceEndpointId: '',
    sourceInlineJson: '{"method":"GET","url":"https://example.test/points"}',
    environmentId: '',
    extractPath: '$.data',
    transformScript: '',
    activeEnvironmentId: 'env-1',
    ...over,
  };
}

describe('previewPayload', () => {
  it('内联形态下接口与取数环境两格归零', () => {
    assert.deepEqual(previewPayload(base({ sourceEndpointId: 'ep-1', environmentId: 'env-2' })), {
      sourceEndpointId: null,
      sourceInlineJson: '{"method":"GET","url":"https://example.test/points"}',
      sourceEnvironmentId: null,
      extractPath: '$.data',
      transformScript: '',
      environmentId: 'env-1',
    });
  });

  it('引用接口形态下内联正文归零，取数环境才参与', () => {
    assert.deepEqual(
      previewPayload(
        base({
          usesEndpoint: true,
          sourceEndpointId: 'ep-1',
          sourceInlineJson: '{"url":"https://example.test/points"}',
          environmentId: 'env-2',
        }),
      ),
      {
        sourceEndpointId: 'ep-1',
        sourceInlineJson: null,
        sourceEnvironmentId: 'env-2',
        extractPath: '$.data',
        transformScript: '',
        environmentId: 'env-1',
      },
    );
  });

  it('空串与空值同义：清空的接口就是「没有」', () => {
    assert.equal(previewPayload(base({ usesEndpoint: true })).sourceEndpointId, null);
    assert.equal(previewPayload(base({ sourceInlineJson: '' })).sourceInlineJson, null);
  });
});

describe('previewFingerprint', () => {
  it('同样的输入给同样的指纹', () => {
    assert.equal(previewFingerprint(base()), previewFingerprint(base()));
  });

  it('换掉内联 URL 就是另一份输入', () => {
    const changed = base({
      sourceInlineJson: '{"method":"GET","url":"https://example.test/other"}',
    });
    assert.notEqual(previewFingerprint(base()), previewFingerprint(changed));
  });

  it('换掉引用的接口就是另一份输入', () => {
    const asEndpoint = base({ usesEndpoint: true, sourceEndpointId: 'ep-1', sourceInlineJson: '' });
    const otherEndpoint = base({
      usesEndpoint: true,
      sourceEndpointId: 'ep-2',
      sourceInlineJson: '',
    });
    assert.notEqual(previewFingerprint(asEndpoint), previewFingerprint(otherEndpoint));
  });

  it('两格都填着内容时切换来源形态，也是另一份输入', () => {
    const filled = base({ sourceEndpointId: 'ep-1', environmentId: 'env-2' });
    assert.notEqual(
      previewFingerprint(filled),
      previewFingerprint({ ...filled, usesEndpoint: true }),
    );
  });

  it('改取数环境（定义点名的那一格）就是另一份输入', () => {
    const asEndpoint = base({ usesEndpoint: true, sourceEndpointId: 'ep-1' });
    const other = { ...asEndpoint, environmentId: 'env-2' };
    assert.notEqual(previewFingerprint(asEndpoint), previewFingerprint(other));
  });

  it('只有继承环境的草稿才受调用方环境影响', () => {
    const inheriting = base({ usesEndpoint: true, sourceEndpointId: 'ep-1' });
    assert.notEqual(
      previewFingerprint(inheriting),
      previewFingerprint({ ...inheriting, activeEnvironmentId: 'env-2' }),
      '继承的定义按调用方环境取数，换环境就是另一份输入',
    );
    assert.notEqual(
      previewFingerprint(inheriting),
      previewFingerprint({ ...inheriting, activeEnvironmentId: null }),
    );

    // 点名了取数环境：这一次取数不会因为调用方换了环境而不同，指纹也不该变——变了就会误报
    // 「这份结果对应旧输入」，让用户白白再打一次请求（非幂等接口就是对端生效两次）。
    const pinned = base({ usesEndpoint: true, sourceEndpointId: 'ep-1', environmentId: 'env-9' });
    assert.equal(
      previewFingerprint(pinned),
      previewFingerprint({ ...pinned, activeEnvironmentId: 'env-2' }),
    );

    // 内联请求根本用不到环境。
    assert.equal(
      previewFingerprint(base()),
      previewFingerprint(base({ activeEnvironmentId: 'env-2' })),
    );
  });

  it('脚本正文改一个字就是另一份输入（不是只比「有没有脚本」）', () => {
    const a = base({ transformScript: 'return pm.data;' });
    const b = base({ transformScript: 'return pm.data.rows;' });
    assert.notEqual(previewFingerprint(a), previewFingerprint(b));
    // 空脚本与只写空白按「不写脚本」看待这件事由后端归一，指纹只需如实区分正文。
    assert.notEqual(previewFingerprint(base()), previewFingerprint(a));
  });

  it('抽取规则改了是另一份输入', () => {
    assert.notEqual(
      previewFingerprint(base()),
      previewFingerprint(base({ extractPath: '$.rows' })),
    );
  });

  it('两个字段的内容不会串位', () => {
    // 序列化的是带键的对象，故 ("$.a","b") 与 ("$.ab","") 不会撞成同一个指纹。
    const left = base({ extractPath: '$.a', transformScript: 'b' });
    const right = base({ extractPath: '$.ab', transformScript: '' });
    assert.notEqual(previewFingerprint(left), previewFingerprint(right));
  });
});

describe('previewFailureMessage', () => {
  it('指纹一致才显示：那条错误属于这份输入', () => {
    assert.equal(
      previewFailureMessage({ fingerprint: 'fp-1', message: '连接被拒绝' }, 'fp-1'),
      '连接被拒绝',
    );
  });

  it('指纹对不上就当这份草稿还没有结果', () => {
    // URL A 的失败不该显示在改后的 B 名下。
    assert.equal(
      previewFailureMessage({ fingerprint: 'fp-a', message: '连接被拒绝' }, 'fp-b'),
      null,
    );
  });

  it('没有失败时什么也不显示', () => {
    assert.equal(previewFailureMessage(null, 'fp-1'), null);
  });
});
