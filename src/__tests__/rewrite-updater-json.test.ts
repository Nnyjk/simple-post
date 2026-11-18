/**
 * latest.json 里每个平台的 url 必须是 release 的下载地址。
 *
 * `releaseDraft: true` 时 tauri-action 写进去的是 `https://api.github.com/repos/<owner>/<repo>/releases/assets/<id>`，
 * 那个端点是 API 端点，不是下载端点：它要求请求带 `User-Agent`，不带时返回
 * `403 Forbidden`（`Request forbidden by administrative rules`），而 updater 用的 reqwest
 * 默认不发 `User-Agent`，所以 0.1.35 → 0.1.36 的应用内更新拿到 `403`。故发布后由
 * `scripts/rewrite-updater-json.mjs` 换成资产的 `browser_download_url` —— 这两个
 * 用例钉住那次改写的判据与「除 url 之外一个字段都不动」。
 *
 * 样本是 0.1.36 线上 latest.json 的原样片段（signature 与 notes 都是真的）。
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { rewriteUpdaterJson } from '../../scripts/rewrite-updater-json.mjs';
import type { Release, UpdaterJson } from '../../scripts/rewrite-updater-json.mjs';

/** 改写前的 latest.json：两条 windows 都指着同一个 API 资产端点。 */
const LATEST: UpdaterJson = {
  version: '0.1.36',
  notes:
    "### 这一版的改动\n<!-- 发布前替换本节：应用内「更新」对话框显示的就是这段正文 -->\n\n## Simple Post v0.1.36\n\nDesktop API client with built-in MCP for AI collaboration.\n\n### Install\n- **macOS**: open the `.dmg`, drag `simple-post.app` to Applications\n- **Ubuntu**: `sudo dpkg -i simple-post_*_amd64.deb` or `chmod +x simple-post_*_amd64.AppImage && ./simple-post_*_amd64.AppImage`\n- **Windows**: run the `simple-post_*_x64-setup.exe` NSIS installer\n\n### Container image\nA matching `linux/amd64` image is published\nto GitHub Container Registry by `.github/workflows/publish-image.yml`\non the same tag:\n\n```\n# GHCR stores the image under the lowercase path, and docker\n# refuses a reference whose path contains uppercase letters\n# (`invalid reference format: repository name must be\n# lowercase`). `github.repository` keeps the owner's original\n# case, so the pull command spells the published path out.\ndocker pull ghcr.io/nnyjk/simple-post:v0.1.36\n```\n\nThe same image is also tagged without the `v` (`0.1.36`), plus\n`:0.1` (rolling minor) and `:latest` (rolling latest).",
  pub_date: '2026-11-17T01:35:31.342Z',
  platforms: {
    'windows-x86_64': {
      signature:
        'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRV082L3JIOG42WDI2UGdsbGZHNkdGSk5RNEc2M1JQMnhTOFAwTzUxQnBmTWRZUEhVWFdLOVY1S0t0NGZ2L0h6bWV2SVh1eklRMUNDZHZPVk1CNllKSXVLRjhCT1JDTkFvPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0ODc4NzQyCWZpbGU6c2ltcGxlLXBvc3RfMC4xLjM2X3g2NC1zZXR1cC5leGUJdmVyc2lvbjowLjEuMzYKVElzM1ExQ2tLTXdNSVNSeERQTFZKZVp1OHhJZmlnYnlFdzRZMHNDK0xSMVBrc1ZCTFlhdC9hV051OURha1gwS3NjcXZyVWlObUd5cEhodFBtWldLQUE9PQo=',
      url: 'https://api.github.com/repos/Nnyjk/simple-post/releases/assets/757141760',
    },
    'windows-x86_64-nsis': {
      signature:
        'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRV082L3JIOG42WDI2UGdsbGZHNkdGSk5RNEc2M1JQMnhTOFAwTzUxQnBmTWRZUEhVWFdLOVY1S0t0NGZ2L0h6bWV2SVh1eklRMUNDZHZPVk1CNllKSXVLRjhCT1JDTkFvPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0ODc4NzQyCWZpbGU6c2ltcGxlLXBvc3RfMC4xLjM2X3g2NC1zZXR1cC5leGUJdmVyc2lvbjowLjEuMzYKVElzM1ExQ2tLTXdNSVNSeERQTFZKZVp1OHhJZmlnYnlFdzRZMHNDK0xSMVBrc1ZCTFlhdC9hV051OURha1gwS3NjcXZyVWlObUd5cEhodFBtWldLQUE9PQo=',
      url: 'https://api.github.com/repos/Nnyjk/simple-post/releases/assets/757141760',
    },
    'darwin-aarch64': {
      signature:
        'dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRV082L3JIOG42WDFybzBiRmFMR3daWVZadDAxOHY2ZGl1WWJSRkx2WDFWaFFOVmVkZG5ySDY1eVQvaFNET1RIdWxDYjlBZC9FejZkekhrWFk4bE4xVFB6SlhnY1N0K3d3PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0ODc5MzI1CWZpbGU6c2ltcGxlLXBvc3QuYXBwLnRhci5negl2ZXJzaW9uOjAuMS4zNgp6V2pGOWk0NHFSeXpDY2hOWEJxQlNJY2Eydkc1UisrczQxNzFxY25wZEY4emhMMzRDeFNpZlAzZm5Zd0FQZVd1ejBsSWtXRDNkblNlbExUc0h0bENDdz09Cg==',
      url: 'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_universal.app.tar.gz',
    },
  },
};

/** 同一个 release 的资产表（`gh api repos/Nnyjk/simple-post/releases/tags/v0.1.36`）。 */
const RELEASE: Release = {
  tag_name: 'v0.1.36',
  assets: [
    {
      id: 757145379,
      name: 'simple-post_0.1.36_amd64.AppImage',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_amd64.AppImage',
    },
    {
      id: 757145512,
      name: 'simple-post_0.1.36_amd64.AppImage.sig',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_amd64.AppImage.sig',
    },
    {
      id: 757145331,
      name: 'simple-post_0.1.36_amd64.deb',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_amd64.deb',
    },
    {
      id: 757145363,
      name: 'simple-post_0.1.36_amd64.deb.sig',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_amd64.deb.sig',
    },
    {
      id: 757165742,
      name: 'simple-post_0.1.36_universal.app.tar.gz',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_universal.app.tar.gz',
    },
    {
      id: 757165811,
      name: 'simple-post_0.1.36_universal.app.tar.gz.sig',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_universal.app.tar.gz.sig',
    },
    {
      id: 757165677,
      name: 'simple-post_0.1.36_universal.dmg',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_universal.dmg',
    },
    {
      id: 757141760,
      name: 'simple-post_0.1.36_x64-setup.exe',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_x64-setup.exe',
    },
    {
      id: 757141792,
      name: 'simple-post_0.1.36_x64-setup.exe.sig',
      browser_download_url:
        'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_x64-setup.exe.sig',
    },
  ],
};

const WINDOWS_SETUP_URL =
  'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_x64-setup.exe';

test('API 资产端点换成 browser_download_url', () => {
  const out = rewriteUpdaterJson(LATEST, RELEASE);

  // 两条 windows 平台指向同一个资产 id，故都换成同一个安装包地址。
  assert.equal(out.platforms['windows-x86_64'].url, WINDOWS_SETUP_URL);
  assert.equal(out.platforms['windows-x86_64-nsis'].url, WINDOWS_SETUP_URL);

  // 带末尾斜杠的端点按同一条规则改写。
  const trailing = rewriteUpdaterJson(
    {
      ...LATEST,
      platforms: {
        'windows-x86_64': {
          ...LATEST.platforms['windows-x86_64'],
          url: 'https://api.github.com/repos/Nnyjk/simple-post/releases/assets/757141760/',
        },
      },
    },
    RELEASE,
  );
  assert.equal(trailing.platforms['windows-x86_64'].url, WINDOWS_SETUP_URL);
});

test('version / notes / pub_date / signature 逐字未变', () => {
  const out = rewriteUpdaterJson(LATEST, RELEASE);

  assert.deepEqual(
    { version: out.version, notes: out.notes, pub_date: out.pub_date },
    { version: LATEST.version, notes: LATEST.notes, pub_date: LATEST.pub_date },
  );

  for (const key of ['windows-x86_64', 'windows-x86_64-nsis', 'darwin-aarch64']) {
    const { url: _before, ...before } = LATEST.platforms[key];
    const { url: _after, ...after } = out.platforms[key];
    assert.deepEqual(after, before, `${key} 的 signature 被改动了`);
  }
});

test('已是 github.com 下载地址的条目一字未变', () => {
  const out = rewriteUpdaterJson(LATEST, RELEASE);
  assert.deepEqual(out.platforms['darwin-aarch64'], LATEST.platforms['darwin-aarch64']);
});

test('跑两遍与跑一遍结果相同', () => {
  const once = rewriteUpdaterJson(LATEST, RELEASE);
  const twice = rewriteUpdaterJson(once, RELEASE);
  assert.deepEqual(twice, once);
  assert.equal(twice.platforms['windows-x86_64'].url, WINDOWS_SETUP_URL);
});

test('不修改入参', () => {
  rewriteUpdaterJson(LATEST, RELEASE);
  assert.equal(
    LATEST.platforms['windows-x86_64'].url,
    'https://api.github.com/repos/Nnyjk/simple-post/releases/assets/757141760',
  );
  assert.equal(
    LATEST.platforms['darwin-aarch64'].url,
    'https://github.com/Nnyjk/simple-post/releases/download/v0.1.36/simple-post_0.1.36_universal.app.tar.gz',
  );
});

test('资产的 browser_download_url 缺失时按 tag 与名字拼地址', () => {
  const withoutUrl: Release = {
    tag_name: 'v0.1.36',
    assets: [{ id: 757141760, name: 'simple-post_0.1.36_x64-setup.exe' }],
  };
  const out = rewriteUpdaterJson(LATEST, withoutUrl);
  assert.equal(out.platforms['windows-x86_64'].url, WINDOWS_SETUP_URL);
});

test('未知资产 id 抛错，信息里带平台键名与那个 url', () => {
  const unknownUrl = 'https://api.github.com/repos/Nnyjk/simple-post/releases/assets/999';
  assert.throws(
    () =>
      rewriteUpdaterJson(
        {
          ...LATEST,
          platforms: {
            'windows-x86_64': { ...LATEST.platforms['windows-x86_64'], url: unknownUrl },
          },
        },
        RELEASE,
      ),
    (err: unknown) => {
      assert.ok(err instanceof Error);
      assert.match(err.message, /windows-x86_64/);
      assert.ok(err.message.includes(unknownUrl), `错误信息里没有带上那个 url：${err.message}`);
      return true;
    },
  );
});

test('既不是 API 端点也不是 github.com 地址的 url 抛错', () => {
  const wrongUrl = 'https://example.com/simple-post_0.1.36_x64-setup.exe';
  assert.throws(
    () =>
      rewriteUpdaterJson(
        {
          ...LATEST,
          platforms: {
            'windows-x86_64': { ...LATEST.platforms['windows-x86_64'], url: wrongUrl },
          },
        },
        RELEASE,
      ),
    (err: unknown) => {
      assert.ok(err instanceof Error);
      assert.match(err.message, /windows-x86_64/);
      assert.ok(err.message.includes(wrongUrl), `错误信息里没有带上那个 url：${err.message}`);
      return true;
    },
  );
});
