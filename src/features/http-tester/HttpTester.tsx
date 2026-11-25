import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useAppStore, useActiveEndpointResource } from '@/stores/app-store';
import { type BaseUrlSelection } from '@/features/http-tester/BaseUrlPicker';
import { type HttpMethod, type RequestProtocol } from '@/types/domain';
import { type FormatResult } from '@/lib/utils';
import { validateRequest, type RequestProblem } from '@/lib/request-validation';
import { UNTITLED_ENDPOINT_NAME, type EndpointDraft } from '@/stores/store-types';
import { buildHeaderSuggestions } from '@/lib/common-headers';
import { useHttpTesterMotion } from './useHttpTesterMotion';
import { useRequestActions, type SaveRejection } from './useRequestActions';
import { useEndpointContext } from './useEndpointContext';
import { useVarHint, type VarEntry } from './var-hint';
import { pathPlaceholders } from '@/lib/path-params';
import { applyRewrites } from '@/lib/url';
import { buildEndpointRequestParts, prepareEndpointRequest } from './model/endpointParts';
import { buildVarEntries } from './model/varEntries';
import { buildDynamicEntries, buildDynamicVars, enabledDynamicNameSet } from './model/dynamicVars';
import { buildTabItems, type TesterTabId } from './tabs/tabItems';
import { buildPathParamError } from './tabs/pathParamError';
import { pathParamsReady } from './model/path-param-gate';
import { sendActionFor } from './model/send-action';
import { TesterShell } from './tabs/TesterShell';
import { SseStreamPanel } from './SseStreamPanel';
import { WebSocketPanel } from './WebSocketPanel';
import { WsUrlBar } from './WsUrlBar';
import { disposeSession, startSse } from './stream/stream-sessions';
import { useGraphQlSchema } from './graphql/use-graphql-schema';
import { pickQueryHistory, type QueryHistoryEntry } from './graphql/query-history';
import type { GraphQlPanelTools } from './GraphQlPanel';
import { dynamicValueScope } from '@/stores/dynamic-slice';

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
  // 已存接口的补丁被后端拒绝时，store 只回滚不留话；横幅要显示的是后端原话（状态码不合
  // 法之类），故这份消息由本页持有，与 `draftSaveError` 共用同一条横幅。落盘是异步的，
  // 期间用户可能已经切走，故这句话记着自己属于哪个接口，只在该接口上显示。
  const [saveError, setSaveError] = useState<SaveRejection | null>(null);
  const saveErrorHere =
    saveError && saveError.endpointId === endpoint?.id ? saveError.message : null;
  // 在途标记与响应是同一份状态（挂在标签上），故它也按标签取：接口 A 上在途的
  // 请求，不该让 B 的标签也显示「请求中…」，也不该把 B 的 Send 按钮一起锁上。
  const requestPending = useAppStore((s) =>
    s.activeTabId ? (s.lastResponses[s.activeTabId]?.pending ?? false) : false,
  );
  const requestSettings = useAppStore((s) => s.requestSettings);
  // 动态变量挂在项目里，故这一页只认**消费方项目**（当前项目）里的定义：
  // 请求侧不回退到别的项目，同名变量在不同项目里是不同的东西。
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const dynamicVariables = useAppStore((s) => s.dynamicVariables);
  const dynamicValues = useAppStore((s) => s.dynamicValues);
  // 值按 (项目 × 取数环境) 分桶：当前环境那一格才是这次请求要用的值。
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);
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

  const { project, ancestors, endpointEnv, moduleId, selection, isCustomUrl } = useEndpointContext(
    endpoint,
    detail,
  );

  // 变量候选只算当前项目里的定义：别的项目的同名变量不参与这次替换。
  const projectDynamicVariables = useMemo(
    () => dynamicVariables.filter((v) => v.projectId === activeProjectId),
    [dynamicVariables, activeProjectId],
  );
  // 已启用且仍存在的动态变量名。停用或删除之后，那个名字既不算被引用，它的旧值也不
  // 进变量池。
  const enabledDynamicNames = useMemo(
    () => enabledDynamicNameSet(projectDynamicVariables),
    [projectDynamicVariables],
  );

  /**
   * 「这一次要发什么」的推导 —— 与批量运行共用同一份（`model/endpointParts.ts`）。
   * 改那条推导的规则时，两处同时生效。
   */
  const parts = useMemo(
    () =>
      buildEndpointRequestParts({
        detail,
        draft,
        endpointEnv,
        moduleId,
        enabledDynamicNames,
      }),
    [detail, draft, endpointEnv, moduleId, enabledDynamicNames],
  );

  // 实时的 URL —— 输入框此刻的内容：通常是 path，自定义模式下是完整地址。
  const liveUrl = parts.urlPath;
  // 有效请求：已存的 detail 叠上草稿（草稿优先），是唯一决定「用户所见即为何」的地方。
  const effective = parts.effective;
  // 脚本看得到的环境与解析用的是**同一份变量池**（停用的变量不参与）。
  const scriptEnv = parts.scriptEnv;
  // 该接口生效的 baseUrl —— 同时驱动选择器标签与发送时的拼接。
  const resolved = parts.resolved;
  // 「跟随模块」那一行要显示的地址：环境里「环境 × 模块」那一格。
  const moduleBaseUrl = parts.moduleBaseUrl;
  // 路径参数那一页的占位符名单来自**改写之后**的那份路径：改写目标里可能带
  // `{tenant}` 这样的占位符，它会进最终 URL，故它也得有地方填值。
  const pathParamUrl = applyRewrites(liveUrl, resolved.rewrites);

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
  const pathParamCount = useMemo(() => pathPlaceholders(pathParamUrl).length, [pathParamUrl]);

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
   * 把一次 baseUrl 选择提交到这个接口存放它的位置。`baseUrlOverride` 是草稿的
   * **文档**字段，未保存的接口须用 `updateDraftEndpoint` 写：改用
   * `updateEndpointDetail` 对草稿是静默空操作，选择器会弹回 跟随模块、URL 保持无前缀。
   */
  const commitBaseUrl = useCallback(
    (patch: { baseUrlOverride: string | null }) => {
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

  /**
   * 组装期问题的出口：把第一条话钉到既有错误面，并切到出问题的那一页。与 `runValidation`
   * 分开——它看的是替换**之后**的文本，校验期看不到（占位符在校验时一律放行）。
   */
  const handleRequestProblems = useCallback((problem: RequestProblem) => {
    setValidationError(problem.message);
    // 与校验同一条规则：GraphQL 的问题住在查询编辑器里。
    if (problem.tab === 'body') setTab('body');
    else setTab('graphql');
  }, []);

  /** 跑校验，钉住或清除徽标，并回报是否通过。失败时顺带切到出问题的那一页。 */
  const runValidation = useCallback((): boolean => {
    const issue = validateRequest({
      body: effective.body,
      protocol: effective.protocol,
      graphql: effective.graphql,
    });
    setValidationError(issue?.message ?? null);
    if (issue) {
      // GraphQL 的两种问题都出在查询编辑器里，故切到那一页而非 Body。
      if (issue.tab === 'body') setTab('body');
      else setTab('graphql');
      return false;
    }
    return true;
  }, [effective.body, effective.protocol, effective.graphql]);

  // Header 补全从每个已加载 detail 收拢用户自己的 header 名（`Object.values` 会新建对象，
  // 故按该 map 的引用 memo）。
  const headerSuggestions = useMemo(
    () => buildHeaderSuggestions(Object.values(allEndpointDetails)),
    [allEndpointDetails],
  );

  // 变量选择器的条目：环境变量与动态变量并进同一份候选（不新开页签）。
  //
  // 取当前项目里的定义与它们当前项目的取值桶：别的项目的同名变量不参与这次替换。
  const projectDynamicValues = useMemo(
    () => dynamicValues[dynamicValueScope(activeProjectId, activeEnvironmentId || null)] ?? {},
    [dynamicValues, activeProjectId, activeEnvironmentId],
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
    if (next.kind === 'module') {
      // 跟随模块 —— 清掉 override，接口改回读环境里本模块那一格。
      commitBaseUrl({ baseUrlOverride: null });
    } else {
      // 自定义 —— `''` 是模式标记而非「清空后的值」：正是它阻止 resolveEndpointBaseUrl
      // 读模块那一格、给用户即将输入的地址加前缀。既存的行可能已带 host。
      const alreadyCustom = selection.kind === 'override';
      commitBaseUrl({ baseUrlOverride: detail?.baseUrlOverride ?? '' });
      if (!alreadyCustom) focusUrlForFullAddress();
    }
  };

  const method = parts.method;
  const protocol = effective.protocol;

  /**
   * WS 的地址、连接与帧都在「响应」页。进入 WS（切换协议或打开一个 WS 端点）时落到那一页：
   * 页签若停在 Params 上，URL 栏又被面板取代，界面上就没有连接按钮。只做一次 —— 之后用户
   * 去 Params / Auth 编辑属于他的选择，不该被拽回来。
   */
  const wsLandedFor = useRef<string | null>(null);
  useEffect(() => {
    if (protocol !== 'websocket' || !endpoint) {
      wsLandedFor.current = null;
      return;
    }
    if (wsLandedFor.current === endpoint.id) return;
    wsLandedFor.current = endpoint.id;
    setTab('response');
  }, [protocol, endpoint]);

  /** 改请求方法 —— 与其它编辑器一样写进草稿。 */
  const handleMethodChange = (m: HttpMethod) => {
    if (endpoint) setEndpointDraft(endpoint.id, { method: m });
  };

  /**
   * 换协议：写草稿，并关掉这个标签上还在跑的流。SSE 的事件与 WS 的帧属于旧协议，
   * 留着只会让用户以为新协议也在收东西。
   *
   * 方法不动：GraphQL 组装时强制 POST，SSE 允许 GET，故切换不该悄悄改掉用户存的值。
   */
  const handleProtocolChange = (next: RequestProtocol) => {
    if (!endpoint) return;
    disposeSession(endpoint.id);
    setEndpointDraft(endpoint.id, { protocol: next });
    // GraphQL 页签只在协议是 GraphQL 时存在，切走时落回 Body，不从页签条上消失。
    if (next !== 'graphql' && tab === 'graphql') setTab('body');
    if (next === 'graphql' && tab === 'body') setTab('graphql');
  };

  /** 格式化 的结果直接定徽标。 */
  const handleFormatResult = (result: FormatResult) => {
    setValidationError(result.ok ? null : result.error);
  };

  // 当前项目里的全部动态变量定义。这里只要当前值。
  //
  // 切环境也要重载：定义生效的取数环境与当前值都可能变（继承的定义换个环境就是另一份值）。
  useEffect(() => {
    void loadDynamicVariables(activeProjectId);
  }, [loadDynamicVariables, activeProjectId, activeEnvironmentId]);

  // 本次请求引用到的动态变量名。由共享推导给出，与批量运行取的是同一份名单。
  const dynamicNames = parts.dynamicNames;

  // 模板变化时重新解析（防抖 300 ms）。TTL 内命中缓存时后端不发取数请求。
  useEffect(() => {
    if (dynamicNames.length === 0) return;
    const timer = window.setTimeout(() => {
      void resolveDynamicVariables(dynamicNames, activeProjectId);
    }, 300);
    return () => window.clearTimeout(timer);
    // 环境变了要重新取数：值按 (项目 × 环境) 分桶，切到另一格时那一格还是空的。
  }, [dynamicNames, activeProjectId, activeEnvironmentId, resolveDynamicVariables]);

  const dynamicVars = useMemo(
    () => buildDynamicVars(projectDynamicValues, enabledDynamicNames),
    [projectDynamicValues, enabledDynamicNames],
  );

  // 组装请求的唯一入口。发送前若解析出了新的值，用新值再组装一次。
  const prepare = useCallback(
    (dynamic: Record<string, string>) => prepareEndpointRequest(parts, dynamic),
    [parts],
  );

  // 请求的最终线上形态，只组装**一次**：发送与 cURL 预览读同一个对象。
  const prepared = useMemo(() => prepare(dynamicVars), [prepare, dynamicVars]);

  // GraphQL 的 schema 补全与查询历史：都按当前接口取，切接口就是另一份。
  const graphqlSchema = useGraphQlSchema({
    endpointId: endpoint?.id ?? '',
    request: prepared,
    auth: effective.auth,
    settings: requestSettings,
  });
  const historyRows = useAppStore((s) => (endpoint ? s.responseHistory[endpoint.id] : undefined));
  const queryHistory = useMemo(() => pickQueryHistory(historyRows ?? []), [historyRows]);
  const handlePickQueryHistory = useCallback(
    (entry: QueryHistoryEntry) => {
      // 历史行带的是 query / variables / operationName 三段，直接整份写回草稿。
      // 变量在这份快照里已解析成明文（见 `query-history.ts`）：回填的是那次发出的请求。
      patchDraft({ graphql: entry });
    },
    [patchDraft],
  );
  const graphqlTools = useMemo<GraphQlPanelTools>(
    () => ({
      schema: graphqlSchema.schema,
      schemaStatus: graphqlSchema.status,
      schemaError: graphqlSchema.error,
      onFetchSchema: graphqlSchema.refresh,
      history: queryHistory,
      onPickHistory: handlePickQueryHistory,
    }),
    [
      graphqlSchema.schema,
      graphqlSchema.status,
      graphqlSchema.error,
      graphqlSchema.refresh,
      queryHistory,
      handlePickQueryHistory,
    ],
  );

  /**
   * SSE 的 Send：建一条流就返回，没有「响应完成」这个时刻可等，故不走
   * `useRequestActions`（它挂脚本、快照与历史）。事件由 `SseStreamPanel` 从 store 读。
   */
  const handleStartStream = () => {
    if (!endpoint) return;
    if (!runValidation()) return;
    // 组装期问题：替换后的文本坏了，校验期放行过占位符，故这一步不能省。
    const problem = prepared.problems[0];
    if (problem) {
      handleRequestProblems(problem);
      return;
    }
    // 与 HTTP 发送同一道路径参数闸门：带着 `{id}` 建流只会收到 404。拦下时同样切到
    // 「路径参数」页，用户一眼看得见缺哪个。
    if (!pathParamsReady(prepared, handlePathParamGate)) return;
    // 流一建立就把响应页翻出来：事件正往 store 里进，用户该立刻看得见。
    setTab('response');
    void startSse(endpoint.id, {
      request: prepared,
      settings: requestSettings,
      tlsOverride: effective.tls,
      endpointId: endpoint.id,
      auth: effective.auth,
    });
  };

  // 必须无条件运行的 hook：切走接口会让本组件以更少的 hook 重渲染、React 抛
  // "Rendered fewer hooks than expected"（违反 rules-of-hooks）。所有 hook 都要放在提前返回之上。

  // 组件卸载时中止在途请求。`handleSend` 复用同一个标签的那一条，故 pending 期间
  // 再次点击 = 取消上一次重来。
  //
  // **按标签分开**：在途标记也按标签取（见 `stores/slices/ui-response.ts`），若这里
  // 还共用一条 controller，B 的发送就会把 A 的在途请求一起 abort 掉。
  const inFlightRef = useRef<Map<string, AbortController>>(new Map());
  useEffect(() => {
    const controllers = inFlightRef.current;
    return () => {
      for (const ctrl of controllers.values()) ctrl.abort();
      controllers.clear();
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
    protocol,
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
    onRequestProblems: handleRequestProblems,
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
    setSaveError,
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
    ownPathParams: pathPlaceholders(pathParamUrl),
    basePathParams: pathPlaceholders(resolved.baseUrl ?? ''),
  });

  const tabItems = buildTabItems(
    {
      params: enabledParams,
      path: pathParamCount,
      headers: enabledHeaders,
      body: hasBody,
      auth: hasAuth,
      response: effective.responses.length,
      script: hasScript,
      tls: hasOwnTls,
      graphql: Boolean(effective.graphql?.query.trim()),
    },
    protocol === 'graphql',
  );
  // 切走 GraphQL 时页签条上已经没有这一页，内容区不能还挂着一个不存在的页签。
  const panelTab = protocol === 'graphql' || tab !== 'graphql' ? tab : 'body';

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
        moduleBaseUrl,
      }}
      {...{ selection, isCustomUrl, liveUrl, pathParamUrl, urlInputRef }}
      {...{ sendBtnRef, playIconRef, copyCurlBtnRef, requestPending, curlFlash }}
      {...{
        dirty,
        protocol,
        graphql: effective.graphql,
        graphqlTools,
        onProtocolChange: handleProtocolChange,
        onMethodChange: handleMethodChange,
        onUrlChange: urlHint.change,
      }}
      {...{
        onSend: sendActionFor(protocol) === 'stream' ? handleStartStream : handleSend,
        onSave: handleSave,
      }}
      {...{ onCopyCurl: handleCopyCurl, onSelectionChange: handleBaseUrlSelectionChange }}
      {...{
        draftSaveError: draftSaveError ?? saveErrorHere,
        dynamicError,
        tabItems,
        tab: panelTab,
        onTabChange: setTab,
      }}
      {...{
        // WS 的地址栏与面板分两处：地址栏占 HTTP `RequestUrlBar` 的槽位（页签条之上），
        // 面板只替换「响应页」的内容。两者共用同一个 `prepared`。
        wsUrlBar:
          protocol === 'websocket' ? (
            <WsUrlBar
              tabId={endpoint.id}
              protocol={protocol}
              onProtocolChange={handleProtocolChange}
              url={liveUrl}
              onUrlChange={urlHint.change}
              varEntries={varEntries}
              envName={endpointEnv?.name}
              prepared={prepared}
              onPathParamsMissing={handlePathParamGate}
              tls={requestSettings.tls}
              tlsOverride={effective.tls}
              auth={effective.auth}
            />
          ) : undefined,
        wsSurface:
          protocol === 'websocket' ? (
            <WebSocketPanel
              tabId={endpoint.id}
              headers={effective.headers}
              onHeadersChange={(headers) => patchDraft({ headers })}
              headerSuggestions={headerSuggestions}
              varEntries={varEntries}
              envName={endpointEnv?.name}
            />
          ) : undefined,
        responseSurface: protocol === 'sse' ? <SseStreamPanel /> : undefined,
      }}
      {...{
        headerSuggestions,
        validationError,
        pathParamError,
        onFormatResult: handleFormatResult,
      }}
      {...{ varEntries, envName: endpointEnv?.name, vars: prepared.vars }}
    />
  );
}
