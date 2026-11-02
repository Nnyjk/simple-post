/**
 * collection 脚本的 patch 形状。
 *
 * 规则必须成立：空脚本必须按「清除」发送，否则会把存的那份抹成一个空串，而不是
 * 删掉它。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { buildScriptsPatch } from '../scripts-patch';

describe('collection 脚本的 patch', () => {
  it('空脚本按「清除」发送', () => {
    assert.deepEqual(buildScriptsPatch('', ''), {
      preRequestScript: null,
      testScript: null,
    });
  });

  it('纯空白也算空', () => {
    assert.deepEqual(buildScriptsPatch('  \n ', '\t'), {
      preRequestScript: null,
      testScript: null,
    });
  });

  it('有内容就原样发送', () => {
    assert.deepEqual(
      buildScriptsPatch("pm.environment.set('a','1');", 'pm.test("t", function () {});'),
      {
        preRequestScript: "pm.environment.set('a','1');",
        testScript: 'pm.test("t", function () {});',
      },
    );
  });

  it('一段空不影响另一段', () => {
    assert.deepEqual(buildScriptsPatch('keep', ''), {
      preRequestScript: 'keep',
      testScript: null,
    });
  });
});
