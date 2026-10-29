/**
 * 设置 → 环境 分区的冷启动守卫。
 *
 * 这里要守的不变式是「新增控件存在」：`新建环境` 按钮只由 `EnvSwitcher` 承载，故环境
 * 条在任何情况下都必须在场，包括零环境的新项目 —— 否则用户连创建第一个环境都做不到。
 * 它只能通过用户真正看到的那棵树来断言，故这些断言刻意针对零环境情形的渲染输出。
 *
 * 渲染走 `react-dom/server` 的静态标记。本仓库没有 DOM 测试环境（没有 Vitest / RTL，
 * 见 AGENTS.md），但 `renderToStaticMarkup` 只需要一张模块图，而该分区的每个组件在模块
 * 作用域就已把 portal 收口（`Dialog` 在 `!open` 时返回 null），故可以无头服务端渲染。
 * 交互（点 新建环境、提交改名）由主会话的端到端流程覆盖，这里不假装测它们。
 *
 * 运行：
 *   npx tsx --test src/features/settings/sections/__tests__/environment-section.test.ts
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { EnvironmentSection, nextEnvironmentName } from '../EnvironmentSection';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { EnvBody } from '../rows/EnvBody';
import { EnvSwitcher } from '../rows/EnvSwitcher';
import type { Environment } from '@/types/domain';

/** 最小环境 —— 该分区只读 id / name / isActive 以及那几个 map。 */
function makeEnv(overrides: Partial<Environment> = {}): Environment {
  return {
    id: 'env-1',
    projectId: 'proj-1',
    name: 'dev',
    baseUrls: {},
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    isActive: true,
    ...overrides,
  };
}

function makeProps(overrides: Partial<EnvironmentSectionProps> = {}): EnvironmentSectionProps {
  return {
    environments: [],
    projectId: 'proj-1',
    modules: [],
    setEnvironmentVar: () => {},
    setEnvironmentVarDisabled: () => {},
    renameEnvironmentVar: () => {},
    setEnvironmentModuleBaseUrl: () => {},
    addEnvironment: async () => 'env-new',
    deleteEnvironment: () => {},
    updateEnvironment: () => {},
    baseUrlDefinitions: [],
    defaultBaseUrlDefinitionId: null,
    addBaseUrlDefinition: () => 'def-1',
    updateBaseUrlDefinition: () => {},
    deleteBaseUrlDefinition: () => {},
    setDefaultBaseUrlDefinition: () => {},
    ...overrides,
  };
}

function render(props: EnvironmentSectionProps): string {
  return renderToStaticMarkup(createElement(EnvironmentSection, props));
}

describe('nextEnvironmentName', () => {
  it('names the first environment in the panel voice', () => {
    assert.equal(nextEnvironmentName([]), '新环境');
  });

  it('derives a free name from the names already taken', () => {
    // 名字必须是拿已占用的名字推出来的固定写法，否则第二、第三个环境跟第一个分不出来。
    assert.equal(nextEnvironmentName(['新环境']), '新环境 2');
    assert.equal(nextEnvironmentName(['新环境', '新环境 2']), '新环境 3');
  });

  it('reuses the lowest free index rather than counting ever-created envs', () => {
    assert.equal(nextEnvironmentName(['新环境', '新环境 3']), '新环境 2');
  });

  it('compares trimmed names so a stray space cannot produce a duplicate', () => {
    assert.equal(nextEnvironmentName(['  新环境  ']), '新环境 2');
  });

  it('leaves hand-named environments alone', () => {
    assert.equal(nextEnvironmentName(['dev', 'staging', 'prod']), '新环境');
  });
});

describe('EnvironmentSection cold start (zero environments)', () => {
  const html = render(makeProps());

  it('still renders the 新建环境 add control', () => {
    // 一条断言守住这条回归：一个环境都没有，但分区里唯一的添加入口在场且可用。
    assert.match(html, /新建环境/);
    assert.doesNotMatch(html, /disabled=""/);
  });

  it('keeps the empty state helpful and points at that control', () => {
    assert.match(html, /该项目还没有环境/);
    assert.match(html, /新建环境/);
  });
});

describe('EnvironmentSection with a single environment', () => {
  // 容易回归的情形：新项目的第一个环境，故环境条必须能承受离开那个条件渲染。
  //
  // 注意这里断言不了什么：环境*正文*。该分区在一个 `useEffect` 里解析激活环境，而静态
  // 标记不跑 effect，故此刻 `activeId` 仍是 `null`。这正是重命名入口改在下面经
  // `EnvSwitcher` 检查的原因。
  const html = render(makeProps({ environments: [makeEnv()] }));

  it('drops the empty hint', () => {
    assert.doesNotMatch(html, /该项目还没有环境/);
  });

  it('still exposes the add control for a second environment', () => {
    assert.match(html, /新建环境/);
  });

  it('shows the env name in the tab strip', () => {
    assert.match(html, />dev</);
  });
});

describe('EnvSwitcher — the env name lives here, and so does its rename', () => {
  // 这条选择带是名字唯一出现的地方，故重命名必须跟着搬过来，否则分区会悄悄失去改名能力。
  const props = {
    environments: [makeEnv()],
    activeId: 'env-1',
    onPick: () => {},
    onRequestDelete: () => {},
    onAdd: () => {},
  };
  const renameable = renderToStaticMarkup(
    createElement(EnvSwitcher, { ...props, onRename: () => {} }),
  );

  it('renders the active chip as a click-to-edit cell, not static text', () => {
    // `InlineNameCell` 关闭时渲染按钮、打开时渲染输入框，故那个按钮正能证明：在只有
    // 一个环境的项目里，改名仍是一次点击的事。
    assert.match(renameable, /<button/);
    assert.match(renameable, />dev</);
    assert.match(renameable, /lucide-pencil/);
  });

  it('exposes no input before the cell is opened', () => {
    assert.doesNotMatch(renameable, /<input/);
  });

  it('keeps the chip a tab for assistive tech after the swap away from <button>', () => {
    // 名称单元格本身是个按钮，故 chip 不能再是按钮。这三条断言的是 chip 替 `<button>`
    // 补齐的语义：角色、可选中状态与可聚焦。
    assert.match(renameable, /role="tab"/);
    assert.match(renameable, /aria-selected="true"/);
    assert.match(renameable, /tabindex="0"/);
  });

  it('drops the rename affordance when the host does not offer it', () => {
    // 模块层不传 `onRename`：在模块标签里改一个项目级的环境不是它该管的事。
    const readOnly = renderToStaticMarkup(createElement(EnvSwitcher, props));
    assert.match(readOnly, />dev</);
    assert.doesNotMatch(readOnly, /lucide-pencil/);
  });
});

describe('EnvBody — no name row of its own', () => {
  const p = makeProps();
  const html = renderToStaticMarkup(
    createElement(EnvBody, {
      env: makeEnv(),
      projectId: p.projectId,
      modules: p.modules,
      setEnvironmentVar: p.setEnvironmentVar,
      setEnvironmentVarDisabled: p.setEnvironmentVarDisabled,
      renameEnvironmentVar: p.renameEnvironmentVar,
      setEnvironmentModuleBaseUrl: p.setEnvironmentModuleBaseUrl,
      onAddVar: () => {},
      baseUrlDefinitions: p.baseUrlDefinitions,
      defaultBaseUrlDefinitionId: p.defaultBaseUrlDefinitionId,
      addBaseUrlDefinition: p.addBaseUrlDefinition,
      updateBaseUrlDefinition: p.updateBaseUrlDefinition,
      setDefaultBaseUrlDefinition: p.setDefaultBaseUrlDefinition,
      onDeleteBaseUrl: () => {},
    }),
  );

  it('never spells the env name, which the switcher directly above already says', () => {
    // 正文曾以自己的一行图标 + 名字开头，于是激活环境在几厘米内被叫了两次；而模块层
    // 从来没有那一行，反倒只有它是唯一读起来像一回事的视图。
    assert.doesNotMatch(html, />dev</);
  });

  it('still renders the groups that belong to the env', () => {
    assert.match(html, /前置 URL/);
    assert.match(html, /环境变量/);
  });
});

describe('EnvironmentSection with several environments', () => {
  const html = render(
    makeProps({
      environments: [
        makeEnv(),
        makeEnv({ id: 'env-2', name: 'prod', isActive: false }),
        makeEnv({ id: 'env-3', name: 'staging', isActive: false }),
      ],
    }),
  );

  it('renders every tab plus the add control', () => {
    for (const name of ['dev', 'prod', 'staging']) assert.match(html, new RegExp(`>${name}<`));
    assert.match(html, /新建环境/);
  });
});
