/**
 * 请求栏的三个动作：发送、保存、复制 cURL。三者共用同一个 `prepared`，故「用户复制到的
 * 就是服务端收到的」这条不变式成立。本模块只编排顺序，状态与 store 写入由调用点持有。
 */
import { useCallback, type RefObject } from 'react';
import { sendPreparedRequest } from '@/lib/http';
import { preparedToCurl, type PreparedRequest, type RequestProblem } from '@/lib/http-build';
import { copyText } from '@/lib/clipboard';
import { draftToPatch } from '@/stores/slices/crud-helpers';
import { useAppStore } from '@/stores/app-store';
import type { EndpointDraft, RequestSettings } from '@/stores/store-types';
import type { TabUploadProgress } from '@/stores/slices/ui-response';
import type {
  ApiResponse,
  AuthConfig,
  EndpointDetail,
  EnvSnapshot,
  EnvWrite,
  RequestProtocol,
  ScriptOutcome,
  StoredRequestSnapshot,
  TlsSettings,
} from '@/types/domain';
import { pathParamsReady } from './model/path-param-gate';
import { toRequestSnapshot } from './model/requestInput';

/** 一个已存接口的补丁：`nodeId` 由目标行决定，补丁里不带。 */
type EndpointPatch = Partial<Omit<EndpointDetail, 'nodeId'>>;

/**
 * 一次保存被后端拒绝。原因挂在产出它的那个接口上：落盘是异步的，期间用户可能已经切到
 * 别的接口，横幅不该把那句话显示在别人头上。
 */
export interface SaveRejection {
  endpointId: string;
  message: string;
}

/**
 * 返回发送 / 保存 / 复制三个处理器。必须无条件调用——它是 hook。
 */
export function useRequestActions({
  endpointId,
  isDraft,
  draft,
  prepared,
  protocol,
  prepareWithDynamic,
  dynamicNames,
  auth,
  env,
  preRequestScript,
  testScript,
  settings,
  tlsOverride,
  runValidation,
  onPathParamGate,
  onRequestProblems,
  setRequestPending,
  setUploadProgress,
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
}: {
  endpointId: string | undefined;
  isDraft: boolean;
  draft: EndpointDraft | undefined;
  prepared: PreparedRequest;
  /** 这条端点当前的协议。流式协议不走这条发送路径。 */
  protocol: RequestProtocol;
  /**
   * 用给定的动态变量值重新组装请求。发送前若解析出了新的动态变量值，就要用它们重新
   * 组装一次 —— 预览用的是 store 里上一批值，而发出去的那份必须是刚解析出来的那一份。
   */
  prepareWithDynamic: (dynamicVars: Record<string, string>) => PreparedRequest;
  /** 本次请求引用到的动态变量名。 */
  dynamicNames: string[];
  auth: AuthConfig;
  /** 脚本看得到的环境。与解析用的是同一份变量池。 */
  env: EnvSnapshot | undefined;
  /** 端点自己的前置脚本（界面上此刻生效的那份）。 */
  preRequestScript: string;
  /** 端点自己的测试脚本（界面上此刻生效的那份）。 */
  testScript: string;
  settings: RequestSettings;
  /**
   * 端点自己的 HTTPS 出站设置。`null` = 继承全局默认（`settings.tls`）。送的是界面上
   * 此刻生效的那份（草稿优先），故未保存的证书设置也会跑到。
   */
  tlsOverride: TlsSettings | null;
  /** 跑校验并回报是否通过；失败时它已把界面切到出问题的页签。 */
  runValidation: () => boolean;
  /**
   * 路径参数的闸门开了还是关了。收到非空名单 = 闸门关了（缺那几个值，发送已经停下）；
   * 收到空名单 = 闸门开了。界面据此显示提示并切到路径参数那一页。
   */
  onPathParamGate: (missing: string[]) => void;
  /**
   * 组装期问题（`PreparedRequest.problems`）的出口。校验期看不到它们：占位符在校验时一律
   * 放行，`{"id": {{id}}}` + `id=abc` 这种替换后才坏掉的情况只有组装结果知道。收到一条就
   * 说明这一发已经停了，界面据此把话显示到既有错误面并切到出问题的页签。
   */
  onRequestProblems: (problem: RequestProblem) => void;
  /**
   * 标记某个标签上有没有在途请求。传标签 id 而不传接口 id：在途标记与响应是同一份
   * 状态（挂在标签上），两处混用会让「发出去的那次」与「标记在途的那次」不是同一个
   * 标签。
   */
  setRequestPending: (tabId: string, v: boolean) => void;
  /**
   * 记下某个标签这一次发送的传输进度。与 `setRequestPending` 一样按标签写入；响应落地时
   * store 自己会清掉它。
   */
  setUploadProgress: (tabId: string, progress: TabUploadProgress | null) => void;
  /**
   * 记下某个标签最近一次的响应与那次请求的快照。响应是挂在标签上的，写到别的标签上
   * 等于把这次调用挂到了另一个接口的工作区上。
   */
  setLastResponse: (
    tabId: string,
    resp: ApiResponse | null,
    request?: StoredRequestSnapshot | null,
  ) => void;
  /** 记下某个标签最近一次发送的脚本产出（断言 / 日志 / 错误 / 写回的环境变量）。 */
  setLastScriptOutcome: (tabId: string, outcome: ScriptOutcome | null) => void;
  /** 把脚本写回的环境变量并进本地副本（落库由后端做）。 */
  applyEnvWrites: (envId: string, writes: EnvWrite[]) => void;
  addResponseHistory: (
    endpointId: string,
    response: ApiResponse,
    request?: StoredRequestSnapshot,
  ) => void;
  saveDraftEndpoint: (id: string) => Promise<string | null>;
  /** 落盘一个已存接口的补丁；resolve 成后端拒绝的原因，`null` = 已接受。 */
  updateEndpointDetail: (id: string, patch: EndpointPatch) => Promise<string | null>;
  clearEndpointDraft: (id: string) => void;
  /** 记下保存被后端拒绝的原因（`null` = 清掉）。上方横幅据此显示并给重试。 */
  setSaveError: (error: SaveRejection | null) => void;
  /**
   * 在途请求的 controller，**按标签 id 分开**。同一个标签 pending 期间再次点击发送 =
   * 取消上一次重来；换成**别的**标签则各发各的，B 的发送不会把 A 的在途请求一起 abort。
   */
  inFlightRef: RefObject<Map<string, AbortController>>;
  setCurlFlash: (v: boolean) => void;
}) {
  const resolveDynamicVariables = useAppStore((s) => s.resolveDynamicVariables);
  const activeProjectId = useAppStore((s) => s.activeProjectId);

  const handleSend = useCallback(async () => {
    if (!endpointId) return;
    // 流式协议不在这条路径上：SSE 走 `startSse` 建流，WebSocket 由面板收发帧。界面上的
    // 发送按钮已经按 `sendActionFor` 分流，但快捷键与任何直接调用仍可能落到这里，而
    // `ws://` 地址当成 HTTP 发出去只会失败得莫名其妙。
    if (protocol === 'sse' || protocol === 'websocket') return;
    // 校验是请求的前置门：解析不了的 json body 不该让服务端去猜，照发只会让用户对着
    // 服务端解析错误，而这个错误本可以指出他敲错的那个字符。
    if (!runValidation()) return;
    // `handleSend` 先取消**这个标签上**任何在途请求。别的标签各有各的，换一个
    // 接口发送不该把这边那次一起 abort 掉。
    inFlightRef.current.get(endpointId)?.abort();
    const ctrl = new AbortController();
    inFlightRef.current.set(endpointId, ctrl);
    setRequestPending(endpointId, true);
    // 上一次发送留下的进度不能挂在这一次上：新一次从 0 开始，且没有体时它一直是空。
    setUploadProgress(endpointId, null);
    try {
      // 动态变量的值必须在**替换之前**到手：替换一旦发生，变量池里就没有同名变量的
      // 概念了，而「动态变量压过同名环境变量」这条规则正是在替换之前生效的。
      let request = prepared;
      if (dynamicNames.length > 0) {
        // 第二个参数是**消费方项目**：动态变量的定义挂在项目里，故只解析当前项目里的那些。
        // 少了它，store 会跨项目找同名定义，发出去的值可能与预览里显示的不是同一条。
        const { values } = await resolveDynamicVariables(dynamicNames, activeProjectId);
        request = prepareWithDynamic(values);
      }
      // 组装期问题：替换**之后**的真实文本坏了（`variables: {"id": {{id}}}` + `id=abc`），
      // 校验期看到的是放行占位符的模板，挡不住。这一步在动态变量重组装之后——重组装会重算
      // `problems`，之前那份不算数。`body` 为 null 时 `fetch` 不设 `init.body`，不拦就会发出
      // 一个没有 body 的 POST。
      const problem = request.problems[0];
      if (problem) {
        onRequestProblems(problem);
        return;
      }
      // 路径参数缺值时这里不发出去。这一步在动态变量解析**之后**：路径参数的值可能就是一个
      // 动态变量，它取不到值时这个路径参数同样算「还没准备好」。判断与 SSE / WS 建连共用
      // `pathParamsReady`。
      if (!pathParamsReady(request, onPathParamGate)) return;
      // 请求的一切都在 `prepared` 里 —— 后端是纯执行器，不会被告知请求来自哪个接口，这正是
      // 未保存的草稿也能发送的原因。「当前环境」只存在于 Zustand（不落库），故解析基于
      // 工作区当前环境，`prepareRequest` 已经折进去了。
      const {
        response: resp,
        script,
        outbound,
      } = await sendPreparedRequest({
        request,
        // 与 `prepared` 用的是同一份 auth：OAuth2 要靠它取 token，`prepared` 本身
        // 不含（token 只有发出去的那一刻才拿得到）。
        auth,
        settings,
        signal: ctrl.signal,
        // 端点 id：后端据此装配脚本链（collection 的两段 + 端点自己的两段）。
        // 草稿接口没有落库的行，故没有集合链可跑——但它自己的那两段脚本照样跑。
        endpointId,
        // 脚本看得到的环境。与解析用的是同一份变量池。
        env,
        // 端点自己那两段脚本：界面上此刻生效的那份（草稿优先），
        // 后端用它覆盖存的那份，故未保存的编辑也会跑到。
        preRequestScript,
        testScript,
        // 端点自己的 HTTPS 出站设置：`null` = 继承全局默认那份。
        tlsOverride,
        // 传输进度。分片区间来自**这一次**组装的结果（动态变量可能改写过分片内容），
        // 故在 `request` 上取而不是从渲染期那份 `prepared` 里取。
        onProgress: (progress) => {
          // 被顶替 / 取消的那次发送的最后一帧不能写进新的那一次。
          if (ctrl.signal.aborted) return;
          setUploadProgress(endpointId, {
            sentBytes: progress.sentBytes,
            totalBytes: progress.totalBytes,
            serverReceivedBytes: progress.serverReceivedBytes ?? null,
            serverTotalBytes: progress.serverTotalBytes ?? null,
            partRanges: request.multipartPartRanges ?? [],
          });
        },
      });
      // 这里存的是 `outbound`（发出去的那一份），不是 `prepared`（预览那份）：OAuth2 的
      // token 只在 `outbound` 里，历史行要显示的就是它。请求头取后端报上来的线上那
      // 一份（含传输层补的 `host` / `content-length` 等），它缺席时才回落到本地拼的。
      const snapshot = toRequestSnapshot(outbound, resp.requestHeaders);
      // 写的是**这个标签**的那一份：响应是挂在标签上的（见 `slices/ui-response.ts`），
      // 故在 A 上发的不会跑到 B 的标签上。用 `endpointId`（它就是这个标签的 id）而不是
      // 当下的活动标签 —— 那次响应属于**发起它**的那个标签。
      setLastResponse(endpointId, resp, snapshot);
      setLastScriptOutcome(endpointId, script);
      addResponseHistory(endpointId, resp, snapshot);
      // 脚本写回的环境变量：**后端已落库**（那是唯一那条持久化路径），这里只把
      // 本地副本同步过来，于是下一个请求立刻能解析到新变量。
      if (script && script.envWrites.length > 0 && env?.id) {
        applyEnvWrites(env.id, script.envWrites);
      }
    } catch (e) {
      // sendRequest 有文档保证不抛 —— 这个 catch 接住 `buildFetchInit` 之类的意外。
      console.error('[HttpTester] sendRequest threw:', e);
    } finally {
      // `inFlightRef` 只有在仍是当前 controller 时才清空（途中可能被更新的发送顶替）。
      if (inFlightRef.current.get(endpointId) === ctrl) {
        inFlightRef.current.delete(endpointId);
        // 这一次结束了——成功、失败、还是被取消（关标签 / 切走时组件卸载会 abort）。
        // 成功那条路 `setLastResponse` 已经清过一次；取消与意外失败没有响应可落地，
        // 不在这里清，进度条就会永远停在取消那一刻。
        setUploadProgress(endpointId, null);
      }
      setRequestPending(endpointId, false);
    }
  }, [
    endpointId,
    runValidation,
    inFlightRef,
    onPathParamGate,
    onRequestProblems,
    prepared,
    prepareWithDynamic,
    protocol,
    dynamicNames,
    resolveDynamicVariables,
    activeProjectId,
    auth,
    env,
    preRequestScript,
    testScript,
    settings,
    tlsOverride,
    setRequestPending,
    setUploadProgress,
    setLastResponse,
    setLastScriptOutcome,
    applyEnvWrites,
    addResponseHistory,
  ]);

  const handleSave = useCallback(() => {
    if (!endpointId) return;
    // 与发送同门：持久化一个解析不了的 json body 等于故意存下坏请求。校验失败时草稿
    // 原样留着 —— 不写不清。
    if (!runValidation()) return;

    // 未保存的接口没有可 patch 的行，故整次写入一步完成：`saveDraftEndpoint` 把文档
    // 字段与待存的请求配置编辑并成一个 `detail` 写入，并把草稿标签换成真实接口标签。
    // 写入失败时保留草稿（与本标签），重试就是再按一次保存，错误经 `draftSaveError` 呈现。
    if (isDraft) {
      void saveDraftEndpoint(endpointId);
      return;
    }

    const patch = draft ? draftToPatch(draft) : {};
    if (Object.keys(patch).length === 0) {
      clearEndpointDraft(endpointId);
      return;
    }
    // 已存接口的补丁由后端落盘，而响应定义的状态码等只在那里判。被拒时草稿必须留着：
    // 清掉它等于让用户刚填的定义静静消失，界面上看不出刚才那下什么都没写进去。
    const submitted = draft;
    void updateEndpointDetail(endpointId, patch).then((rejected) => {
      if (rejected !== null) {
        setSaveError({ endpointId, message: `保存被后端拒绝：${rejected}` });
        return;
      }
      setSaveError(null);
      // 落盘期间用户还能改字段。改过就说明手上这份草稿比刚才提交的新，清掉等于把那次
      // 编辑一起丢掉，故只在它还是提交时那一份时才清。
      if (useAppStore.getState().endpointDrafts[endpointId] === submitted) {
        clearEndpointDraft(endpointId);
      }
    });
  }, [
    endpointId,
    runValidation,
    isDraft,
    draft,
    saveDraftEndpoint,
    updateEndpointDetail,
    clearEndpointDraft,
    setSaveError,
  ]);

  const handleCopyCurl = useCallback(async () => {
    // 命令由 `prepared`（发送所用的那个对象）渲染，故不可能描述另一个请求。复制不属
    // 于校验路径，body 解析不了也照样给命令 —— 这是复制而非请求，拒绝只会碍事。
    const curl = preparedToCurl(prepared);
    // 两条通道都失败：先打到 console（还没有 toast 组件），用户可退而手动选中文本。
    if (!(await copyText(curl))) {
      console.error('[HttpTester] copy as cURL failed: clipboard unavailable');
      return;
    }
    setCurlFlash(true);
    window.setTimeout(() => setCurlFlash(false), 1500);
  }, [prepared, setCurlFlash]);

  return { handleSend, handleSave, handleCopyCurl };
}
