/**
 * 交给用户照抄的镜像路径必须全小写。
 *
 * GHCR 把镜像存在小写路径上（`ghcr.io/nnyjk/simple-post`），而 docker CLI 解析镜像
 * 引用时要求路径全小写：路径里出现一个大写字母，docker 就报
 * `invalid reference format: repository name must be lowercase`，那条命令根本跑不起来。
 * `github.repository` 保留仓库名原有的大小写（`Nnyjk/simple-post`），把它直接写进
 * 发布说明或发版清单，用户照抄就失败 —— 这条用例钉住那两处写法的路径。
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';

/** 会被用户或运维照抄的清单与说明文件。 */
const FILES = [
  '.github/workflows/release.yml',
  '.github/CONTRIBUTING.md',
  'AGENTS.md',
  'README.md',
];

/** docker 解析镜像引用时拒绝的写法：路径里带 GitHub 表达式。 */
const EXPRESSION_REF = 'ghcr.io/$' + '{{';

/** 读仓库根下的一个文件（路径相对于本测试文件所在目录的上两级）。 */
function read(rel: string): string {
  return readFileSync(new URL('../../' + rel, import.meta.url), 'utf8');
}

test('用户照抄的镜像路径全小写', () => {
  const offenders: string[] = [];
  for (const file of FILES) {
    const src = read(file);
    for (const m of src.matchAll(/ghcr\.io\/([A-Za-z0-9._/-]+)/g)) {
      if (m[1] !== m[1].toLowerCase()) {
        offenders.push(file + ': ghcr.io/' + m[1]);
      }
    }
    if (src.includes(EXPRESSION_REF)) {
      offenders.push(file + ': ' + EXPRESSION_REF + ' ... }}');
    }
  }
  assert.deepEqual(
    offenders,
    [],
    '这些地方写的镜像路径带大写字母，docker 解析镜像引用时会拒绝它，' +
      '照抄命令的用户拉不到镜像。路径要写成 ghcr.io/nnyjk/simple-post。',
  );
});

test('发布说明里有一条 docker 拉取镜像的命令', () => {
  const src = read('.github/workflows/release.yml');
  const m = src.match(/docker pull ghcr\.io\/([A-Za-z0-9._/-]+):/);
  assert.ok(m, 'release.yml 的发布说明里没有 docker pull ghcr.io/<path>:<tag> 这一行');
  assert.equal(m[1], m[1].toLowerCase(), '发布说明里的镜像路径必须全小写');
});
