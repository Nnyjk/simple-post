/**
 * 文档测试的素材构造器。三种实体（节点 / 接口配置 / 环境）字段都不少，逐个手写会让测试本身
 * 比被测代码还长；这里只暴露「这条用例真正关心的那几个字段」，其余按健康默认值补齐。
 */
import type { EndpointDetail, Environment, KeyValue, ResourceNode } from '@/types/domain';

export function node(
  init: Partial<ResourceNode> & Pick<ResourceNode, 'id' | 'kind'>,
): ResourceNode {
  return {
    rootId: 'prj_1',
    parentId: null,
    name: init.id,
    sortOrder: 0,
    createdAt: 0,
    updatedAt: 0,
    ...init,
  };
}

export function detail(
  init: Partial<EndpointDetail> & Pick<EndpointDetail, 'nodeId'>,
): EndpointDetail {
  return {
    method: 'GET',
    url: '/items',
    params: [],
    pathParams: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    responses: [],
    tags: [],
    ...init,
  };
}

export function kv(key: string, value: string, description?: string): KeyValue {
  return { id: `kv_${key}`, key, value, enabled: true, description };
}

export function environment(init: Partial<Environment> & Pick<Environment, 'name'>): Environment {
  return {
    id: 'env_1',
    projectId: 'prj_1',
    moduleBaseUrls: {},
    variables: {},
    disabledVariables: [],
    createdAt: 0,
    ...init,
  };
}
