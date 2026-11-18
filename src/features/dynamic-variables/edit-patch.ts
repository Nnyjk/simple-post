/**
 * 从「打开表单时的那份定义」与「此刻的草稿」算出编辑用的 patch。
 *
 * 三态是后端的契约（`core::domain::DynamicVariablePatch`）：**缺席 = 不动**、`null` = 清除、
 * 值 = 写入。故这一份只写真的变过的字段：两个窗口各改一格时，一个只改名的保存不会把别人
 * 改的数据源覆盖回去。
 *
 * 比对前把两边都归一到「保存后会是什么」：草稿里的空串与定义里的 `null` 都是「没有」，故
 * 「本来就空、现在还是空」不会变成一条 `null`（那会撞后端「必须有一个数据源」的校验）。
 */
import type { DynamicVariable } from '@/types/domain';
import type { DynamicVariableDraft } from '@/stores/dynamic-slice';

/** 草稿里的空串、全空白与定义里的 null 都表示「没有」：只敲了几个空格不等于填了内容。 */
function orNull(value: string): string | null {
  return value.trim() === '' ? null : value;
}

/**
 * 缓存时长：输入框存的是原文，清空或写坏了按默认 60 秒算。
 *
 * 0 是有意义的（不缓存，每次重新取数），故不能用 `|| 60`——那会把 0 变成假值再回退。
 */
export function ttlSecondsOf(raw: string): number {
  // 空输入不能被读成 0：`Number('')` 是 0，而 0 的意思是「不缓存，每次重新取数」。
  if (raw.trim() === '') return 60;
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : 60;
}

/** 编辑用的 patch。只含真的变过的字段。 */
export function buildEditPatch(
  initial: DynamicVariable,
  draft: DynamicVariableDraft,
): Partial<DynamicVariable> {
  const patch: Partial<DynamicVariable> = {};

  if (draft.name !== initial.name) patch.name = draft.name;
  if (draft.extractPath !== initial.extractPath) patch.extractPath = draft.extractPath;

  const description = orNull(draft.description);
  if (description !== orNull(initial.description ?? '')) patch.description = description;

  // 只写空白的处理脚本与不写脚本等价：两边都过同一个归一，才不会把定义里原本只有空白的
  // 脚本判成「用户清空了」而发 `null`；真把一段非空脚本清掉时仍然发 `null`。
  const transformScript = orNull(draft.transformScript);
  if (transformScript !== orNull(initial.transformScript ?? '')) {
    patch.transformScript = transformScript;
  }

  if (draft.valueMode !== initial.valueMode) patch.valueMode = draft.valueMode;

  const ttlSeconds = ttlSecondsOf(draft.ttlSeconds);
  if (ttlSeconds !== initial.ttlSeconds) patch.ttlSeconds = ttlSeconds;

  if (draft.staleFallback !== initial.staleFallback) patch.staleFallback = draft.staleFallback;
  if (draft.enabled !== initial.enabled) patch.enabled = draft.enabled;

  // 数据源三格：形态决定另两格「保存后应当是什么」——内联请求自带完整地址，用不到取数环境；
  // 引用接口时，定义里那份内联请求也已经没有意义。
  const usesEndpoint = draft.sourceKind === 'endpoint';
  const sourceEndpointId = usesEndpoint ? orNull(draft.sourceEndpointId) : null;
  const sourceInlineJson = usesEndpoint ? null : orNull(draft.sourceInlineJson);
  const sourceEnvironmentId = usesEndpoint ? orNull(draft.environmentId) : null;

  if (sourceEndpointId !== orNull(initial.sourceEndpointId ?? '')) {
    patch.sourceEndpointId = sourceEndpointId;
  }
  if (sourceInlineJson !== orNull(initial.sourceInlineJson ?? '')) {
    patch.sourceInlineJson = sourceInlineJson;
  }
  if (sourceEnvironmentId !== orNull(initial.sourceEnvironmentId ?? '')) {
    patch.sourceEnvironmentId = sourceEnvironmentId;
  }

  return patch;
}

/**
 * 这份 patch 落到这条定义上之后，还留着一个数据源吗。
 *
 * 判的必须是**将要提交的那份 patch**，不是「定义此刻有没有来源」：别的客户端刚把删掉的接口
 * 补回来时，本地只改名字的编辑也该放行——它提交的 patch 里根本没有来源那两格。反过来，把
 * 两个来源都清空的 patch 一定要被挡下（那样的定义永远解析不出值）。
 */
export function keepsSource(initial: DynamicVariable, patch: Partial<DynamicVariable>): boolean {
  const after = { ...initial, ...patch };
  return !!(after.sourceEndpointId || after.sourceInlineJson);
}
