/**
 * 路径拼接与脚本片段生成 —— 这里钉的是「点出来的路径后端一定读得懂」。
 *
 * 后端用 `serde_json_path` 解析抽取规则，它的点号简写只收 ASCII 字母/数字/下划线，所以
 * 「看起来只是个键」和「能不能写成 `.key`」是两件事：前者靠眼睛，后者靠解析器。测试里
 * 中文键、`$`、空格、引号、反斜杠、控制字符都走括号形式，且转义必须是 JSONPath 认的。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  ROOT_PATH,
  buildScriptSnippet,
  escapeKey,
  isBareKey,
  isUnderPath,
  normalizePath,
  pathForIndex,
  pathForKey,
  pathForSegments,
  shouldConfirmOverwrite,
} from '../extract-path';

describe('isBareKey', () => {
  it('只放行 ASCII 字母/数字/下划线', () => {
    for (const key of ['a', '_x', 'a1', 'A_9']) assert.equal(isBareKey(key), true, key);
    for (const key of ['', '1a', 'a-b', 'a b', '中文', 'a$b', '$', 'a.b']) {
      assert.equal(isBareKey(key), false, key);
    }
  });

  it('带 $ 的键不算安全标识符（serde_json_path 的点号简写不收 $）', () => {
    assert.equal(isBareKey('user$id'), false);
    assert.equal(pathForKey('$', 'user$id'), "$['user$id']");
  });
});

describe('pathForKey', () => {
  it('安全键写成点号形式', () => {
    assert.equal(pathForKey('$', 'data'), '$.data');
    assert.equal(pathForKey('$.data', 'items'), '$.data.items');
  });

  it('非安全键写成括号形式', () => {
    assert.equal(pathForKey('$', 'first name'), "$['first name']");
    assert.equal(pathForKey('$', '中文'), "$['中文']");
    assert.equal(pathForKey('$.a', '1st'), "$.a['1st']");
  });

  it('括号里的引号与反斜杠要转义', () => {
    assert.equal(escapeKey("it's"), "it\\'s");
    assert.equal(escapeKey('a\\b'), 'a\\\\b');
    assert.equal(pathForKey('$', "it's"), "$['it\\'s']");
    assert.equal(pathForKey('$', 'a\\b'), "$['a\\\\b']");
  });

  it('控制字符写成转义序列，不能原样落进路径', () => {
    assert.equal(escapeKey('a\nb'), 'a\\nb');
    assert.equal(escapeKey('a\tb'), 'a\\tb');
    assert.equal(escapeKey('a\u0001b'), 'a\\u0001b');
    assert.equal(pathForKey('$', 'a\nb'), "$['a\\nb']");
  });
});

describe('pathForIndex', () => {
  it('下标接在父路径后面', () => {
    assert.equal(pathForIndex('$', 0), '$[0]');
    assert.equal(pathForIndex('$.items', 12), '$.items[12]');
    assert.equal(pathForIndex('$[0]', 3), '$[0][3]');
  });

  it('负数与非整数是调用方的错，当场抛（不悄悄生成一条读不懂的路径）', () => {
    assert.throws(() => pathForIndex('$', -1), RangeError);
    assert.throws(() => pathForIndex('$', 1.5), RangeError);
    assert.throws(() => pathForIndex('$', Number.NaN), RangeError);
  });
});

describe('pathForSegments', () => {
  it('混合键与下标一路接下去', () => {
    assert.equal(pathForSegments([]), '$');
    assert.equal(pathForSegments(['a', 0, 'b b']), "$.a[0]['b b']");
    assert.equal(pathForSegments(['data', 'list', 2]), '$.data.list[2]');
  });
});

describe('normalizePath', () => {
  it('空白等于没填，回落到根', () => {
    assert.equal(normalizePath(''), '$');
    assert.equal(normalizePath('   '), '$');
    assert.equal(normalizePath(null), '$');
    assert.equal(normalizePath(undefined), '$');
  });

  it('已有路径原样保留（只裁空白）', () => {
    assert.equal(normalizePath('  $.a.b  '), '$.a.b');
    assert.equal(ROOT_PATH, '$');
  });
});

describe('isUnderPath', () => {
  it('相等算在下面', () => {
    assert.equal(isUnderPath('$.a', '$.a'), true);
    assert.equal(isUnderPath('$', '$'), true);
  });

  it('后代算在下面', () => {
    assert.equal(isUnderPath('$.a.b', '$.a'), true);
    assert.equal(isUnderPath('$.a[0]', '$.a'), true);
    assert.equal(isUnderPath("$['a b'].c", "$['a b']"), true);
    assert.equal(isUnderPath('$.a.b', '$'), true);
    assert.equal(isUnderPath('$[0][1]', '$[0]'), true);
  });

  it('前缀相同但不是后代 —— 不算', () => {
    assert.equal(isUnderPath('$.ab', '$.a'), false);
    assert.equal(isUnderPath("$['ab']", "$['a']"), false);
    assert.equal(isUnderPath('$.b', '$.a'), false);
    assert.equal(isUnderPath('$', '$.a'), false);
  });
});

describe('buildScriptSnippet', () => {
  it('节点在抽取规则之下时点到 pm.data 的那一格，且默认语句仍是原样返回', () => {
    const snippet = buildScriptSnippet('$.data.list[0].id', '$.data');
    assert.equal(snippet.usesExtract, true);
    assert.match(snippet.code, /^return pm\.data;$/m, '直接保存必须等于不写脚本');
    assert.match(snippet.code, /点选的这一格：pm\.data\.list\[0\]\.id/);
    // 抽取结果与原始响应两种输入都要点到，用户才知道还有哪些东西可用。
    assert.match(snippet.code, /pm\.response\.text\(\)/);
    assert.match(snippet.code, /pm\.response\.json\(\)/);
  });

  it('节点就是抽取规则本身时，点选的那一格就是整个 pm.data', () => {
    const snippet = buildScriptSnippet('$.data', '$.data');
    assert.equal(snippet.usesExtract, true);
    assert.match(snippet.code, /点选的这一格：pm\.data$/m);
    assert.match(snippet.code, /^return pm\.data;$/m);
  });

  it('抽取规则留空时按根算，片段指向 pm.data 的全路径', () => {
    const snippet = buildScriptSnippet('$.a.b', '');
    assert.equal(snippet.usesExtract, true);
    assert.match(snippet.code, /点选的这一格：pm\.data\.a\.b/);
  });

  it('节点不在抽取规则之下时退回原始响应，并明说它不依赖抽取规则', () => {
    const snippet = buildScriptSnippet('$.data.list', '$.data.items');
    assert.equal(snippet.usesExtract, false);
    assert.match(snippet.code, /点选的这一格：pm\.response\.json\(\)\.data\.list/);
    assert.match(snippet.code, /^return pm\.data;$/m, '换了取材点也不许改默认取值');
    assert.match(snippet.hint, /不依赖抽取规则/);
  });

  it('根节点在别的抽取规则之下也算「不在下面」', () => {
    const snippet = buildScriptSnippet('$', '$.data');
    assert.equal(snippet.usesExtract, false);
    assert.match(snippet.code, /点选的这一格：pm\.response\.json\(\)$/m);
  });

  it('路径没以 $ 开头时补成绝对路径，不拼出后端读不懂的东西', () => {
    assert.match(buildScriptSnippet('a.b', '').code, /点选的这一格：pm\.data\.a\.b/);
    assert.match(buildScriptSnippet('.a.b', '').code, /点选的这一格：pm\.data\.a\.b/);
  });

  it('模板保持简短：十行以内', () => {
    for (const code of [
      buildScriptSnippet('$.a', '').code,
      buildScriptSnippet('$.a', '$.b').code,
    ]) {
      assert.ok(code.split('\n').length <= 10, code);
    }
  });
});

describe('shouldConfirmOverwrite', () => {
  it('空脚本不必确认，写过的必须确认', () => {
    assert.equal(shouldConfirmOverwrite(''), false);
    assert.equal(shouldConfirmOverwrite('   '), false);
    assert.equal(shouldConfirmOverwrite(null), false);
    assert.equal(shouldConfirmOverwrite('return pm.data;'), true);
  });
});
