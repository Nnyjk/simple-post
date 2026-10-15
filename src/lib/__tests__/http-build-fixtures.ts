/**
 * `http-build.test.ts` 各分片共用的夹具。**不是**测试文件（不匹配
 * `*.test.ts`，故不被 `scripts/run-tests.mjs` 发现）——它只是让每个分片不必
 * 各自复制一份同形的 `BuildInput` / `PrepareRequestInput` 工厂。
 */
import type { BuildInput, PrepareRequestInput } from '../http-build';
import type { Environment, KeyValue } from '../../types/domain';

export const ENV: Environment = {
  id: 'env_test',
  projectId: 'p_test',
  name: 'test',
  baseUrls: {},
  // (模块 × 服务) 的 URL 层——`prepareRequest` 并不认这个概念（它收到的是已解析好的
  // `baseUrl`），但该字段没有默认值，所以夹具必须点名它
  moduleBaseUrls: {},
  variables: {
    token: 'eyJ.tok',
    userId: 'u_demo',
    apiKey: 'k_123',
    site: 'cn',
  },
  isActive: true,
};

export function kv(key: string, value: string, enabled = true): KeyValue {
  return { id: `k_${key}`, key, value, enabled };
}

export function input(overrides: Partial<BuildInput> = {}): BuildInput {
  return {
    method: 'GET',
    url: 'http://api.example.com/x',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    ...overrides,
  };
}

/** 同样的夹具形状，但拆成 `prepareRequest`（与 base URL 选择器）真正吃的那对
 *  path / base URL。 */
export function prep(overrides: Partial<PrepareRequestInput> = {}): PrepareRequestInput {
  return {
    method: 'GET',
    urlPath: '/x',
    baseUrl: '',
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { kind: 'none' },
    ...overrides,
  };
}

/** 元组列表 → 查找表，供「某个 header 存不存在」这类断言用。 */
export function headerMap(
  tuples: ReadonlyArray<readonly [string, string]>,
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of tuples) out[k] = v;
  return out;
}
