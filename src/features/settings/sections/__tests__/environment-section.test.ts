/**
 * Cold-start guards for the settings → environment section.
 *
 * The bug this file exists for: the whole header block was wrapped in
 * `environments.length === 0 ? <EmptyHint/> : <EnvSwitcher …/>`, and
 * `EnvSwitcher` is the only component carrying the `新建环境` button.
 * A project with zero environments therefore rendered a bare hint
 * string and *no way to create its first environment* — a dead end that
 * no unit test noticed, because there was no test and nothing in the
 * store changed shape.
 *
 * So these assertions are deliberately about the rendered output of the
 * zero-env case, not about a helper: the invariant that matters is
 * "the add control exists", and that can only be checked through the
 * tree the user actually sees.
 *
 * Rendering goes through `react-dom/server`'s static markup. The repo
 * has no DOM test harness (no Vitest / RTL — see AGENTS.md), but
 * `renderToStaticMarkup` needs nothing but a module graph, and every
 * component in this section is portal-closed at module scope
 * (`Dialog` returns null when `!open`), so the section server-renders
 * headless. Interactions (clicking 新建环境, committing a rename) are
 * covered by the main session's end-to-end pass; nothing here pretends
 * to test them.
 *
 * Run via:
 *   npx tsx --test src/features/settings/sections/__tests__/environment-section.test.ts
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { EnvironmentSection, nextEnvironmentName } from '../EnvironmentSection';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import { EnvHeader } from '../rows/EnvHeader';
import type { Environment } from '@/types/domain';

/** Minimal env — the section only reads id / name / isActive / the maps. */
function makeEnv(overrides: Partial<Environment> = {}): Environment {
  return {
    id: 'env-1',
    projectId: 'proj-1',
    name: 'dev',
    baseUrls: {},
    moduleBaseUrls: {},
    variables: {},
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
    // The old hardcoded literal made every env read `new-env`, so the
    // second and third were indistinguishable from the first.
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
    // The regression in one assertion: no env, but the only add
    // affordance in the section is present and enabled.
    assert.match(html, /新建环境/);
    assert.doesNotMatch(html, /disabled=""/);
  });

  it('keeps the empty state helpful and points at that control', () => {
    assert.match(html, /该项目还没有环境/);
    assert.match(html, /新建环境/);
  });
});

describe('EnvironmentSection with a single environment', () => {
  // The case that regresses easily: it is the first env of a fresh
  // project, so the tab strip must survive the move away from the
  // conditional.
  //
  // Note what this cannot assert: the env *body*. The section resolves
  // its active env in a `useEffect`, and static markup runs no effects,
  // so `activeId` is still `null` here. That is why the rename
  // affordance is checked through `EnvHeader` below instead.
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

describe('EnvHeader — inline rename for a project’s only environment', () => {
  const html = renderToStaticMarkup(
    createElement(EnvHeader, { env: makeEnv(), onRename: () => {} }),
  );

  it('renders the name as a click-to-edit cell, not static text', () => {
    // `InlineNameCell` renders a button when closed and an input when
    // open, so the button is what proves rename is still one click away
    // for the first env of a single-env project.
    assert.match(html, /<button/);
    assert.match(html, />dev</);
  });

  it('exposes no input before the cell is opened', () => {
    assert.doesNotMatch(html, /<input/);
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
