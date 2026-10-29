import { test } from 'node:test';
import assert from 'node:assert/strict';
import { groupTools, OTHER_GROUP_KEY } from '@/lib/mcp-tool-groups';

/**
 * daemon 今天实际注册的工具。逐字照抄（而非推导）是有意的：给 Rust 注册表加了
 * 工具却没教 `groupTools` 它的前缀时，它会变成一条失败的「其他」断言，而不是表格里
 * 悄悄少一行。
 */
const REGISTERED_TOOLS = [
  'spec_upsert',
  'env_list',
  'env_get_active',
  'env_create',
  'env_update',
  'env_delete',
  'env_set_var',
  'env_set_baseurl',
  'env_set_active',
  'project_list',
  'project_get',
  'project_create',
  'project_update',
  'project_delete',
  'module_list',
  'module_create',
  'module_update',
  'module_delete',
  'module_move',
  'collection_list',
  'collection_create',
  'collection_update',
  'collection_delete',
  'collection_move',
  'endpoint_list',
  'endpoint_get',
  'endpoint_search',
  'endpoint_create',
  'endpoint_update',
  'endpoint_delete',
  'endpoint_duplicate',
  'endpoint_move',
  'api_run_request',
  'api_call_endpoint',
  'api_resolve_template',
  'api_resolve_endpoint',
  'api_validate_endpoint',
  'api_parse_response',
  'api_diff_responses',
  'api_extract_jsonpath',
  'dynamic_var_list',
  'dynamic_var_get',
  'dynamic_var_create',
  'dynamic_var_update',
  'dynamic_var_delete',
  'dynamic_var_resolve',
  'dynamic_var_set_override',
];

test('groupTools: every registered tool lands in its prefix family', () => {
  const groups = groupTools(REGISTERED_TOOLS);
  const keys = groups.map((g) => g.key);

  // 当前注册表不该有任何东西落到「其他」——落到那里就是「新工具需要一个分类」的信号。
  assert.equal(
    keys.includes(OTHER_GROUP_KEY),
    false,
    `未归类: ${JSON.stringify(groups.find((g) => g.key === OTHER_GROUP_KEY)?.items)}`,
  );
  assert.deepEqual(keys, [
    'spec',
    'env',
    'project',
    'module',
    'collection',
    'endpoint',
    'dynamic',
    'api',
  ]);
});

test('groupTools: per-family counts match the registry', () => {
  const byKey = Object.fromEntries(
    groupTools(REGISTERED_TOOLS).map((g) => [g.key, g.items.length]),
  );
  assert.deepEqual(byKey, {
    spec: 1,
    env: 8,
    project: 5,
    module: 5,
    collection: 5,
    endpoint: 8,
    dynamic: 7,
    api: 8,
  });
  assert.equal(groupTools(REGISTERED_TOOLS).flatMap((g) => g.items).length, 47);
});

test('groupTools: no tool is dropped or duplicated', () => {
  const flat = groupTools(REGISTERED_TOOLS).flatMap((g) => g.items);
  assert.equal(flat.length, REGISTERED_TOOLS.length);
  assert.deepEqual([...flat].sort(), [...REGISTERED_TOOLS].sort());
});

test('groupTools: a name with no known prefix lands in 其他, last', () => {
  const groups = groupTools(['endpoint_list', 'brand_new_tool']);
  assert.equal(groups.length, 2);
  assert.equal(groups[1].key, OTHER_GROUP_KEY);
  assert.deepEqual(groups[1].items, ['brand_new_tool']);
});

test('groupTools: empty input yields no groups', () => {
  assert.deepEqual(groupTools([]), []);
});

test('groupTools: an empty family is dropped, not rendered as a heading', () => {
  const groups = groupTools(['env_list', 'api_run_request']);
  assert.deepEqual(
    groups.map((g) => g.key),
    ['env', 'api'],
  );
});

test('groupTools: input order does not change output order', () => {
  const forward = groupTools(['endpoint_list', 'env_list']);
  const reversed = groupTools(['env_list', 'endpoint_list']);
  assert.deepEqual(forward, reversed);
});
