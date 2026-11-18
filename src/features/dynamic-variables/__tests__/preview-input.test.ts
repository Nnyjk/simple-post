/**
 * 预览结果的指纹：改哪一格，手上的旧结果就不再是「当前草稿」的结果。
 *
 * 这条判断决定了界面会不会把上一份响应说成「当前草稿」。旧的实现只比抽取规则与「有没有
 * 脚本」，于是换掉内联 URL、换一个引用接口、改脚本正文之后，上一份响应仍被标成当前的。
 *
 * **环境不进指纹**：一份响应最终在哪个环境下取到，只有后端说得准（定义点名 / 继承调用方 /
 * 回落到数据源接口所属项目的默认环境）。前端按调用方环境算，本项目里切一下环境就会把同一份
 * 结果标成「对应旧输入」，还诱导用户再点一次预览（非幂等接口就是对端生效两次）。环境那一路
 * 由 `previewExpectedEnvironment` 单独回答，界面据此在结果旁补一句「这份结果取自 X」。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  previewExpectedEnvironment,
  previewFailureKey,
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

  it('环境不进指纹：换调用方环境、换点名环境都还是同一份输入', () => {
    const inheriting = base({ usesEndpoint: true, sourceEndpointId: 'ep-1' });
    assert.equal(
      previewFingerprint(inheriting),
      previewFingerprint({ ...inheriting, activeEnvironmentId: 'env-2' }),
      '切一下调用方环境不该把手上这份响应标成「旧输入」',
    );
    assert.equal(
      previewFingerprint(inheriting),
      previewFingerprint({ ...inheriting, activeEnvironmentId: null }),
    );

    const pinned = base({ usesEndpoint: true, sourceEndpointId: 'ep-1', environmentId: 'env-9' });
    assert.equal(
      previewFingerprint(pinned),
      previewFingerprint({ ...pinned, environmentId: 'env-10' }),
      '点名环境由后端的回落规则兜底，前端复刻只会误标',
    );

    // 内联请求根本用不到环境。
    assert.equal(
      previewFingerprint(base()),
      previewFingerprint(base({ activeEnvironmentId: 'env-2' })),
    );
  });

  it('改抽取规则就是另一份输入', () => {
    assert.notEqual(
      previewFingerprint(base()),
      previewFingerprint(base({ extractPath: '$.rows' })),
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

describe('previewExpectedEnvironment', () => {
  it('定义点名了取数环境就用它', () => {
    assert.equal(
      previewExpectedEnvironment(
        base({ usesEndpoint: true, sourceEndpointId: 'ep-1', environmentId: 'env-9' }),
      ),
      'env-9',
    );
  });

  it('没点名时用调用方此刻选中的那个', () => {
    assert.equal(
      previewExpectedEnvironment(base({ usesEndpoint: true, sourceEndpointId: 'ep-1' })),
      'env-1',
    );
    assert.equal(
      previewExpectedEnvironment(
        base({ usesEndpoint: true, sourceEndpointId: 'ep-1', activeEnvironmentId: null }),
      ),
      null,
      '调用方也没有可用环境时，这一档算不出来',
    );
  });

  it('内联请求用不到环境', () => {
    assert.equal(previewExpectedEnvironment(base()), null);
  });
});

describe('previewFailureMessage', () => {
  it('键一致才显示：那条错误属于这份输入的这个环境', () => {
    const key = previewFailureKey('fp-1', 'env-1');
    assert.equal(previewFailureMessage({ key, message: '连接被拒绝' }, key), '连接被拒绝');
  });

  it('指纹对不上就当这份草稿还没有结果', () => {
    // URL A 的失败不该显示在改后的 B 名下。
    assert.equal(
      previewFailureMessage(
        { key: previewFailureKey('fp-a', 'env-1'), message: '连接被拒绝' },
        previewFailureKey('fp-b', 'env-1'),
      ),
      null,
    );
  });

  it('同一份输入换了环境，就不再是同一个失败', () => {
    // 失败只带一句原因，看不出它属于哪个环境：A 的连接被拒绝，不该挂在 B 名下。
    assert.equal(
      previewFailureMessage(
        { key: previewFailureKey('fp-1', 'env-1'), message: '连接被拒绝' },
        previewFailureKey('fp-1', 'env-2'),
      ),
      null,
    );
  });

  it('算不出会生效的环境时按空串记，仍然与「有环境」分开', () => {
    assert.notEqual(previewFailureKey('fp-1', null), previewFailureKey('fp-1', 'env-1'));
  });

  it('没有失败时什么也不显示', () => {
    assert.equal(previewFailureMessage(null, previewFailureKey('fp-1', 'env-1')), null);
  });
});
