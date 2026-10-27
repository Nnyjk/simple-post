import type { Environment } from '@/types/domain';
import { mergePool } from '../dynamic-vars';

/**
 * 跨请求涉及的每个字段累积未解析的 `{{name}}`，按首次出现去重。
 * `Set` 管去重 + 数组管顺序：只用 `Set` 集合相同但顺序依赖插入内部实现，
 * 而顺序正是「缺哪些变量」提示在用户打字时保持稳定的原因。
 */
export interface MissingTracker {
  add(names: readonly string[]): void;
  list(): string[];
}

export function createMissingTracker(): MissingTracker {
  const seen = new Set<string>();
  const names: string[] = [];
  return {
    add(chunk) {
      for (const n of chunk) {
        if (seen.has(n)) continue;
        seen.add(n);
        names.push(n);
      }
    },
    // 拷贝：`PreparedRequest` 是交给调用方的值对象，不能把仍在追加的数组交出去。
    list: () => names.slice(),
  };
}

/**
 * 合成变量池：环境变量（去掉停用的那些）+ 动态变量的值。
 *
 * 合并规则只有一份实现，在 `src/lib/dynamic-vars.ts` 的 `mergePool`——它与 Rust 的
 * `core::dynamic::merge_pool` 同一套，由 `fixtures/dynamic-parity.json` 钉住。
 */
export function buildPool(
  env: Environment | undefined,
  dynamicVars: Record<string, string> | undefined,
): Record<string, string> {
  return mergePool(env?.variables ?? {}, env?.disabledVariables ?? [], dynamicVars ?? {});
}
