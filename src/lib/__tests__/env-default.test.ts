/**
 * 默认环境的挑选规则。
 *
 * 默认环境 = 一个项目里**最早创建**的那个环境。它是项目的属性：谁发请求时没点名
 * 环境，就用它。后端按 `created_at ASC, id ASC LIMIT 1` 取它，故这里的排序规则必须
 * 与后端一致 —— 两边给出的默认环境不同，则界面预览的请求与真正发出去的请求不是同一个。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import { pickCurrentEnvironment, pickDefaultEnvironment } from '../env-default';
import type { Environment } from '@/types/domain';

function env(id: string, projectId: string, createdAt: number): Environment {
  return {
    id,
    projectId,
    name: id,
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    createdAt,
  };
}

const E_DEV = env('e-dev', 'p1', 100);
const E_STAGING = env('e-staging', 'p1', 200);
const E_OTHER = env('e-other', 'p2', 50);

describe('默认环境 = 最早创建的那个', () => {
  it('按创建时间取，不按名字取', () => {
    const picked = pickDefaultEnvironment([E_STAGING, E_DEV], 'p1');
    assert.equal(picked?.id, 'e-dev', 'dev 先创建，故它是默认环境');
  });

  it('只看本项目的环境', () => {
    const picked = pickDefaultEnvironment([E_OTHER, E_DEV, E_STAGING], 'p1');
    assert.equal(picked?.id, 'e-dev', '别的项目的环境不参与');
  });

  it('项目没有环境时没有默认环境', () => {
    assert.equal(pickDefaultEnvironment([E_OTHER], 'p1'), undefined);
  });

  it('同一毫秒里创建的两个环境按 id 决胜', () => {
    const a = env('a', 'p1', 100);
    const b = env('b', 'p1', 100);
    assert.equal(pickDefaultEnvironment([b, a], 'p1')?.id, 'a', 'id 小的那一个');
    assert.equal(pickDefaultEnvironment([a, b], 'p1')?.id, 'a', '与传入顺序无关');
  });
});

describe('当前环境：记住的那个优先，其次默认环境', () => {
  it('记住的那个还在就用它', () => {
    const picked = pickCurrentEnvironment([E_DEV, E_STAGING], 'p1', 'e-staging');
    assert.equal(picked?.id, 'e-staging');
  });

  it('记住的那个不在了就用默认环境', () => {
    const picked = pickCurrentEnvironment([E_DEV, E_STAGING], 'p1', 'e-gone');
    assert.equal(picked?.id, 'e-dev');
  });

  it('记住的那个属于别的项目时不算数', () => {
    const picked = pickCurrentEnvironment([E_DEV, E_STAGING], 'p1', 'e-other');
    assert.equal(picked?.id, 'e-dev');
  });

  it('没记住过时用默认环境', () => {
    const picked = pickCurrentEnvironment([E_DEV, E_STAGING], 'p1', '');
    assert.equal(picked?.id, 'e-dev');
  });

  it('项目没有环境时没有当前环境', () => {
    assert.equal(pickCurrentEnvironment([], 'p1', 'e-dev'), undefined);
  });
});
