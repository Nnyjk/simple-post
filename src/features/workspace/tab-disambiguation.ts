/**
 * 标签条上的同名消歧 —— 给接口标签算出它所在容器的路径。
 *
 * **为什么需要它。** 标签的身份是 id，标题是接口名，而接口名在项目里**不唯一**：不同模块、
 * 不同集合下可以有同名接口，标签条于是摆出两个一字不差的标签，用户无从分辨哪个是哪个。
 * 标签宽度是固定的 160px，放不下完整路径，故路径要挑着放。
 *
 * **为什么可见的提示只在重名时出现。** 提示占掉标题右侧约 40px，平时留着就是白花掉标题的
 * 空间。故只有这批标签里确实有同名接口时，才给它补一段可见提示；没有冲突时标签与原来
 * 一模一样。
 *
 * **提示按三个级别往外退。** 直接父容器的名字最短也最贴近用户；它不足以区分这一组同名
 * 标签时（例如两个模块下各有一个叫「订单」的集合），退到模块名；再不够就退到**最短的可区
 * 别前缀**——一段一段往下加，加到这一组各条互不相同为止。三个级别都不足以区分时（同名模块
 * 下的同名集合里各有一个同名接口）就不写提示——写一段看起来有区别的文字，比不写更让人误
 * 解。
 */
import type { ResourceNode } from '@/types/domain';
import { containerPathOf } from '@/lib/tree-index';

export interface EndpointTabSource {
  /** 标签 id。 */
  id: string;
  /** 标签标题，即接口名。 */
  title: string;
  /** 接口所在容器的节点 id。已存接口与未保存的草稿都给得出。 */
  parentId: string | null;
}

export interface TabPathInfo {
  /** 完整路径，从模块往下逐层用「›」隔开。接口还没有容器时是空串。 */
  path: string;
  /** 标题右侧那一小段提示。重名时才是非空串，其余时候是空串。 */
  hint: string;
}

/**
 * 可见提示的三个级别，从最贴近接口的一层往外退。
 *
 * `last` 是直接父容器的名字，`module` 是路径最外层（模块）的名字，`prefix` 是能让这一组
 * 互相区分开的最短前缀。
 */
type HintLevel = 'last' | 'module' | 'prefix';

/** 提示怎么取的一组规则。`prefixLen` 只在 `prefix` 级别下有意义。 */
interface HintRule {
  level: HintLevel;
  prefixLen: number;
}

/**
 * 分组用的标题键：连续的空白压成一个空格，首尾空白去掉。
 *
 * HTML 把连续的空白渲染成一个空格，故「用户 列表」与「用户  列表」在标签上看起来
 * 一模一样；草稿的名字在保存之前也没有被 trim 过（保存时才 trim，见
 * `saveDraftEndpoint`）。若按原始标题分组，这两个标签会各自落进不同的组，谁也不给
 * 提示，于是标签条上又出现两个看起来一样的标签。
 *
 * 这个键**只用于分组**；标签上显示的仍是原始标题。
 */
function normalizeTitle(title: string): string {
  return title.replace(/\s+/g, ' ').trim();
}

/**
 * 算出每个接口标签的路径，并标出哪些标签需要可见的消歧提示。
 *
 * 返回的 map 覆盖**每一个**接口标签：完整路径始终算得出（它不占地方，只写进悬停提示），
 * 而可见的那一段提示只在同名时才有。
 *
 * 只在**接口类**标签之间比名字：模块与集合有各自的图标，一个叫「用户列表」的模块与一个
 * 叫「用户列表」的接口不会让人认错。
 */
export function endpointTabPaths(
  sources: readonly EndpointTabSource[],
  nodes: readonly ResourceNode[],
): Map<string, TabPathInfo> {
  const out = new Map<string, TabPathInfo>();

  // 先按标题分组：同一组里的标签互相之间才需要区分。
  const byTitle = new Map<string, EndpointTabSource[]>();
  for (const s of sources) {
    const key = normalizeTitle(s.title);
    const group = byTitle.get(key);
    if (group) group.push(s);
    else byTitle.set(key, [s]);
  }

  for (const group of byTitle.values()) {
    // 这个标题下只有一个标签时，用户不会认错，不需要可见提示。
    const collides = group.length > 1;
    const entries = group.map((source) => ({
      source,
      segments: containerPathOf(nodes, source.parentId),
    }));
    // 提示的级别整组共用：同一个级别下各条的取值互不相同，用户才分得清谁是谁。
    const rule = collides ? pickHintRule(entries) : null;

    for (const { source, segments } of entries) {
      out.set(source.id, {
        path: segments.join(' › '),
        hint: rule === null ? '' : hintBy(rule, segments),
      });
    }
  }
  return out;
}

/**
 * 选出整组共用的提示规则。
 *
 * 一个级别可用，当且仅当组内各条的取值互不相同。组内有一条拿不出取值（接口还没有容器）
 * 时，整组都不写——它没有可写的提示。
 *
 * 两个单段级别都不够时，取**最短的可区分前缀**：短的那些段先试，不够再往下加一段。前缀
 * 比整条路径短，而标签上能显示的字数有限，短一点就多一分显示得下的机会。
 */
function pickHintRule(entries: { segments: string[] }[]): HintRule | null {
  // 有一条拿不出路径（接口还没有容器）时，整组都不写提示。
  if (entries.some((e) => e.segments.length === 0)) return null;

  for (const level of ['last', 'module'] as const) {
    const values = entries.map((e) => hintBy({ level, prefixLen: 0 }, e.segments));
    if (new Set(values).size === values.length) return { level, prefixLen: 0 };
  }

  const prefixLen = shortestDistinguishingPrefix(entries.map((e) => e.segments));
  if (prefixLen === null) return null;
  return { level: 'prefix', prefixLen };
}

/**
 * 让组内各条互不相同的最短前缀有几段。各条路径完全相同时没有这样的长度——那种情况
 * 任何提示都区分不了它们。
 */
function shortestDistinguishingPrefix(all: string[][]): number | null {
  const maxLen = Math.max(0, ...all.map((s) => s.length));
  for (let k = 1; k <= maxLen; k++) {
    const keys = all.map((s) => s.slice(0, k).join(' › '));
    if (new Set(keys).size === keys.length) return k;
  }
  return null;
}

/**
 * 按规则取出提示文字。
 *
 * `last` 取最靠近接口的那一段容器名，`module` 取最外层（模块）的名字，`prefix` 取路径最
 * 前面的若干段。接口还没有容器时没有哪一段可取，一律返回空串。
 */
function hintBy(rule: HintRule, segments: string[]): string {
  if (segments.length === 0) return '';
  if (rule.level === 'last') return segments[segments.length - 1];
  if (rule.level === 'module') return segments[0];
  return segments.slice(0, rule.prefixLen).join(' › ');
}
