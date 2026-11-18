/**
 * 预览结果的指纹：改哪一格，手上的旧结果就不再是「当前草稿」的结果。
 *
 * 这条判断决定了界面会不会把上一份响应说成「当前草稿」。旧的实现只比抽取规则与「有没有
 * 脚本」，于是换掉内联 URL、换一个引用接口、换取数环境、改脚本正文之后，上一份响应仍被
 * 标成当前的。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { previewFingerprint, type PreviewInput } from '../preview-input';

function base(over: Partial<PreviewInput> = {}): PreviewInput {
  return {
    sourceEndpointId: '',
    sourceInlineJson: '{"method":"GET","url":"https://example.test/points"}',
    environmentId: '',
    extractPath: '$.data',
    transformScript: '',
    activeEnvironmentId: 'env-1',
    ...over,
  };
}

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
    const asEndpoint = base({ sourceEndpointId: 'ep-1', sourceInlineJson: '' });
    const otherEndpoint = base({ sourceEndpointId: 'ep-2', sourceInlineJson: '' });
    assert.notEqual(previewFingerprint(asEndpoint), previewFingerprint(otherEndpoint));
  });

  it('改取数环境（定义点名的那一格）就是另一份输入', () => {
    const other = base({ environmentId: 'env-2' });
    assert.notEqual(previewFingerprint(base()), previewFingerprint(other));
  });

  it('改调用方当前的环境也是另一份输入：继承的定义按它取数', () => {
    const other = base({ activeEnvironmentId: 'env-2' });
    assert.notEqual(previewFingerprint(base()), previewFingerprint(other));
    assert.notEqual(
      previewFingerprint(base()),
      previewFingerprint(base({ activeEnvironmentId: null })),
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

  it('两个不同的数据源字段不会拼出同一个指纹', () => {
    // 用 JSON 数组拼而不是裸字符串相粘：否则 ("ab","c") 与 ("a","bc") 会撞在一起。
    const left = base({ sourceEndpointId: 'ab', sourceInlineJson: 'c' });
    const right = base({ sourceEndpointId: 'a', sourceInlineJson: 'bc' });
    assert.notEqual(previewFingerprint(left), previewFingerprint(right));
  });
});
