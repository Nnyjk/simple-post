import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  GITHUB_MIRROR_IDS,
  PROJECT_GITHUB_URL,
  mirrorGithubUrl,
  projectHomeUrl,
  resolveGithubMirror,
  type GithubMirrorId,
} from '../github-mirror';

/**
 * 镜像重写的回归网。失败模式是静默的：链接改写到一半（丢了 query / hash）用户点开
 * 仍是那个页面，只有路径与参数少了一截才看得出来，故逐条钉住形状。
 */
const RELEASE_URL = 'https://github.com/owner/repo/releases/tag/v1.2.3';

test('direct 原样返回', () => {
  assert.equal(mirrorGithubUrl(RELEASE_URL, 'direct'), RELEASE_URL);
  assert.equal(
    mirrorGithubUrl('https://api.github.com/repos/a/b', 'direct'),
    'https://api.github.com/repos/a/b',
  );
});

test('前缀镜像把原 URL 接在前缀之后', () => {
  assert.equal(mirrorGithubUrl(RELEASE_URL, 'ghfast'), `https://ghfast.top/${RELEASE_URL}`);
  assert.equal(mirrorGithubUrl(RELEASE_URL, 'ghproxy'), `https://gh-proxy.com/${RELEASE_URL}`);
  assert.equal(mirrorGithubUrl(RELEASE_URL, 'moeyy'), `https://github.moeyy.xyz/${RELEASE_URL}`);
});

test('kkgithub 只换主机，path / query / hash 保留', () => {
  assert.equal(
    mirrorGithubUrl('https://github.com/owner/repo/blob/main/README.md?plain=1#L20', 'kkgithub'),
    'https://kkgithub.com/owner/repo/blob/main/README.md?plain=1#L20',
  );
  assert.equal(
    mirrorGithubUrl('https://www.github.com/owner/repo', 'kkgithub'),
    'https://kkgithub.com/owner/repo',
  );
  assert.equal(
    mirrorGithubUrl('https://raw.githubusercontent.com/o/r/main/a.ts', 'kkgithub'),
    'https://raw.kkgithub.com/o/r/main/a.ts',
  );
});

test('kkgithub 不覆盖 gist，原样返回', () => {
  const gist = 'https://gist.github.com/user/abc';
  assert.equal(mirrorGithubUrl(gist, 'kkgithub'), gist);
});

test('api.github.com 与非 GitHub 主机不改写', () => {
  const api = 'https://api.github.com/repos/o/r/releases/latest';
  assert.equal(mirrorGithubUrl(api, 'ghfast'), api);
  const other = 'https://example.com/github.com/o/r';
  assert.equal(mirrorGithubUrl(other, 'kkgithub'), other);
});

test('前缀镜像同样保留 query 与 hash', () => {
  const url = 'https://github.com/o/r?tab=readme#install';
  assert.equal(mirrorGithubUrl(url, 'ghfast'), `https://ghfast.top/${url}`);
});

test('非法输入原样返回', () => {
  assert.equal(mirrorGithubUrl('', 'ghfast'), '');
  assert.equal(mirrorGithubUrl('not a url', 'kkgithub'), 'not a url');
  assert.equal(
    mirrorGithubUrl('mailto:someone@example.com', 'ghfast'),
    'mailto:someone@example.com',
  );
});

test('仓库主页只经能呈现网页的镜像，其余直连', () => {
  // ghfast 对仓库页回 403 `Invalid input.`；moeyy 没有验证过能代理 HTML。
  assert.equal(projectHomeUrl('direct'), PROJECT_GITHUB_URL);
  assert.equal(projectHomeUrl('ghfast'), PROJECT_GITHUB_URL);
  assert.equal(projectHomeUrl('moeyy'), PROJECT_GITHUB_URL);
  assert.equal(projectHomeUrl('ghproxy'), `https://gh-proxy.com/${PROJECT_GITHUB_URL}`);
  assert.equal(projectHomeUrl('kkgithub'), 'https://kkgithub.com/Nnyjk/simple-post');
});

test('resolveGithubMirror 合法值通过、非法值回落 direct', () => {
  for (const id of GITHUB_MIRROR_IDS) assert.equal(resolveGithubMirror(id), id);
  assert.equal(resolveGithubMirror('nope'), 'direct');
  assert.equal(resolveGithubMirror(''), 'direct');
  assert.equal(resolveGithubMirror(null), 'direct');
  assert.equal(resolveGithubMirror(undefined), 'direct');
});

test('每个镜像 id 都是可用的字面量', () => {
  const ids: GithubMirrorId[] = ['direct', 'ghfast', 'ghproxy', 'kkgithub', 'moeyy'];
  assert.deepEqual([...GITHUB_MIRROR_IDS], ids);
});
