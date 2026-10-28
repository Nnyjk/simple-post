/**
 * 设置面板「请求行为」小节里的 HTTPS 出站设置（**全局默认**）。
 *
 * 这一组存在后端 `preferences` 表的 `request_settings` 行里，MCP 侧发请求读的是同一
 * 行，故它是全局的那一份，端点可以在自己的页签里整体顶掉它。
 *
 * 这里把渲染结果钉住，钉的是**用户看得见的文案与控件状态**：分组副标题、四项设置、
 * 跳过校验的警示、证书只存路径那句说明、页脚那句提醒。
 *
 * 用服务端渲染（`renderToStaticMarkup`）而不是浏览器：这段界面没有交互状态，静态
 * 标记已经足够说明它长什么样，且不依赖一个跑不起来的 dev server。注意 Zustand v5
 * 的服务端快照取的是**初始状态**，所以这里渲染出来的永远是缺省值那一版——「改动之后
 * 长什么样」由 `isRequestSettingsDirty` 与端点那侧的用例分别钉住。
 */
import { describe, it, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { installBrowserShim } from '@/stores/__tests__/_browser-shim';
import { SkipVerifyWarning } from '@/components/tls/skip-verify-warning';
import { useAppStore } from '@/stores/app-store';
import { REQUEST_SETTINGS_DEFAULTS } from '@/stores/slices/ui-prefs';
import { isRequestSettingsDirty, RequestSection } from '../RequestSection';

const shim = installBrowserShim();

afterEach(() => {
  // 渲染时会读 store 里的请求设置，故每个用例之后把它放回缺省值。
  useAppStore.setState({ requestSettings: REQUEST_SETTINGS_DEFAULTS });
  shim.teardown();
});

/** 渲染一次「请求行为」小节，返回它的静态标记。 */
function render(): string {
  return renderToStaticMarkup(createElement(RequestSection));
}

/** 渲染一次那条中间人警示，返回它的静态标记。 */
function renderWarning(skipVerify: boolean): string {
  return renderToStaticMarkup(createElement(SkipVerifyWarning, { skipVerify }));
}

/** 取出带某个 aria-label 的开关的标签。 */
function switchTag(html: string, ariaLabel: string): string {
  const match = html.match(new RegExp(`<button[^>]*aria-label="${ariaLabel}"[^>]*>`));
  assert.ok(match, `期望渲染出 aria-label 为「${ariaLabel}」的开关`);
  return match![0];
}

/** 取出带某个 aria-label 的输入框的标签。 */
function inputTag(html: string, ariaLabel: string): string {
  const match = html.match(new RegExp(`<input[^>]*aria-label="${ariaLabel}"[^>]*>`));
  assert.ok(match, `期望渲染出 aria-label 为「${ariaLabel}」的输入框`);
  return match![0];
}

/** 取出「恢复默认」按钮的起始标签。 */
function restoreButtonTag(html: string): string {
  const idx = html.indexOf('恢复默认');
  assert.ok(idx >= 0, '期望渲染出「恢复默认」按钮');
  const start = html.lastIndexOf('<button', idx);
  return html.slice(start, html.indexOf('>', start) + 1);
}

describe('设置面板：请求行为 · HTTPS 出站（全局默认）', () => {
  it('渲染出分组副标题与四项设置', () => {
    const html = render();

    assert.ok(html.includes('HTTPS 出站'), '要有把 HTTPS 那一组与出站旋钮分开的副标题');
    assert.ok(html.includes('跳过证书校验'));
    assert.ok(html.includes('自定义 CA 证书'));
    assert.ok(html.includes('客户端证书'));
    assert.ok(html.includes('客户端私钥'));
    // 三个路径输入框都在，且各自带着自己的标签。
    for (const label of ['自定义 CA 证书', '客户端证书', '客户端私钥']) {
      assert.ok(inputTag(html, label).includes('<input'), '路径输入框要真的渲染出来');
    }
  });

  it('跳过校验关着时不显示那条中间人警示', () => {
    const html = render();

    assert.ok(
      !html.includes('任何中间人都能截获并改写这次请求'),
      '开关关着时面板不该断言一个并不存在的中间人风险 —— 端点那侧也是关着就不显示',
    );
    assert.ok(
      !renderWarning(false).includes('任何中间人都能截获并改写这次请求'),
      '警示组件在开关关着时不渲染任何文字',
    );
  });

  it('跳过校验打开时显示那条中间人警示', () => {
    // 这一条渲染的是警示组件本身，而不是整个设置小节：Zustand v5 的服务端渲染取的是
    // store 的**初始状态**，`useAppStore.setState` 改不动它，故从设置小节这一层渲染出
    // 「开关打开」那一版在服务端渲染里做不到。组件不读 store，故它两种状态都能渲染。
    const html = renderWarning(true);

    assert.ok(
      html.includes('任何中间人都能截获并改写这次请求'),
      '警示必须说清打开这个开关之后会发生什么',
    );
  });

  it('说清证书只存路径、不存内容', () => {
    const html = render();
    assert.ok(html.includes('证书只存文件路径，不存内容'));
  });

  it('页脚说清这些设置何时生效，以及 MCP 侧也受它影响', () => {
    const html = render();
    assert.ok(html.includes('仅在真实请求模式下生效'));
    assert.ok(html.includes('MCP'), '页脚要点明 MCP 侧发请求也受这一组影响');
  });

  it('缺省值下跳过校验是关的、路径是空的、恢复默认不可用', () => {
    const html = render();

    assert.ok(switchTag(html, '跳过证书校验').includes('aria-checked="false"'));
    assert.ok(inputTag(html, '自定义 CA 证书').includes('value=""'));
    assert.ok(restoreButtonTag(html).includes('disabled'), '什么都没改过时不该引诱用户去重置');
  });
});

describe('设置面板：「恢复默认」把 TLS 那一组也算进去', () => {
  it('缺省值不算已改动', () => {
    assert.equal(isRequestSettingsDirty(REQUEST_SETTINGS_DEFAULTS), false);
  });

  it('改「跳过证书校验」算已改动', () => {
    const settings = {
      ...REQUEST_SETTINGS_DEFAULTS,
      tls: { ...REQUEST_SETTINGS_DEFAULTS.tls, skipVerify: true },
    };
    assert.equal(isRequestSettingsDirty(settings), true);
  });

  it('改任何一个证书路径都算已改动', () => {
    for (const key of ['caCertificatePath', 'clientCertificatePath', 'clientKeyPath'] as const) {
      const settings = {
        ...REQUEST_SETTINGS_DEFAULTS,
        tls: { ...REQUEST_SETTINGS_DEFAULTS.tls, [key]: 'C:\\certs\\x.pem' },
      };
      assert.equal(isRequestSettingsDirty(settings), true, `${key} 改了就算已改动`);
    }
  });

  it('三个出站旋钮改了也算已改动', () => {
    assert.equal(isRequestSettingsDirty({ ...REQUEST_SETTINGS_DEFAULTS, timeoutMs: 1000 }), true);
    assert.equal(
      isRequestSettingsDirty({ ...REQUEST_SETTINGS_DEFAULTS, followRedirects: false }),
      true,
    );
    assert.equal(
      isRequestSettingsDirty({ ...REQUEST_SETTINGS_DEFAULTS, maxResponseSizeKb: 1 }),
      true,
    );
  });
});
