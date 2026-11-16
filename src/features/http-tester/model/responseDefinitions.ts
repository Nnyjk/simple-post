/**
 * 响应定义列表的纯操作：追加、改一条、删除、上下移动。
 *
 * 界面只维护这份数组，落盘时整份交给后端（`EndpointDetail.responses` 整体替换）。字段规则
 * （状态码语法、同份内重复、`schema` 必须是对象、`contentType` 的 `type/subtype`）只在
 * `simple_post_core::domain::response_problems` 判一次，这里一条都不复制。
 */
import type { ResponseDefinition } from '@/types/domain';

/**
 * 在末尾追加一条空定义。`id` 留空符合契约：写入口在落盘时给缺 id 的行铸 `resp_` 前缀，
 * 故界面不必自己编一个（自编的 id 不带那个前缀，反而不合约定）。
 */
export function appendResponse(responses: ResponseDefinition[]): ResponseDefinition[] {
  return [...responses, { id: '', status: '', description: '', contentType: 'application/json' }];
}

/** 按位置改一条，其余行原样留下。 */
export function patchResponse(
  responses: ResponseDefinition[],
  index: number,
  patch: Partial<ResponseDefinition>,
): ResponseDefinition[] {
  return responses.map((row, i) => (i === index ? { ...row, ...patch } : row));
}

/** 按位置删一条。 */
export function removeResponse(
  responses: ResponseDefinition[],
  index: number,
): ResponseDefinition[] {
  return responses.filter((_, i) => i !== index);
}

/**
 * 把第 `index` 条挪 `delta` 格（-1 向上、1 向下）。
 *
 * 到头了就返回**原来的数组**：`useAppStore` 的草稿写入是不可变更新，给回同一个引用等于
 * 「这一下没发生」，不会多渲染一次。
 */
export function moveResponse(
  responses: ResponseDefinition[],
  index: number,
  delta: -1 | 1,
): ResponseDefinition[] {
  const target = index + delta;
  if (target < 0 || target >= responses.length) return responses;
  const next = [...responses];
  const [row] = next.splice(index, 1);
  next.splice(target, 0, row);
  return next;
}
