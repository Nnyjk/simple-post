/**
 * 端点编辑器「证书」那一页的渲染结果。
 *
 * 这一页管的是**端点自己那份** HTTPS 出站设置。合成语义是整体取代：端点给了这一份
 * 就顶掉全局默认那份，故界面上只有一个「继承全局设置」开关加四个字段，没有逐项覆盖
 * 的中间态。
 *
 * 这里钉住两件事：
 *
 *   1. 继承全局时，四个字段是禁用的且显示全局默认那份的值。
 *   2. 不继承时，四个字段可编辑且显示端点自己那份的值。
 *
 * 面板的取值全部来自 props，故这里用服务端渲染（`renderToStaticMarkup`）就能把两种
 * 状态都渲染出来，不必依赖浏览器。
 */
import { describe, it, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { installBrowserShim } from '@/stores/__tests__/_browser-shim';
import type { TlsSettings } from '@/types/domain';
import { TlsPanel } from '../TlsPanel';

const shim = installBrowserShim();

/** 全局默认那份：设置面板里改的那一份。 */
const GLOBAL: TlsSettings = {
  skipVerify: false,
  caCertificatePath: null,
  clientCertificatePath: null,
  clientKeyPath: null,
};

/** 端点自己那份。 */
const OWN: TlsSettings = {
  skipVerify: true,
  caCertificatePath: 'C:\\certs\\ca.pem',
  clientCertificatePath: 'C:\\certs\\client.pem',
  clientKeyPath: null,
};

afterEach(() => {
  shim.teardown();
});

/** 渲染一次面板，返回它的静态标记。 */
function render(value: TlsSettings | null, globalSettings: TlsSettings = GLOBAL): string {
  return renderToStaticMarkup(
    createElement(TlsPanel, { value, globalSettings, onChange: () => {} }),
  );
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

/**
 * 标签里有没有 `disabled` **属性**。
 *
 * 不能用 `includes('disabled')` 判定：`Switch` 与 `Input` 的 class 名里都写着
 * `disabled:cursor-not-allowed`，那个字符串包含 `disabled`，让「没禁用」也被判成「禁用了」。
 * React 把布尔属性渲染成 `disabled=""`，故按这个形状判定。
 */
function isDisabled(tag: string): boolean {
  return tag.includes('disabled=""');
}

describe('端点编辑器：证书那一页', () => {
  it('继承全局时开关是开的，四个字段禁用', () => {
    const html = render(null);

    assert.ok(switchTag(html, '继承全局设置').includes('aria-checked="true"'));
    assert.ok(isDisabled(switchTag(html, '跳过证书校验')), '继承全局时「跳过证书校验」不可改');
    for (const label of ['自定义 CA 证书', '客户端证书', '客户端私钥']) {
      assert.ok(isDisabled(inputTag(html, label)), `${label} 在继承全局时不可改`);
    }
  });

  it('继承全局时显示的是全局默认那份的值', () => {
    const html = render(null, {
      ...GLOBAL,
      caCertificatePath: 'C:\\certs\\global-ca.pem',
      skipVerify: true,
    });

    assert.ok(inputTag(html, '自定义 CA 证书').includes('value="C:\\certs\\global-ca.pem"'));
    assert.ok(switchTag(html, '跳过证书校验').includes('aria-checked="true"'));
  });

  it('不继承时开关是关的，四个字段可编辑并显示端点自己那份的值', () => {
    const html = render(OWN);

    assert.ok(switchTag(html, '继承全局设置').includes('aria-checked="false"'));
    assert.ok(!isDisabled(switchTag(html, '跳过证书校验')), '不继承全局时「跳过证书校验」可改');
    assert.ok(inputTag(html, '自定义 CA 证书').includes('value="C:\\certs\\ca.pem"'));
    assert.ok(inputTag(html, '客户端证书').includes('value="C:\\certs\\client.pem"'));
    assert.ok(inputTag(html, '客户端私钥').includes('value=""'), '没填的路径是空串');
    for (const label of ['自定义 CA 证书', '客户端证书', '客户端私钥']) {
      assert.ok(!isDisabled(inputTag(html, label)), `${label} 在不继承全局时可改`);
    }
  });

  it('跳过校验打开时写着中间人能截获并改写请求', () => {
    const html = render(OWN);
    assert.ok(html.includes('任何中间人都能截获并改写这次请求'));
  });

  it('说清证书只存路径、不存内容', () => {
    const html = render(OWN);
    assert.ok(html.includes('证书只存文件路径，不存内容'));
  });

  it('说清关掉继承之后这份设置整体顶掉全局默认', () => {
    const html = render(null);
    assert.ok(html.includes('整体顶掉全局默认'));
  });
});
