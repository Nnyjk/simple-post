/**
 * `crud-slice` 各分片共用的切片级夹具。**不是**测试文件（不匹配 `*.test.ts`，
 * 故不被 `scripts/run-tests.mjs` 发现）。
 *
 * 每个测试文件是 node:test 的独立进程，故这里只提供工厂与探针安装/还原。
 */
import { beforeEach, afterEach } from 'node:test';

import { installBrowserShim, clearIpcMock } from './_browser-shim';

import { createCrudSlice } from '../crud-slice';
import type { EndpointDetail, Environment, Project, ResourceNode } from '@/types/domain';

/** IPC mock 捕获到的一次调用。 */
export type CapturedInvoke = { cmd: string; args: Record<string, unknown> };

/** 一个最小但有领域形状的 `EndpointDetail`。 */
export function makeDetail(overrides: Partial<EndpointDetail> = {}): EndpointDetail {
  return {
    nodeId: 'ep-1',
    method: 'GET',
    url: '/widgets',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    tags: [],
    ...overrides,
  };
}

/** 用于环境值增删 action 的最小环境数据。 */
export function makeEnvironment(overrides: Partial<Environment> = {}): Environment {
  return {
    id: 'env-1',
    projectId: 'project-1',
    name: 'dev',
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    isActive: true,
    ...overrides,
  };
}

/** 一个项目。地址住在环境里的「模块 × 环境」那一格，故项目行上没有地址。 */
export function makeProject(overrides: Partial<Project> = {}): Project {
  return {
    id: 'project-1',
    name: '项目',
    color: '#000000',
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

/**
 * 构造 CRUD slice 所需的最小 store；环境 action 读取 environments，
 * endpoint action 读取 endpointDetails，project action 读取 projects，
 * 二者共用 Zustand 风格的 setter。
 */
type CrudHarnessState = {
  nodes: ResourceNode[];
  endpointDetails: Record<string, EndpointDetail>;
  environments: Environment[];
  projects: Project[];
  openTabs: never[];
  activeTabId: null;
  activeEndpointId: null;
};

export function makeCrudHarness(
  initialDetails: Record<string, EndpointDetail> = {},
  initialEnvironments: Environment[] = [],
  initialNodes: ResourceNode[] = [],
  initialProjects: Project[] = [],
) {
  const bag: CrudHarnessState = {
    nodes: initialNodes,
    endpointDetails: initialDetails,
    environments: initialEnvironments,
    projects: initialProjects,
    openTabs: [],
    activeTabId: null,
    activeEndpointId: null,
  };
  const set = (partial: unknown) => {
    const updater =
      typeof partial === 'function'
        ? (partial as (state: CrudHarnessState) => unknown)(bag)
        : partial;
    if (updater && typeof updater === 'object') Object.assign(bag, updater);
  };
  const get = () => bag;
  const slice = createCrudSlice(set as never, get as never, {} as never);
  return { bag, slice };
}

export let warnings: string[] = [];

let origWarn: typeof console.warn;
let origError: typeof console.error;

export function resetWarnings(): void {
  warnings = [];
}

export function readWarnings(): string[] {
  return warnings;
}

beforeEach(() => {
  installBrowserShim();
  // 在 console.warn / console.error 上装探针，好让我们能断言回滚分支
  // 记下了 IPC 失败（生产行为）。
  origWarn = console.warn.bind(console);
  origError = console.error.bind(console);
  resetWarnings();
  const fmt = (a: unknown) => {
    if (typeof a === 'string') return a;
    if (a instanceof Error) return `${a.name}: ${a.message}`;
    try {
      return JSON.stringify(a);
    } catch {
      return String(a);
    }
  };
  console.warn = (...args: unknown[]) => {
    warnings.push(args.map(fmt).join(' '));
  };
  console.error = (...args: unknown[]) => {
    warnings.push(args.map(fmt).join(' '));
  };
});

afterEach(() => {
  console.warn = origWarn;
  console.error = origError;
  clearIpcMock();
});
