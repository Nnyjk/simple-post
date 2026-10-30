/**
 * 「跟随模块」那一行显示的是环境里那一格的地址。
 *
 * 端点处在「自定义」档时，已解析的地址是端点自己的 `baseUrlOverride`。拿它填那一行，
 * 用户看到的就是「切回跟随模块会用这个地址」，而真正会用的是环境里「环境 × 模块」
 * 那一格 —— 显示与做法不一致。
 *
 * 渲染走 `react-dom/server`；本仓库没有 DOM 测试环境（没有 Vitest / RTL，见 AGENTS.md），
 * 且下拉那两行不用任何 hook。
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { readFileSync } from 'node:fs';

import { BaseUrlPickerRows, moduleRowSecondary } from '../BaseUrlPicker';

const MODULE_URL = 'http://svc.internal:8080/api/a';
const OVERRIDE_URL = 'https://custom.example.com';

function render(moduleBaseUrl: string | null): string {
  return renderToStaticMarkup(
    createElement(BaseUrlPickerRows, {
      selection: { kind: 'override' },
      moduleBaseUrl,
      onPickModule: () => {},
      onPickOverride: () => {},
    }),
  );
}

describe('「跟随模块」那一行', () => {
  it('显示环境里那一格的地址，而不是端点自己的地址', () => {
    const html = render(MODULE_URL);
    assert.match(html, /跟随模块/, '下拉里要有「跟随模块」那一行');
    assert.ok(html.includes(MODULE_URL), '「跟随模块」那一行必须显示环境里那一格的地址');
    assert.ok(!html.includes(OVERRIDE_URL), '「跟随模块」那一行不能显示端点自己的地址');
  });

  it('那一格没有地址时给一句提示，而不是显示端点自己的地址', () => {
    const html = render(null);
    assert.match(html, /环境里本模块没有配地址/, '没有那一格时要说明为什么没有地址');
    assert.ok(!html.includes(OVERRIDE_URL), '提示里不能出现端点自己的地址');
  });

  it('副标题的两种形态', () => {
    assert.equal(moduleRowSecondary(MODULE_URL), MODULE_URL);
    assert.equal(moduleRowSecondary(null), '环境里本模块没有配地址');
    assert.equal(moduleRowSecondary(''), '环境里本模块没有配地址');
  });
});

describe('BaseUrlPicker 把环境里那一格的地址交给那两行', () => {
  // 上面那几条只核对了「那两行拿到什么就显示什么」。这里核对的是「BaseUrlPicker 交给
  // 那两行的到底是什么」——把已解析的地址（自定义档下是端点自己的地址）交给它们，
  // 上面那几条照样全绿，而界面上显示的仍是错的。读源码而不跑界面：这条不变式说的是
  // 接线，源码里一眼可见。
  const src = readFileSync(new URL('../BaseUrlPicker.tsx', import.meta.url), 'utf8');

  it('那两行拿到的是环境里那一格的地址', () => {
    assert.ok(
      src.includes('moduleBaseUrl={moduleBaseUrl}'),
      'BaseUrlPicker 必须把「环境里那一格的地址」交给那两行',
    );
  });

  it('那两行拿到的不是端点自己的已解析地址', () => {
    assert.ok(
      !src.includes('moduleBaseUrl={resolvedBaseUrl}'),
      '那两行不能拿到已解析的地址：自定义档下它是端点自己的地址',
    );
  });
});
