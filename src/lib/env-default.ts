/**
 * 环境的挑选规则 —— 「默认环境」与「当前环境」各是哪一条。
 *
 * 库里没有「当前环境」这一列。当前环境是每个客户端自己的实时状态：桌面窗口与网页
 * 标签各有一份，各切各的。落库会让 web 端多个人互相踩——一个人切了环境，另一个人
 * 发请求就换了地址与端口。
 *
 * 库里只有一个与「选哪个环境」有关的概念：**默认环境**。它是项目的属性，即一个项目
 * 里**最早创建**的那个环境。后端按 `created_at ASC, id ASC LIMIT 1` 取它（见
 * `db::environments::get_default`），本文件取的是同一批数据，故两边给出同一个答案。
 */
import type { Environment } from '@/types/domain';

/**
 * 挑出某个项目的**默认环境**：该项目里最早创建的那个环境。项目没有环境时返回
 * `undefined`。
 *
 * 「最早创建」按 `createdAt` 认定；同一毫秒里创建的两个环境按 id 决胜，故这个结果
 * 唯一，与项目里有几个环境无关。
 *
 * 排序规则必须与后端 `db::environments::get_default` 的 `ORDER BY created_at ASC,
 * id ASC LIMIT 1` 一致：两边给出的默认环境不同，则界面预览的请求与真正发出去的请求
 * 不是同一个。
 */
export function pickDefaultEnvironment(
  environments: readonly Environment[],
  projectId: string,
): Environment | undefined {
  let best: Environment | undefined;
  for (const env of environments) {
    if (env.projectId !== projectId) continue;
    if (!best) {
      best = env;
      continue;
    }
    if (env.createdAt < best.createdAt) {
      best = env;
      continue;
    }
    if (env.createdAt === best.createdAt && env.id < best.id) best = env;
  }
  return best;
}

/**
 * 挑出某个项目**当前**应该用的环境。
 *
 * 顺序是「记住的那个 → 默认环境」：
 *
 *   - 记住的那个是本客户端上次在这个项目里用的环境（存在 localStorage 里）。它还在
 *     这个项目里就用它——用户切到 staging 之后刷新页面，不该被拽回 dev。
 *   - 记住的那个已经不在了（环境被删了、项目被换掉了），或者压根没记住过，就用
 *     默认环境。
 *
 * 项目没有环境时返回 `undefined`——那是诚实的答案，界面据此显示「—」。
 */
export function pickCurrentEnvironment(
  environments: readonly Environment[],
  projectId: string,
  rememberedId: string,
): Environment | undefined {
  if (rememberedId) {
    const remembered = environments.find((e) => e.id === rememberedId && e.projectId === projectId);
    if (remembered) return remembered;
  }
  return pickDefaultEnvironment(environments, projectId);
}
