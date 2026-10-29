import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useAppStore, useActiveEndpointResource } from '@/stores/app-store';
import { type BaseUrlSelection } from '@/features/http-tester/BaseUrlPicker';
import { type EnvSnapshot, type HttpMethod } from '@/types/domain';
import { type FormatResult } from '@/lib/utils';
import { validateRequest } from '@/lib/request-validation';
import { enabledVars } from '@/lib/url';
import { UNTITLED_ENDPOINT_NAME, type EndpointDraft } from '@/stores/store-types';
import { buildHeaderSuggestions } from '@/lib/common-headers';
import { useHttpTesterMotion } from './useHttpTesterMotion';
import { useRequestActions } from './useRequestActions';
import { useEndpointContext } from './useEndpointContext';
import { useVarHint, type VarEntry } from './var-hint';
import { pathPlaceholders } from '@/lib/path-params';
import { buildEffectiveRequest } from './model/effectiveRequest';
import { buildRequestInput } from './model/requestInput';
import { buildVarEntries } from './model/varEntries';
import {
  buildDynamicEntries,
  buildDynamicNames,
  buildDynamicVars,
  enabledDynamicNameSet,
} from './model/dynamicVars';
import { buildTabItems, type TesterTabId } from './tabs/tabItems';
import { buildPathParamError } from './tabs/pathParamError';
import { TesterShell } from './tabs/TesterShell';

export function HttpTester() {
  // `useActiveEndpointResource` 对草稿接口同样有答案：两行都没有时它从 `DraftEndpoint` 合成。
  // 两个 selector 都返回 store 里的稳定引用，不会新建对象。
  const resource = useActiveEndpointResource();
  const endpoint = resource?.node ?? null;
  const detail = resource?.detail;
  /** 接口尚未写入数据库。 */
  const isDraft = resource?.isDraft ?? false;
  // 供 Header 键补全：把用户自己用过的 header 名与内置常见列表一起作为 datalist 的候选。
  const allEndpointDetails = useAppStore((s) => s.endpointDetails);
  const updateNode = useAppStore((s) => s.updateNode);
  const updateEndpointDetail = useAppStore((s) => s.updateEndpointDetail);
  const setRequestPending = useAppStore((s) => s.setRequestPending);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const setLastScriptOutcome = useAppStore((s) => s.setLastScriptOutcome);
  const applyEnvWrites = useAppStore((s) => s.applyEnvWrites);
  const addResponseHistory = useAppStore((s) => s.addResponseHistory);
  const setEndpointDraft = useAppStore((s) => s.setEndpointDraft);
  const clearEndpointDraft = useAppStore((s) => s.clearEndpointDraft);
  const saveDraftEndpoint = useAppStore((s) => s.saveDraftEndpoint);
  const updateDraftEndpoint = useAppStore((s) => s.updateDraftEndpoint);
  const draftSaveError = useAppStore((s) => s.draftSaveError);
  const requestPending = useAppStore((s) => s.requestPending);
  const requestSettings = useAppStore((s) => s.requestSettings);
  // 动态变量挂在项目里，故这一页只认**消费方项目**（当前项目）里的定义：
  // 请求侧不回退到别的项目，同名变量在不同项目里是不同的东西。
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const dynamicVariables = useAppStore((s) => s.dynamicVariables);
  const dynamicValues = useAppStore((s) => s.dynamicValues);
  const loadDynamicVariables = useAppStore((s) => s.loadDynamicVariables);
  const resolveDynamicVariables = useAppStore((s) => s.resolveDynamicVariables);
  const dynamicError = useAppStore((s) => s.dynamicError);

  const [tab, setTab] = useState<TesterTabId>('params');
  const [curlFlash, setCurlFlash] = useState(false);

  // URL/method 草稿存在 store（`endpointDrafts[endpointId]`）而非本地 state，切走再切回
  // 标签才留住未提交的编辑；store 把它同步镜像进 `dirtyTabIds`，标签条的脏点因此自动更新。
  const draft = useAppStore((s) => (endpoint ? s.endpointDrafts[endpoint.id] : undefined));
  // 名称 / 描述的编辑归 `<InlineEdit>`（以 `endpoint.id` 为 key，激活接口一变编辑器即卸载重置）。

  const urlInputRef = useRef<HTMLInputElement>(null);

  // 实时的 URL —— 输入框此刻的内容：通常是 path，自定义模式下是完整地址。
  const liveUrl = draft?.url ?? detail?.url ?? '';

  const { project, ancestors, endpointEnv, resolved, selection, isCustomUrl } = useEndpointContext(
    endpoint,
    detail,
  );

  // 脚本看得到的环境与解析用的是**同一份变量池**（停用的变量不参与）。
  const scriptEnv = useMemo<EnvSnapshot | undefined>(() => {
    if (!endpointEnv) return undefined;
    return { id: endpointEnv.id, variables: enabledVars(endpointEnv) };
  }, [endpointEnv]);

  // 有效请求：已存的 detail 叠上草稿（草稿优先），是唯一决定「用户所见即为何」的地方。
  const effective = useMemo(() => buildEffectiveRequest(draft, detail), [draft, detail]);

  /** 把某个编辑器的产出写进草稿（从而置脏）。 */
  const patchDraft = useCallback(
    (patch: Partial<EndpointDraft>) => {
      if (!endpoint) return;
      setEndpointDraft(endpoint.id, patch);
    },
    [endpoint, setEndpointDraft],
  );

  // URL 路径里有几个占位符 —— 路径参数那一页要列几行。这个 useMemo 必须在提前返回
  // **之上**：条件调用的 hook 会让 React 在切换接口时抛「Rendered fewer hooks than expected」。
  const pathParamCount = useMemo(() => pathPlaceholders(liveUrl).length, [liveUrl]);

  // 草稿的文档字段存在 `draftEndpoints`（草稿没有 `nodes` 行），经 `saveDraftEndpoint` 落库。
  const saveEndpointName = useCallback(
    (next: string) => {
      if (!endpoint) return;
      if (isDraft) updateDraftEndpoint(endpoint.id, { name: next });
      else updateNode(endpoint.id, { name: next });
    },
    [endpoint, isDraft, updateDraftEndpoint, updateNode],
  );

  const saveEndpointDescription = useCallback(
    (next: string) => {
      if (!endpoint) return;
      if (isDraft) updateDraftEndpoint(endpoint.id, { description: next });
      else updateNode(endpoint.id, { description: next || undefined });
    },
    [endpoint, isDraft, updateDraftEndpoint, updateNode],
  );

  /**
   * 把一次 baseUrl 选择提交到这个接口存放它的位置。`baseUrlDefinitionId` /
   * `baseUrlOverride` 是草稿的**文档**字段，未保存的接口须用 `updateDraftEndpoint` 写：
   * 改用 `updateEndpointDetail` 对草稿是静默空操作，选择器会弹回项目默认、URL 保持无前缀。
   */
  const commitBaseUrl = useCallback(
    (patch: { baseUrlDefinitionId: string | null; baseUrlOverride: string | null }) => {
      if (!endpoint) return;
      if (isDraft) updateDraftEndpoint(endpoint.id, patch);
      else updateEndpointDetail(endpoint.id, patch);
    },
    [endpoint, isDraft, updateDraftEndpoint, updateEndpointDetail],
  );

  // 新建即命名 —— 新草稿打开时标题已选中。按草稿 id 上闩而非按名称驱动：条件是
  // 「这个草稿仍是占位名」。
  const [namingDraft, setNamingDraft] = useState(false);
  const autoNameOpenedFor = useRef<string | null>(null);
  useEffect(() => {
    if (!isDraft || !endpoint) return;
    if (endpoint.name !== UNTITLED_ENDPOINT_NAME) return;
    if (autoNameOpenedFor.current === endpoint.id) return;
    autoNameOpenedFor.current = endpoint.id;
    setNamingDraft(true);
  }, [isDraft, endpoint]);

  // 钉住的格式错误。见 `lib/request-validation`：校验是 发送 / 保存 / 格式化 触发的动作，
  // 通过即清除。刻意留在本组件：切页时它存活（BodyEditor 会重挂载而它不会）。
  const [validationError, setValidationError] = useState<string | null>(null);

  /**
   * 上一次发送是不是被路径参数缺值拦下了。只记「拦下过」这一件事，缺哪几个由**当下**的
   * 请求状态算出来。
   */
  const [sendBlocked, setSendBlocked] = useState(false);

  /** 路径参数的闸门开了还是关了。闸门关着时切到路径参数那一页，用户一眼看得见缺哪个。 */
  const handlePathParamGate = useCallback((missing: string[]) => {
    setSendBlocked(missing.length > 0);
    if (missing.length > 0) setTab('path');
  }, []);

  /** 跑校验，钉住或清除徽标，并回报是否通过。失败时顺带切到出问题的那一页。 */
  const runValidation = useCallback((): boolean => {
    const issue = validateRequest({ body: effective.body });
    setValidationError(issue?.message ?? null);
    if (issue) {
      if (issue.tab === 'body') setTab('body');
      return false;
    }
    return true;
  }, [effective.body]);

  // Header 补全从每个已加载 detail 收拢用户自己的 header 名（`Object.values` 会新建对象，
  // 故按该 map 的引用 memo）。
  const headerSuggestions = useMemo(
    () => buildHeaderSuggestions(Object.values(allEndpointDetails)),
    [allEndpointDetails],
  );

  // 变量选择器的条目：环境变量与动态变量并进同一份候选（不新开页签）。
  //
  // 只取当前项目里的定义与它们当前项目的取值桶：别的项目的同名变量不参与这次替换。
  const projectDynamicVariables = useMemo(
    () => dynamicVariables.filter((v) => v.projectId === activeProjectId),
    [dynamicVariables, activeProjectId],
  );
  const projectDynamicValues = useMemo(
    () => dynamicValues[activeProjectId] ?? {},
    [dynamicValues, activeProjectId],
  );

  // 变量候选：环境变量与动态变量并进同一份候选（不新开页签）。
  const dynamicEntries = useMemo<VarEntry[]>(
    () => buildDynamicEntries(projectDynamicVariables, projectDynamicValues),
    [projectDynamicVariables, projectDynamicValues],
  );

  // 全部候选（环境 + 动态）。URL 栏与各输入框共用同一份，故候选只在这里算一次。
  const varEntries = useMemo<VarEntry[]>(
    () => buildVarEntries(endpointEnv, dynamicEntries),
    [endpointEnv, dynamicEntries],
  );

  // URL 输入框里的变量提示。用户敲 `{{` 时浮出候选，选中即写进草稿。
  const urlHint = useVarHint({
    value: liveUrl,
    onChange: (next) => {
      if (!endpoint) return;
      setEndpointDraft(endpoint.id, { url: next });
    },
    entries: varEntries,
  });

  /**
   * 自定义 把 URL 栏变成完整地址，故把光标放过去并全选旧内容 —— 直接输入即替换旧
   * path 而非追加。延后一拍执行，使其发生在切换模式的重渲染（以及浮层关闭）之后。
   */
  const focusUrlForFullAddress = useCallback(() => {
    window.setTimeout(() => {
      const el = urlInputRef.current;
      if (!el) return;
      el.focus();
      try {
        el.select();
      } catch {
        /* 不是文本输入框 —— 无内容可全选 */
      }
    }, 0);
  }, []);

  /** baseUrl 选择器的变更 —— 写到哪里由 `commitBaseUrl` 决定（草稿写文档，已存写行）。 */
  const handleBaseUrlSelectionChange = (next: BaseUrlSelection) => {
    if (!endpoint) return;
    if (next.kind === 'default') {
      // 两者都清 —— baseUrlDefinitionId 为 null 会让 resolveEndpointBaseUrl 回落到项目 ★。
      commitBaseUrl({
        baseUrlDefinitionId: null,
        baseUrlOverride: null,
      });
    } else if (next.kind === 'definition') {
      commitBaseUrl({
        baseUrlDefinitionId: next.definitionId,
        baseUrlOverride: null,
      });
    } else {
      // 自定义 —— `''` 是模式标记而非「清空后的值」：正是它阻止 resolveEndpointBaseUrl
      // 回落到环境 base URL、给用户即将输入的地址加前缀。既存的行可能已带 host。
      const alreadyCustom = selection.kind === 'override';
      commitBaseUrl({
        baseUrlOverride: detail?.baseUrlOverride ?? '',
        baseUrlDefinitionId: null,
      });
      if (!alreadyCustom) focusUrlForFullAddress();
    }
  };

  const method = draft?.method ?? detail?.method ?? 'GET';

  /** 改请求方法 —— 与其它编辑器一样写进草稿。 */
  const handleMethodChange = (m: HttpMethod) => {
    if (endpoint) setEndpointDraft(endpoint.id, { method: m });
  };

  /** 格式化 的结果直接定徽标。 */
  const handleFormatResult = (result: FormatResult) => {
    setValidationError(result.ok ? null : result.error);
  };

  // 当前项目里的全部动态变量定义。这里只要当前值。
  useEffect(() => {
    void loadDynamicVariables(activeProjectId);
  }, [loadDynamicVariables, activeProjectId]);

  const enabledDynamicNames = useMemo(
    () => enabledDynamicNameSet(projectDynamicVariables),
    [projectDynamicVariables],
  );

  const dynamicNames = useMemo(() => {
    if (enabledDynamicNames.size === 0) return [];
    const names = buildDynamicNames(resolved.baseUrl, liveUrl, effective);
    return names.filter((name) => enabledDynamicNames.has(name));
  }, [enabledDynamicNames, resolved.baseUrl, liveUrl, effective]);

  // 模板变化时重新解析（防抖 300 ms）。TTL 内命中缓存时后端不发取数请求。
  useEffect(() => {
    if (dynamicNames.length === 0) return;
    const timer = window.setTimeout(() => {
      void resolveDynamicVariables(dynamicNames, activeProjectId);
    }, 300);
    return () => window.clearTimeout(timer);
  }, [dynamicNames, activeProjectId, resolveDynamicVariables]);

  const dynamicVars = useMemo(
    () => buildDynamicVars(projectDynamicValues, enabledDynamicNames),
    [projectDynamicValues, enabledDynamicNames],
  );

  // 组装请求的唯一入口。发送前若解析出了新的值，用新值再组装一次。
  const prepare = useCallback(
    (dynamic: Record<string, string>) =>
      buildRequestInput(method, liveUrl, resolved.baseUrl, effective, endpointEnv, dynamic),
    [method, liveUrl, resolved.baseUrl, effective, endpointEnv],
  );

  // 请求的最终线上形态，只组装**一次**：发送与 cURL 预览读同一个对象。
  const prepared = useMemo(() => prepare(dynamicVars), [prepare, dynamicVars]);

  // 必须无条件运行的 hook：切走接口会让本组件以更少的 hook 重渲染、React 抛
  // "Rendered fewer hooks than expected"（违反 rules-of-hooks）。所有 hook 都要放在提前返回之上。

  // 组件卸载时中止在途请求。`handleSend` 复用同一个 ref，故 pending 期间再次点击 =
  // 取消上一次重来。
  const inFlightRef = useRef<AbortController | null>(null);
  useEffect(() => {
    return () => {
      inFlightRef.current?.abort();
      inFlightRef.current = null;
    };
  }, []);

  const { sendBtnRef, playIconRef, copyCurlBtnRef } = useHttpTesterMotion(
    requestPending,
    curlFlash,
  );

  // 面包屑回答「我在树的哪里」，但接口一旦拥有工作区标签，那标签已显示名称与集合 hint。
  const endpointHasTab = useAppStore(
    (s) =>
      !!endpoint &&
      s.openTabs.some(
        // 两种接口都算：只匹配 `'endpoint'` 会让刚新建的草稿接口仍留着面包屑。
        (t) => (t.kind === 'endpoint' || t.kind === 'endpoint-draft') && t.id === endpoint.id,
      ),
  );

  const { handleSend, handleSave, handleCopyCurl } = useRequestActions({
    endpointId: endpoint?.id,
    isDraft,
    draft,
    prepared,
    prepareWithDynamic: prepare,
    dynamicNames,
    auth: effective.auth,
    env: scriptEnv,
    preRequestScript: effective.preRequestScript,
    testScript: effective.testScript,
    settings: requestSettings,
    // 端点自己那份 HTTPS 出站设置：`null` = 继承全局默认那份。
    tlsOverride: effective.tls,
    runValidation,
    onPathParamGate: handlePathParamGate,
    setRequestPending,
    setLastResponse,
    setLastScriptOutcome,
    applyEnvWrites,
    addResponseHistory,
    saveDraftEndpoint,
    updateEndpointDetail,
    clearEndpointDraft,
    inFlightRef,
    setCurlFlash,
  });
  // 没有 detail 行的节点是数据漏洞而非正常态（每个接口创建时都带一行）。返回同样的空态。
  if (!endpoint || !detail) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        从左侧选择一个接口开始
      </div>
    );
  }

  const enabledParams = effective.params.filter((p) => p.enabled && p.key).length;
  const enabledHeaders = effective.headers.filter((h) => h.enabled && h.key).length;
  // 只有 schema 的接口也算「body 页有东西」：那份定义同样要看得到。
  const hasBody =
    effective.body.mode !== 'none' && !!(effective.body.content || effective.body.schema);
  const hasAuth = effective.auth.kind !== 'none';
  const hasScript = Boolean(effective.preRequestScript || effective.testScript);
  // 端点带着自己那份 HTTPS 出站设置时，证书那一页挂一枚圆点徽标。
  const hasOwnTls = effective.tls !== null;

  const pathParamError = buildPathParamError({
    sendBlocked,
    missingPathParams: prepared.missingPathParams,
    ownPathParams: pathPlaceholders(liveUrl),
    basePathParams: pathPlaceholders(resolved.baseUrl ?? ''),
  });

  const tabItems = buildTabItems({
    params: enabledParams,
    path: pathParamCount,
    headers: enabledHeaders,
    body: hasBody,
    auth: hasAuth,
    script: hasScript,
    tls: hasOwnTls,
  });

  // 草稿天生未保存，故即使还没有任何编辑器触发过，按钮也必须可用。`dirty` 只按
  // `draft !== undefined` 判定会漏掉「新建、输了 URL、发了请求，现在想留下它」这类接口。
  const dirty = isDraft || draft !== undefined;

  // 外壳只按显式 props 渲染，故这里把它的 props 按区域分组铺开。
  return (
    <TesterShell
      {...{ project, ancestors, endpoint, endpointHasTab, isDraft, namingDraft }}
      {...{ onNamingDraftChange: setNamingDraft, saveEndpointName, saveEndpointDescription }}
      {...{
        patchDraft,
        effective,
        globalTls: requestSettings.tls,
        method,
        resolvedBaseUrl: resolved.baseUrl,
      }}
      {...{ selection, isCustomUrl, liveUrl, urlInputRef }}
      {...{ sendBtnRef, playIconRef, copyCurlBtnRef, requestPending, curlFlash }}
      {...{ dirty, onMethodChange: handleMethodChange, onUrlChange: urlHint.change }}
      {...{ onSend: handleSend, onSave: handleSave }}
      {...{ onCopyCurl: handleCopyCurl, onSelectionChange: handleBaseUrlSelectionChange }}
      {...{ draftSaveError, dynamicError, tabItems, tab, onTabChange: setTab }}
      {...{
        headerSuggestions,
        validationError,
        pathParamError,
        onFormatResult: handleFormatResult,
      }}
      {...{ varEntries, envName: endpointEnv?.name }}
    />
  );
}
