import { listDynamicNames } from '@/lib/dynamic-vars';
import type { DynamicValueStatus, DynamicVariableView } from '@/types/domain';
import type { VarEntry } from '../var-hint';
import type { EffectiveRequest } from './effectiveRequest';

/**
 * 变量选择器的候选条目：动态变量的定义与它们当前的取值。
 *
 * 停用的定义不参与替换，故 `buildDynamicEntries` 不显示它的值——显示一个用不上的值会误导。
 */
export function buildDynamicEntries(
  dynamicVariables: DynamicVariableView[],
  dynamicValues: Record<string, DynamicValueStatus>,
): VarEntry[] {
  return dynamicVariables.map((v) => {
    if (!v.enabled) {
      return { key: v.name, value: '', origin: 'dynamic' as const, hint: '已停用' };
    }
    const status = dynamicValues[v.name];
    const source = status?.source ?? v.source;
    const fetchedAt = status?.fetchedAt ?? v.fetchedAt;
    return {
      key: v.name,
      value: status?.value ?? v.value ?? '',
      origin: 'dynamic' as const,
      hint:
        source === 'override'
          ? '手动改的'
          : source === 'stale'
            ? '已过期'
            : fetchedAt
              ? `${Math.max(0, Math.round((Date.now() - fetchedAt) / 1000))} 秒前取回`
              : undefined,
    };
  });
}

/**
 * **已启用且仍存在**的动态变量名。
 *
 * `buildDynamicNames`（扫出谁被引用了）与 `buildDynamicVars`（把值并进变量池）共用它：
 * 停用或删除之后，那个名字既不算被引用，它的旧值也不进变量池——否则「停用后不参与替换」
 * 是假的。
 */
export function enabledDynamicNameSet(dynamicVariables: DynamicVariableView[]): Set<string> {
  return new Set(dynamicVariables.filter((v) => v.enabled).map((v) => v.name));
}

/**
 * 这个请求引用了哪些动态变量。
 *
 * 扫的是**未替换**的模板，故同名环境变量也会被扫出来——动态变量在替换时压过环境变量
 * （见 `mergePool`），故名单里只留有定义且已启用的那些。
 */
export function buildDynamicNames(
  baseUrl: string,
  url: string,
  effective: EffectiveRequest,
): string[] {
  return listDynamicNames({
    baseUrl,
    url,
    params: effective.params,
    pathParams: effective.pathParams,
    headers: effective.headers,
    body: effective.body,
    auth: effective.auth,
  });
}

/**
 * 当前生效的动态变量值。它们在变量池的最后一步并入，故同名时压过环境变量；被停用的环境
 * 变量也不影响它们。桶里只留下**已启用且仍存在**的那些名字：桶是累加的，停用或删除
 * 之后旧条目还在，直接并进去就等于停用之后仍然在替换。
 */
export function buildDynamicVars(
  dynamicValues: Record<string, DynamicValueStatus>,
  enabledDynamicNames: Set<string>,
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [name, status] of Object.entries(dynamicValues)) {
    if (!enabledDynamicNames.has(name)) continue;
    if (status.value !== null && status.value !== undefined) out[name] = status.value;
  }
  return out;
}
