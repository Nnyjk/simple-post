import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { buildEndpointRequestParts, prepareEndpointRequest } from '../model/endpointParts';
import type { EndpointDetail, Environment } from '@/types/domain';
import type { EndpointDraft } from '@/stores/store-types';

function detail(over: Partial<EndpointDetail> = {}): EndpointDetail {
  return {
    nodeId: 'e1',
    method: 'GET',
    url: '/users',
    baseUrlOverride: null,
    params: [],
    pathParams: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    docs: '',
    notes: '',
    tags: [],
    preRequestScript: '',
    testScript: '',
    tls: null,
    ...over,
  };
}

function env(over: Partial<Environment> = {}): Environment {
  return {
    id: 'env1',
    projectId: 'p1',
    name: 'dev',
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    createdAt: 1,
    ...over,
  };
}

const NO_DYNAMIC = new Set<string>();

describe('buildEndpointRequestParts', () => {
  it('没有草稿时用已存的那份', () => {
    const parts = buildEndpointRequestParts({
      detail: detail({ method: 'POST', url: '/orders' }),
      draft: undefined,
      endpointEnv: env(),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.equal(parts.method, 'POST');
    assert.equal(parts.urlPath, '/orders');
  });

  it('草稿压过已存的那份', () => {
    const draft: EndpointDraft = { method: 'DELETE', url: '/draft' };
    const parts = buildEndpointRequestParts({
      detail: detail({ method: 'POST', url: '/orders' }),
      draft,
      endpointEnv: env(),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.equal(parts.method, 'DELETE');
    assert.equal(parts.urlPath, '/draft');
  });

  it('自定义模式：baseUrl 就是覆盖值，不读模块那一格', () => {
    const parts = buildEndpointRequestParts({
      detail: detail({ baseUrlOverride: 'https://example.test' }),
      draft: undefined,
      endpointEnv: env({ moduleBaseUrls: { m1: { url: 'https://m1.test' } } }),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.equal(parts.baseUrl, 'https://example.test');
    assert.equal(parts.resolved.source, 'override');
    assert.equal(parts.resolved.proxy, null);
  });

  it('跟随模块：读环境里「环境 × 模块」那一格', () => {
    const parts = buildEndpointRequestParts({
      detail: detail(),
      draft: undefined,
      endpointEnv: env({ moduleBaseUrls: { m1: { url: 'https://m1.test' } } }),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.equal(parts.baseUrl, 'https://m1.test');
    assert.equal(parts.resolved.source, 'module');
    assert.equal(parts.moduleBaseUrl, 'https://m1.test');
  });

  it('那一格带代理地址时，baseUrl 是后端地址、proxy 是实际访问地址', () => {
    const parts = buildEndpointRequestParts({
      detail: detail(),
      draft: undefined,
      endpointEnv: env({
        moduleBaseUrls: {
          m1: { url: 'http://svc.internal:8080/api', proxy: 'https://gw.test/api' },
        },
      }),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.equal(parts.baseUrl, 'http://svc.internal:8080/api');
    assert.equal(parts.resolved.proxy, 'https://gw.test/api');
  });

  it('模块没有那一格时不借用别的模块的格子', () => {
    const parts = buildEndpointRequestParts({
      detail: detail(),
      draft: undefined,
      endpointEnv: env({ moduleBaseUrls: { m2: { url: 'https://m2.test' } } }),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.equal(parts.baseUrl, '');
    assert.equal(parts.resolved.source, 'none');
    assert.equal(parts.moduleBaseUrl, null);
  });

  it('脚本环境滤掉停用的变量', () => {
    const parts = buildEndpointRequestParts({
      detail: detail(),
      draft: undefined,
      endpointEnv: env({ variables: { a: '1', b: '2' }, disabledVariables: ['b'] }),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.deepEqual(parts.scriptEnv?.variables, { a: '1' });
  });

  it('没有环境时脚本环境是 undefined', () => {
    const parts = buildEndpointRequestParts({
      detail: detail(),
      draft: undefined,
      endpointEnv: undefined,
      moduleId: null,
      enabledDynamicNames: NO_DYNAMIC,
    });
    assert.equal(parts.scriptEnv, undefined);
  });

  it('动态变量名只留有定义且已启用的那些', () => {
    const parts = buildEndpointRequestParts({
      detail: detail({ url: '/users/{{userId}}' }),
      draft: undefined,
      endpointEnv: env({ variables: { userId: '7' } }),
      moduleId: 'm1',
      enabledDynamicNames: new Set(['userId']),
    });
    assert.deepEqual(parts.dynamicNames, ['userId']);
  });

  it('没有被引用的动态变量不进名单', () => {
    const parts = buildEndpointRequestParts({
      detail: detail({ url: '/users' }),
      draft: undefined,
      endpointEnv: env(),
      moduleId: 'm1',
      enabledDynamicNames: new Set(['userId']),
    });
    assert.deepEqual(parts.dynamicNames, []);
  });
});

describe('prepareEndpointRequest', () => {
  it('用给定的动态变量值组装', () => {
    const parts = buildEndpointRequestParts({
      detail: detail({ url: '/users/{{userId}}' }),
      draft: undefined,
      endpointEnv: env({ variables: { userId: 'from-env' } }),
      moduleId: 'm1',
      enabledDynamicNames: new Set(['userId']),
    });
    const request = prepareEndpointRequest(parts, { userId: 'from-dynamic' });
    assert.equal(request.url, '/users/from-dynamic', '动态变量压过同名环境变量');
  });

  it('不带动态变量时用环境里的值', () => {
    const parts = buildEndpointRequestParts({
      detail: detail({ url: '/users/{{userId}}' }),
      draft: undefined,
      endpointEnv: env({ variables: { userId: 'from-env' } }),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    const request = prepareEndpointRequest(parts, {});
    assert.equal(request.url, '/users/from-env');
  });

  it('代理地址替换请求地址开头的模块地址', () => {
    const parts = buildEndpointRequestParts({
      detail: detail({ url: '/orders/list' }),
      draft: undefined,
      endpointEnv: env({
        moduleBaseUrls: {
          m1: { url: 'http://svc.internal:8080/api', proxy: 'https://gw.test/api' },
        },
      }),
      moduleId: 'm1',
      enabledDynamicNames: NO_DYNAMIC,
    });
    const request = prepareEndpointRequest(parts, {});
    assert.equal(request.url, 'https://gw.test/api/orders/list');
  });
});
