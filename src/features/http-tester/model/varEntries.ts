import type { Environment } from '@/types/domain';
import type { VarEntry } from '../var-hint';

/**
 * 变量候选列表：接口自己的环境变量 + 动态变量。
 *
 * 环境变量取的是**接口解析 `{{x}}` 所依据的那一份环境**（`endpointEnv`），不是项目的
 * 全部环境——项目里别处的变量在这个接口上替换不到，列出来只是噪声。
 *
 * 同名时动态变量排在前面：它在替换时压过环境变量（见 `lib/dynamic-vars.ts` 的
 * `mergePool`），故列表里的先后顺序要与替换时的优先级一致。
 */
export function buildVarEntries(env: Environment | undefined, dynamic: VarEntry[]): VarEntry[] {
  const envEntries: VarEntry[] = env
    ? Object.entries(env.variables).map(([k, v]) => ({
        key: k,
        value: v,
        origin: 'environment' as const,
      }))
    : [];
  return [...dynamic, ...envEntries].sort((a, b) => a.key.localeCompare(b.key));
}
