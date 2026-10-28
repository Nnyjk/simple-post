/**
 * 请求栏的三个动作：发送、保存、复制 cURL。
 *
 * 三者共用同一个 `prepared`（请求的最终线上形态），故「用户复制到的就是服务端
 * 收到的」这条不变式在这里成立。本模块只编排顺序——校验门、在途请求的取消与清空、
 * 复制 cURL 的反馈；状态与 store 写入全部由调用点持有。
 */
import { useCallback, type RefObject } from 'react';
import { sendPreparedRequest } from '@/lib/http';
import { preparedToCurl, type PreparedRequest } from '@/lib/http-build';
import { draftToPatch } from '@/stores/slices/crud-helpers';
import { useAppStore } from '@/stores/app-store';
import type { EndpointDraft, RequestSettings } from '@/stores/store-types';
import type {
  ApiResponse,
  AuthConfig,
  EnvSnapshot,
  EnvWrite,
  ScriptOutcome,
  TlsSettings,
} from '@/types/domain';

/** 与 store 里 `setLastResponse` / `addResponseHistory` 相同的形状。 */

/**
 * 返回发送 / 保存 / 复制三个处理器。必须无条件调用——它是 hook。
 */
export function useRequestActions({
  endpointId,
  isDraft,
  draft,
  prepared,
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
}: {
  endpointId: string | undefined;
  isDraft: boolean;
  draft: EndpointDraft | undefined;
  prepared: PreparedRequest;
  /**
   * 用给定的动态变量值重新组装请求。
   *
   * 发送前若解析出了新的动态变量值，就要用它们重新组装一次——预览用的是 store 里
   * 上一批值，而发出去的那份必须是刚解析出来的那一份。
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
   * 端点自己的 HTTPS 出站设置。`null` = 继承全局默认（`settings.tls`）。
   *
   * 送的是界面上此刻生效的那份（草稿优先），故未保存的证书设置也会跑到。
   */
  tlsOverride: TlsSettings | null;
  /** 跑校验并回报是否通过；失败时它已把界面切到出问题的页签。 */
  runValidation: () => boolean;
  /**
   * 路径参数的闸门开了还是关了。
   *
   * 收到非空名单 = 闸门关了（缺那几个值，发送已经停下）；收到空名单 = 闸门开了。
   * 界面据此显示提示并切到路径参数那一页。
   */
  onPathParamGate: (missing: string[]) => void;
  setRequestPending: (v: boolean) => void;
  setLastResponse: (resp: ApiResponse | null) => void;
  /** 记下这次发送的脚本产出（断言 / 日志 / 错误 / 写回的环境变量）。 */
  setLastScriptOutcome: (outcome: ScriptOutcome | null) => void;
  /** 把脚本写回的环境变量并进本地副本（落库由后端做）。 */
  applyEnvWrites: (envId: string, writes: EnvWrite[]) => void;
  addResponseHistory: (endpointId: string, response: ApiResponse) => void;
  saveDraftEndpoint: (id: string) => Promise<string | null>;
  updateEndpointDetail: (id: string, patch: ReturnType<typeof draftToPatch>) => void;
  clearEndpointDraft: (id: string) => void;
  /** 在途请求的 ref：pending 期间再次点击发送 = 取消上一次重来。 */
  inFlightRef: RefObject<AbortController | null>;
  setCurlFlash: (v: boolean) => void;
}) {
  const resolveDynamicVariables = useAppStore((s) => s.resolveDynamicVariables);

  const handleSend = useCallback(async () => {
    if (!endpointId) return;
    // 校验是请求的前置门。解析不了的 json body 不该让服务端去猜；照发只会让用户对着
    // 服务端解析错误，而这个错误本可以指出他敲错的那个字符。
    if (!runValidation()) return;
    // 先取消任何在途请求。
    inFlightRef.current?.abort();
    const ctrl = new AbortController();
    inFlightRef.current = ctrl;
    setRequestPending(true);
    try {
      // 动态变量的值必须在**替换之前**到手：替换一旦发生，变量池里就没有同名变量的
      // 概念了，而「动态变量压过同名环境变量」这条规则正是在替换之前生效的。
      // TTL 内命中缓存时后端不发取数请求，故这一步通常是廉价的一次 IPC。
      let request = prepared;
      if (dynamicNames.length > 0) {
        const { values } = await resolveDynamicVariables(dynamicNames);
        request = prepareWithDynamic(values);
      }
      // 路径参数缺值就不发：它按 OpenAPI 惯例是必填的，带着 `{id}` 发出去只会换一个
      // 含义不明的 404，而用户看不出是自己漏填了一个值。
      //
      // 这一步在动态变量解析**之后**：路径参数的值可能就是一个动态变量，它取不到值时
      // 这个路径参数同样算「还没准备好」。
      const missingPathParams = request.missingPathParams;
      onPathParamGate(missingPathParams);
      if (missingPathParams.length > 0) {
        return;
      }
      // 请求的一切都在 `prepared` 里 —— 后端是纯执行器，不会被告知请求来自哪个接口。
      // 这正是未保存的草稿也能发送的原因：没有 `endpointId` 要查，也没有东西要先持久化。
      //
      // ENV 药丸只存在于 Zustand，数据库的 `is_active` 不跟随它，故解析必须基于界面上的
      // `endpointEnv` —— `prepareRequest` 已经折进去了。
      const { response: resp, script } = await sendPreparedRequest({
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
      });
      setLastResponse(resp);
      setLastScriptOutcome(script);
      addResponseHistory(endpointId, resp);
      // 脚本写回的环境变量：**后端已落库**（那是唯一那条持久化路径），这里只把
      // 本地副本同步过来，于是下一个请求立刻能解析到新变量。
      if (script && script.envWrites.length > 0 && env?.id) {
        applyEnvWrites(env.id, script.envWrites);
      }
    } catch (e) {
      // sendRequest 有文档保证不抛 —— 这个 catch 是防御网，兜住 buildFetchInit 之类
      // 的意外。
      console.error('[HttpTester] sendRequest threw:', e);
    } finally {
      // 只有仍是当前 controller 才清空（途中可能被更新的发送顶替）。
      if (inFlightRef.current === ctrl) inFlightRef.current = null;
      setRequestPending(false);
    }
  }, [
    endpointId,
    runValidation,
    inFlightRef,
    onPathParamGate,
    prepared,
    prepareWithDynamic,
    dynamicNames,
    resolveDynamicVariables,
    auth,
    env,
    preRequestScript,
    testScript,
    settings,
    tlsOverride,
    setRequestPending,
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
    if (Object.keys(patch).length > 0) updateEndpointDetail(endpointId, patch);
    clearEndpointDraft(endpointId);
  }, [
    endpointId,
    runValidation,
    isDraft,
    draft,
    saveDraftEndpoint,
    updateEndpointDetail,
    clearEndpointDraft,
  ]);

  const handleCopyCurl = useCallback(async () => {
    // 命令由 `prepared`（发送所用的那个对象）渲染，故不可能描述另一个请求。复制不属
    // 于校验路径，body 解析不了也照样给命令 —— 这是复制而非请求，拒绝只会碍事。
    const curl = preparedToCurl(prepared);
    try {
      await navigator.clipboard.writeText(curl);
      setCurlFlash(true);
      window.setTimeout(() => setCurlFlash(false), 1500);
    } catch (err) {
      // 权限被拒、非安全上下文等 —— 先打到 console（还没有 toast 组件），用户可退而
      // 手动选中文本。
      console.error('[HttpTester] copy as cURL failed:', err);
    }
  }, [prepared, setCurlFlash]);

  return { handleSend, handleSave, handleCopyCurl };
}
