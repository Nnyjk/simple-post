import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useAppStore, useActiveEndpointResource } from '@/stores/app-store';
import { Tabs } from '@/components/ui/tabs';
import { type BaseUrlSelection } from '@/features/http-tester/BaseUrlPicker';
import { type AuthConfig, type EnvSnapshot, type RequestBody } from '@/types/domain';
import { validateRequest } from '@/lib/request-validation';
import { prepareRequest } from '@/lib/http-build';
import { enabledVars } from '@/lib/url';
import { UNTITLED_ENDPOINT_NAME, type EndpointDraft } from '@/stores/store-types';
import { buildHeaderSuggestions } from '@/lib/common-headers';
import { EndpointHeader } from './EndpointHeader';
import { EndpointBreadcrumb } from './EndpointBreadcrumb';
import { RequestUrlBar } from './RequestUrlBar';
import { useHttpTesterMotion } from './useHttpTesterMotion';
import { useUrlVarPicker } from './useUrlVarPicker';
import { useRequestActions } from './useRequestActions';
import { RequestTabPanels } from './RequestTabPanels';
import { useEndpointContext } from './useEndpointContext';
import { EnvVarPicker, type VarEntry } from './EnvVarPicker';
import { listDynamicNames } from '@/lib/dynamic-vars';
import { pathPlaceholders } from '@/lib/path-params';

const TABS: { id: TabId; label: string; disabled?: boolean }[] = [
  { id: 'params', label: 'Params' },
  { id: 'path', label: '路径参数' },
  { id: 'headers', label: 'Headers' },
  { id: 'body', label: 'Body' },
  { id: 'auth', label: 'Auth' },
  { id: 'docs', label: 'Docs' },
  { id: 'script', label: '脚本' },
];

/**
 * 缺失 `endpoint_details` 行时的占位，让 effective 请求 memo 有类型可读。
 * 按引用共享：所有编辑器都是先展开再写，不会就地修改。
 */
const EMPTY_BODY: RequestBody = { mode: 'none', content: '' };
const EMPTY_AUTH: AuthConfig = { kind: 'none' };

type TabId = 'params' | 'path' | 'headers' | 'body' | 'auth' | 'docs' | 'script';

export function HttpTester() {
  // 接口是 `ResourceNode`（身份 + 位置），请求配置是独立的 `EndpointDetail` 行；
  // 两个 selector 都返回 store 里的稳定引用，不会新建对象。
  //
  // `useActiveEndpointResource` 对草稿接口同样有答案：未保存的接口两行都没有，
  // hook 从 `DraftEndpoint` 合成二者。故以下代码只写一遍「用户正在看的接口」，
  // 不按存储形式各写一份。
  const resource = useActiveEndpointResource();
  const endpoint = resource?.node ?? null;
  const detail = resource?.detail;
  /** 接口尚未写入数据库。 */
  const isDraft = resource?.isDraft ?? false;
  // 供 Header 键补全：把用户自己用过的 header 名与内置常见列表一起喂给 datalist
  // （见 buildHeaderSuggestions）。
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
  // 动态变量：本项目的定义与它们当前的取值。
  const dynamicVariables = useAppStore((s) => s.dynamicVariables);
  const dynamicValues = useAppStore((s) => s.dynamicValues);
  const loadDynamicVariables = useAppStore((s) => s.loadDynamicVariables);
  const resolveDynamicVariables = useAppStore((s) => s.resolveDynamicVariables);
  // 动态变量的定义没加载上来 / 取数调用失败时的原因。
  const dynamicError = useAppStore((s) => s.dynamicError);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);

  const [tab, setTab] = useState<TabId>('params');
  const [curlFlash, setCurlFlash] = useState(false);

  // URL/method 草稿存在 store（`endpointDrafts[endpointId]`）而非本地 state，切走再
  // 切回标签才留住未提交的编辑。store 会把草稿的存在同步镜像进 `dirtyTabIds`，
  // 标签条的脏点因此自动更新。
  //
  // 用专用 selector 读草稿，使 HttpTester 只在**自己**接口的草稿变化时重渲染
  // （A 激活时 B 上的草稿不该影响这里）。
  const draft = useAppStore((s) => (endpoint ? s.endpointDrafts[endpoint.id] : undefined));
  // 名称 / 描述的编辑归 `<InlineEdit>`（以 `endpoint.id` 为 key，激活接口一变编辑器
  // 即卸载重置，无需在这里手动「切标签时清编辑状态」）。

  const urlInputRef = useRef<HTMLInputElement>(null);

  // 实时的 URL —— 输入框此刻的内容。通常只是 path（baseUrl 之后那段），自定义模式下
  // 是完整地址。取自 store 侧的草稿（切标签不丢），回落到已持久化的 detail URL。
  const liveUrl = draft?.url ?? detail?.url ?? '';

  // 项目 / 祖先链 / 环境 / 模块 / 有效 baseUrl 与选择器状态 —— 全是 store 的派生，
  // 合成一个 hook，让本组件只留状态编排与 JSX。
  const { project, ancestors, endpointEnv, resolved, selection, isCustomUrl } = useEndpointContext(
    endpoint,
    detail,
  );

  // 动态变量的缓存键含环境，故这里用的是**这个接口**解析 `{{x}}` 所依据的那份环境
  // （接口自选的环境，没有就用工作区当前环境）——与 `endpointEnv` 同一个来源。
  const dynamicEnvId = endpointEnv?.id ?? activeEnvironmentId;

  // 脚本看得到的环境。与解析用的是**同一份变量池**（停用的变量不参与），
  // 否则 `pm.environment.get('token')` 读到的与请求里替换进去的不是同一个值。
  const scriptEnv = useMemo<EnvSnapshot | undefined>(() => {
    if (!endpointEnv) return undefined;
    return { id: endpointEnv.id, variables: enabledVars(endpointEnv) };
  }, [endpointEnv]);

  // ---------------------------------------------------------------------------
  // 有效请求：已存的 detail 叠上草稿。
  //
  // 所有请求配置编辑器都写草稿而非该行，故这里是唯一决定「用户所见即为何」的地方。
  // 发送、保存、cURL 预览与校验都读它 —— 只派生一次，才不会让这四者对请求各说各话。
  //
  // URL / method 同理（见上方 `liveUrl` / `method`），但不放进本对象：URL 栏还要原始值
  // 做自己的选择器与光标逻辑。
  // ---------------------------------------------------------------------------
  const effective = useMemo(
    () => ({
      params: draft?.params ?? detail?.params ?? [],
      pathParams: draft?.pathParams ?? detail?.pathParams ?? [],
      headers: draft?.headers ?? detail?.headers ?? [],
      body: draft?.body ?? detail?.body ?? EMPTY_BODY,
      auth: draft?.auth ?? detail?.auth ?? EMPTY_AUTH,
      docs: draft?.docs ?? detail?.docs ?? '',
      tags: draft?.tags ?? detail?.tags ?? [],
      // 两段脚本。草稿优先，回落到已存的那份。
      preRequestScript: draft?.preRequestScript ?? detail?.preRequestScript ?? '',
      testScript: draft?.testScript ?? detail?.testScript ?? '',
    }),
    [draft, detail],
  );

  /** 把某个编辑器的产出写进草稿（从而置脏）。 */
  const patchDraft = useCallback(
    (patch: Partial<EndpointDraft>) => {
      if (!endpoint) return;
      setEndpointDraft(endpoint.id, patch);
    },
    [endpoint, setEndpointDraft],
  );

  // URL 路径里有几个占位符 —— 路径参数那一页要列几行。
  // 这个 useMemo 必须在提前返回**之上**：条件调用的 hook 会让 React 在切换接口时
  // 抛「Rendered fewer hooks than expected」。
  const pathParamCount = useMemo(() => pathPlaceholders(liveUrl).length, [liveUrl]);

  // ---------------------------------------------------------------------------
  // 草稿的文档字段 vs 已保存节点的
  //
  // 草稿没有 `nodes` 行，对它 `updateNode` 会打在不存在的行上 —— 其文档字段存在
  // `draftEndpoints`，经 `saveDraftEndpoint` 落库，不走这里。两种写法都收敛到一个
  // callback，重命名控件就不必知道自己坐在哪类接口上。
  // ---------------------------------------------------------------------------
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
   * 把一次 baseUrl 选择提交到这个接口存放它的位置。
   *
   * `baseUrlDefinitionId` / `baseUrlOverride` 是草稿的**文档**字段
   * （见 `slices/ui-draft-endpoints.ts`），未保存的接口须用 `updateDraftEndpoint` 写，
   * `saveDraftEndpoint` 会把它们并进它持久化的那一行。改用 `updateEndpointDetail`
   * 对草稿是静默空操作：该动作在没有 `endpoint_details` 行时提前返回，选择器会弹回
   * 项目默认、URL 保持无前缀，点击看起来毫无效果。
   *
   * 已保存的接口仍立即落库：此选择没有 Save 按钮，接口行是它唯一能待的地方。
   */
  const commitBaseUrl = useCallback(
    (patch: { baseUrlDefinitionId: string | null; baseUrlOverride: string | null }) => {
      if (!endpoint) return;
      if (isDraft) updateDraftEndpoint(endpoint.id, patch);
      else updateEndpointDetail(endpoint.id, patch);
    },
    [endpoint, isDraft, updateDraftEndpoint, updateEndpointDetail],
  );

  // 新建即命名 —— 新草稿打开时标题已选中。
  //
  // 草稿没有行可供树把 `editing` 指向，故该便利性搬到这里、搬进真正读名称的界面；
  // 否则「新建接口」会落在写着 未命名接口 的标签上，每个新建接口都多一次点击。
  //
  // 按草稿 id 上闩而非按名称驱动：条件是「这个草稿仍是占位名」，否则用户回到一个
  // 故意不命名的草稿时编辑器会反复弹出。
  const [namingDraft, setNamingDraft] = useState(false);
  const autoNameOpenedFor = useRef<string | null>(null);
  useEffect(() => {
    if (!isDraft || !endpoint) return;
    if (endpoint.name !== UNTITLED_ENDPOINT_NAME) return;
    if (autoNameOpenedFor.current === endpoint.id) return;
    autoNameOpenedFor.current = endpoint.id;
    setNamingDraft(true);
  }, [isDraft, endpoint]);

  // 钉住的格式错误。见 `lib/request-validation`：校验是 发送 / 保存 / 格式化 触发的
  // 动作，通过即清除。刻意留在本组件：Params / Body / Auth 之间切换时它存活
  // （BodyEditor 会重挂载而它不会），下次发送/保存重新检查当时的界面内容。
  const [validationError, setValidationError] = useState<string | null>(null);

  /**
   * 上一次发送是不是被路径参数缺值拦下了。
   *
   * 只记「拦下过」这一件事，缺哪几个由**当下**的请求状态算出来：用户补上值之后提示
   * 立刻消失，改 URL 让占位符对不上时提示也跟着改。另存一份文案就会留下过期提示。
   */
  const [sendBlocked, setSendBlocked] = useState(false);

  /**
   * 路径参数的闸门开了还是关了。闸门关着时切到路径参数那一页，用户一眼看得见缺哪个。
   */
  const handlePathParamGate = useCallback((missing: string[]) => {
    setSendBlocked(missing.length > 0);
    if (missing.length > 0) setTab('path');
  }, []);

  /**
   * 跑校验，钉住或清除徽标，并回报是否通过。
   *
   * 失败时顺带切到出问题的那一页 —— 用户停在 Params 时把错误钉在 body 编辑器里，
   * 正是这个徽标要终结的那种看不见。
   */
  const runValidation = useCallback((): boolean => {
    const issue = validateRequest({ body: effective.body });
    setValidationError(issue?.message ?? null);
    if (issue) {
      if (issue.tab === 'body') setTab('body');
      return false;
    }
    return true;
  }, [effective.body]);

  // Header 补全从每个已加载 detail 收拢用户自己的 header 名。`Object.values` 会新建
  // 对象，故按该 map 的引用 memo，而不是每次渲染重建。
  const headerSuggestions = useMemo(
    () => buildHeaderSuggestions(Object.values(allEndpointDetails)),
    [allEndpointDetails],
  );

  // 变量选择器的条目：环境变量与动态变量并进同一份候选（不新开页签）。
  const dynamicEntries = useMemo<VarEntry[]>(() => {
    const bucket = dynamicValues[dynamicEnvId] ?? {};
    return dynamicVariables.map((v) => {
      // 停用的定义不参与替换，故它不显示值——显示一个用不上的值会误导。
      if (!v.enabled) {
        return { key: v.name, value: '', origin: 'dynamic' as const, hint: '已停用' };
      }
      const status = bucket[v.name];
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
  }, [dynamicVariables, dynamicValues, dynamicEnvId]);

  // EnvVarPicker 状态 —— 用户在 URL 输入框敲 `{{` 时打开。选择器是纯展示：选中项与
  // 过滤条目据当前环境 + 最后一个 `{{` 之后输入的片段派生，写回仍由本组件的草稿完成。
  const {
    varPickerOpen,
    varSelected,
    filteredEntries,
    setVarSelected,
    setVarPickerOpen,
    handleUrlChange,
    handleUrlKeyDown,
    insertVar,
    focusUrlForFullAddress,
  } = useUrlVarPicker({
    endpointEnv,
    dynamicEntries,
    liveUrl,
    urlInputRef,
    onUrlChange: (next) => {
      if (!endpoint) return false;
      setEndpointDraft(endpoint.id, { url: next });
      return true;
    },
  });

  /** baseUrl 选择器的变更 —— 落点由 `commitBaseUrl` 决定（草稿写文档，已存写行）。 */
  const handleBaseUrlSelectionChange = (next: BaseUrlSelection) => {
    if (!endpoint) return;
    if (next.kind === 'default') {
      // 两者都清 —— baseUrlDefinitionId 为 null 会让 resolveEndpointBaseUrl 回落到
      // 项目 ★。
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
      // 自定义 —— `''` 是模式标记而非「清空后的值」：正是它阻止
      // resolveEndpointBaseUrl（以及 Rust 侧解析器）回落到环境 base URL、给用户
      // 即将输入的地址加前缀。既存的行可能已带 host，保留它，以免重选 自定义 时
      // 意外改指已有请求。
      const alreadyCustom = selection.kind === 'override';
      commitBaseUrl({
        baseUrlOverride: detail?.baseUrlOverride ?? '',
        baseUrlDefinitionId: null,
      });
      if (!alreadyCustom) focusUrlForFullAddress();
    }
  };

  // 草稿与当前激活的接口同步
  const method = draft?.method ?? detail?.method ?? 'GET';

  // ---------------------------------------------------------------------------
  // 动态变量
  // ---------------------------------------------------------------------------

  // 本项目的动态变量定义。面板与请求编辑器都读同一份，故两处看到的东西一样。
  useEffect(() => {
    if (activeProjectId === '') return;
    void loadDynamicVariables(activeProjectId, dynamicEnvId || undefined);
  }, [activeProjectId, dynamicEnvId, loadDynamicVariables]);

  // 本项目**已启用且仍存在**的动态变量名。
  //
  // `dynamicNames`（扫出谁被引用了）与 `dynamicVars`（把值并进变量池）共用它：停用或删除
  // 之后，那个名字既不算被引用，它的旧值也不进变量池——否则「停用后不参与替换」是假的，
  // 界面预览与真正发出的请求还会描述两个不同的请求。
  const enabledDynamicNames = useMemo(
    () => new Set(dynamicVariables.filter((v) => v.enabled).map((v) => v.name)),
    [dynamicVariables],
  );

  // 这个请求引用了哪些动态变量。
  //
  // 扫的是**未替换**的模板，故同名环境变量也会被扫出来——动态变量在替换时压过环境变量
  // （见 `mergePool`），故名单里只留本项目有定义且已启用的那些。
  const dynamicNames = useMemo(() => {
    if (enabledDynamicNames.size === 0) return [];
    return listDynamicNames({
      baseUrl: resolved.baseUrl,
      url: liveUrl,
      params: effective.params,
      headers: effective.headers,
      body: effective.body,
      auth: effective.auth,
    }).filter((name) => enabledDynamicNames.has(name));
  }, [enabledDynamicNames, resolved.baseUrl, liveUrl, effective]);

  // 模板或环境变化时重新解析（防抖 300 ms）。TTL 内命中缓存时后端不发取数请求，
  // 故这一步通常只是一次廉价的 IPC；预览里的 `{{name}}` 也因此很快变成真值。
  useEffect(() => {
    if (dynamicNames.length === 0) return;
    const timer = window.setTimeout(() => {
      void resolveDynamicVariables(activeProjectId, dynamicNames, dynamicEnvId || undefined);
    }, 300);
    return () => window.clearTimeout(timer);
  }, [dynamicNames, activeProjectId, dynamicEnvId, resolveDynamicVariables]);

  // 当前生效的动态变量值。它们在变量池的最后一步并入，故同名时压过环境变量；
  // 被停用的环境变量也不影响它们（停用过滤只作用于环境变量）。
  //
  // 桶里只留下**已启用且仍存在**的那些名字：桶是累加的，停用或删除之后旧条目还在，
  // 直接并进去就等于停用之后仍然在替换。
  const dynamicVars = useMemo(() => {
    const bucket = dynamicValues[dynamicEnvId] ?? {};
    const out: Record<string, string> = {};
    for (const [name, status] of Object.entries(bucket)) {
      if (!enabledDynamicNames.has(name)) continue;
      if (status.value !== null && status.value !== undefined) out[name] = status.value;
    }
    return out;
  }, [dynamicValues, dynamicEnvId, enabledDynamicNames]);

  // 组装请求的唯一入口。发送前若解析出了新的值，用新值再组装一次。
  const prepare = useCallback(
    (dynamic: Record<string, string>) =>
      prepareRequest({
        method,
        urlPath: liveUrl,
        baseUrl: resolved.baseUrl,
        params: effective.params,
        pathParams: effective.pathParams,
        headers: effective.headers,
        body: effective.body,
        auth: effective.auth,
        env: endpointEnv,
        dynamicVars: dynamic,
      }),
    [method, liveUrl, resolved.baseUrl, effective, endpointEnv],
  );

  // 请求的最终线上形态，只组装**一次**。
  //
  // 发送与 cURL 预览读同一个对象 —— 这正是此安排的全部意义：预览现在是发送所发字节的
  // 序列化，不可能描述出另一个请求。
  const prepared = useMemo(() => prepare(dynamicVars), [prepare, dynamicVars]);

  // ---------------------------------------------------------------------------
  // 必须无条件运行的 hook。切走接口会让本组件以更少的 hook 重渲染、React 抛
  // "Rendered fewer hooks than expected"（违反 rules-of-hooks）。所有 hook 都要放在
  // 下方 `if (!endpoint)` 提前返回之上。
  // ---------------------------------------------------------------------------

  // 组件卸载（切标签、换项目等）时中止在途请求，免得挂起的 Promise 比发起它的 UI
  // 活得更久。`handleSend` 复用同一个 ref，故 pending 期间再次点击 = 取消上一次重来，
  // 而不是无操作的竞态。
  const inFlightRef = useRef<AbortController | null>(null);
  useEffect(() => {
    return () => {
      inFlightRef.current?.abort();
      inFlightRef.current = null;
    };
  }, []);

  // C2/C3 的反馈动画与请求无关，refs 与动画一起搬进了这个 hook。
  const { sendBtnRef, playIconRef, copyCurlBtnRef } = useHttpTesterMotion(
    requestPending,
    curlFlash,
  );

  // 面包屑回答「我在树的哪里」，但接口一旦拥有工作区标签，那标签已显示名称与集合
  // hint，面包屑重复的是同一屏上一行高的信息。直接从 selector 读成布尔（避免新建
  // 对象）而非由「tester 是否在渲染」推导，好让没有标签可承载答案的路径上仍能显示。
  // 置于提前返回之上，理由同本块其余 hook。
  const endpointHasTab = useAppStore(
    (s) =>
      !!endpoint &&
      s.openTabs.some(
        // 两种接口都算。草稿接口同样拥有标签、同样显示在条上 —— 只匹配 `'endpoint'`
        // 会让最可能刚新建的那类接口仍留着面包屑。
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
    environmentId: dynamicEnvId,
    auth: effective.auth,
    env: scriptEnv,
    preRequestScript: effective.preRequestScript,
    testScript: effective.testScript,
    settings: requestSettings,
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
  // 没有 detail 行的节点是数据漏洞而非正常态（每个接口创建时都带一行）。返回同样的
  // 空态，而不是渲染半残的 tester。
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

  // 路径参数缺值的提示。文案按当下的请求状态算，闸门开着（没被拦下）时没有提示。
  //
  // 缺值可能来自两处：端点自己的路径（「路径参数」那一页有对应的行可填），或前置
  // URL（服务地址）。后者在那一页没有行可填，提示必须说清去哪儿改 —— 否则用户被
  // 拦在一扇打不开的门外面。
  const ownPathParams = pathPlaceholders(liveUrl);
  const basePathParams = pathPlaceholders(resolved.baseUrl ?? '');
  const missingInBase = prepared.missingPathParams.filter(
    (n) => !ownPathParams.includes(n) && basePathParams.includes(n),
  );
  const missingInPath = prepared.missingPathParams.filter((n) => !missingInBase.includes(n));
  const pathParamError =
    sendBlocked && prepared.missingPathParams.length > 0
      ? missingInBase.length > 0
        ? `前置 URL 里的 ${missingInBase.join('、')} 不是端点自己的路径参数，请把前置 URL 改成 {{变量}} 或直接写死`
        : `路径参数 ${missingInPath.join('、')} 没有填值，填好才能发送`
      : null;

  const tabItems = TABS.map((t) => {
    let badge: React.ReactNode = null;
    if (t.id === 'params' && enabledParams)
      badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledParams}</span>;
    if (t.id === 'path' && pathParamCount)
      badge = <span className="rounded bg-muted px-1 text-[10px]">{pathParamCount}</span>;
    if (t.id === 'headers' && enabledHeaders)
      badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledHeaders}</span>;
    if (t.id === 'body' && hasBody)
      badge = <span className="rounded bg-primary/20 px-1 text-[10px] text-primary">●</span>;
    if (t.id === 'auth' && hasAuth)
      badge = (
        <span className="rounded bg-warning/20 px-1 text-[10px] text-warning-foreground">●</span>
      );
    if (t.id === 'script' && hasScript)
      badge = <span className="rounded bg-primary/20 px-1 text-[10px] text-primary">●</span>;
    return { id: t.id, label: t.label, badge, disabled: t.disabled };
  });

  // 草稿天生未保存，故即使还没有任何编辑器触发过，按钮也必须可用。只按
  // `draft !== undefined` 判定，恰好会卡住这个功能要服务的接口：用户新建、输了 URL、
  // 发了请求，现在想留下它 —— 却没有任何 params / headers / body 编辑把它变脏。
  const dirty = isDraft || draft !== undefined;

  return (
    <section className="flex h-full min-w-0 flex-1 flex-col">
      {/* 面包屑 —— 与设置面板的 `设置 › 连接` 同一写法，在一屏内回答「我在树的哪里」。
          工作区标签承载同样信息时隐藏（见 `endpointHasTab`）。 */}
      {!endpointHasTab && (
        <EndpointBreadcrumb project={project} ancestors={ancestors} endpoint={endpoint} />
      )}

      {/* Endpoint header */}
      <EndpointHeader
        endpoint={endpoint}
        isDraft={isDraft}
        namingDraft={namingDraft}
        onNamingDraftChange={setNamingDraft}
        tags={effective.tags}
        saveEndpointName={saveEndpointName}
        saveEndpointDescription={saveEndpointDescription}
        patchDraft={patchDraft}
      />

      {/* URL bar */}
      <RequestUrlBar
        method={method}
        definitions={project?.baseUrlDefinitions ?? []}
        defaultDefinitionId={project?.defaultBaseUrlDefinitionId ?? null}
        resolvedBaseUrl={resolved.baseUrl}
        selection={selection}
        isCustomUrl={isCustomUrl}
        liveUrl={liveUrl}
        urlInputRef={urlInputRef}
        sendBtnRef={sendBtnRef}
        playIconRef={playIconRef}
        copyCurlBtnRef={copyCurlBtnRef}
        requestPending={requestPending}
        curlFlash={curlFlash}
        dirty={dirty}
        isDraft={isDraft}
        onMethodChange={(m) => {
          if (endpoint) setEndpointDraft(endpoint.id, { method: m });
        }}
        onSelectionChange={handleBaseUrlSelectionChange}
        onUrlChange={handleUrlChange}
        onUrlKeyDown={handleUrlKeyDown}
        onSend={handleSend}
        onSave={handleSave}
        onCopyCurl={handleCopyCurl}
      />

      {/* 草稿保存失败会保留草稿与标签，重试就是再按一次保存 —— 但用户必须知道刚才那下
          什么都没做，否则点击看起来是坏的。`addNode` 把真实错误吞成 `null` 只记日志，
          故这是 UI 能给的唯一信号，重试入口就是这个按钮。 */}
      {draftSaveError && (
        <div
          role="alert"
          className="flex shrink-0 items-center gap-2 border-b border-border bg-destructive/10 px-4 py-1.5 text-xs text-destructive"
          data-testid="draft-save-error"
        >
          <span>{draftSaveError}</span>
          <button
            type="button"
            className="underline underline-offset-2 hover:no-underline"
            onClick={handleSave}
          >
            重试
          </button>
        </div>
      )}

      {/* 动态变量的定义没加载上来时，请求里那些 `{{name}}` 不会被替换，而界面上没有任何
          提示——用户只看到服务端回了一个与真实原因无关的错误。故这里把那条原因摆出来。 */}
      {dynamicError && (
        <div
          role="alert"
          className="flex shrink-0 items-center gap-2 border-b border-border bg-destructive/10 px-4 py-1.5 text-xs text-destructive"
          data-testid="dynamic-error"
        >
          <span>{dynamicError}</span>
        </div>
      )}

      {/* Tabs —— 与下方面板相连：本行独占那条分隔线（条内不再有），激活标签以面板底色
          填充自己，盖住分隔线并与之圆角相接。`bg-card/30` 与上方 URL 栏一致，也是
          激活标签得以凸显的背景 —— 少了它标签与面板同色，效果就成了「少一条线」而非
          「标签并入面板」。`pt-1.5` 不配 `pb`，使条的下沿与分隔线齐平，标签上的
          `-mb-px` 才能正好压在其上。 */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/30 px-4 pt-1.5">
        <Tabs
          variant="connected"
          joinStripClass="bg-card/30"
          items={tabItems}
          value={tab}
          onChange={(id) => setTab(id as TabId)}
        />
        <div className="ml-auto text-[10px] text-muted-foreground">
          <span className="kbd">Ctrl</span> + <span className="kbd">Enter</span> 发送
        </div>
      </div>

      {/* Tab content */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <RequestTabPanels
          tab={tab}
          params={effective.params}
          pathParams={effective.pathParams}
          url={liveUrl}
          headers={effective.headers}
          body={effective.body}
          auth={effective.auth}
          docs={effective.docs}
          preRequestScript={effective.preRequestScript}
          testScript={effective.testScript}
          headerSuggestions={headerSuggestions}
          validationError={validationError}
          pathParamError={pathParamError}
          patchDraft={patchDraft}
          onFormatResult={(result) => setValidationError(result.ok ? null : result.error)}
        />
      </div>

      {/* Env 变量选择器 —— 锚在 URL 输入框上，在选择器内部用 getBoundingClientRect
          定位，故滚动 / resize 时跟随输入框。 */}
      <EnvVarPicker
        open={varPickerOpen}
        onClose={() => setVarPickerOpen(false)}
        entries={filteredEntries}
        selected={varSelected}
        onSelectedChange={setVarSelected}
        onSelect={(key) => insertVar(key)}
        anchorRef={urlInputRef}
        envName={endpointEnv?.name}
      />
    </section>
  );
}
