import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { Play, Save, Copy, Check, ChevronRight } from 'lucide-react';
import { useAppStore, useActiveEndpointResource, useActiveEnvironment } from '@/stores/app-store';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { InlineEdit } from '@/components/ui/InlineEdit';
import { MethodPicker } from '@/components/ui/method-picker';
import { BaseUrlPicker, type BaseUrlSelection } from '@/features/http-tester/BaseUrlPicker';
import { TagInput } from '@/components/ui/tag-input';
import { Tooltip } from '@/components/ui/tooltip';
import { type AuthConfig, type RequestBody } from '@/types/domain';
import { methodColorVar, cn } from '@/lib/utils';
import { resolveEndpointBaseUrl } from '@/lib/url';
import { moduleIdOfEndpoint } from '@/lib/tree-index';
import { validateRequest } from '@/lib/request-validation';
import { sendPreparedRequest } from '@/lib/http';
import { prepareRequest, preparedToCurl } from '@/lib/http-build';
import { draftToPatch } from '@/stores/slices/crud-helpers';
import { UNTITLED_ENDPOINT_NAME, type EndpointDraft } from '@/stores/store-types';
import { KeyValueEditor } from './KeyValueEditor';
import { buildHeaderSuggestions } from '@/lib/common-headers';
import { BodyEditor } from './BodyEditor-lazy';
import { AuthEditor } from './AuthEditor';
import { DocsEditor } from './DocsEditor-lazy';
import { EnvVarPicker, extractPartialVar, shouldShowVarPicker } from './EnvVarPicker';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

const TABS: { id: TabId; label: string; disabled?: boolean }[] = [
  { id: 'params', label: 'Params' },
  { id: 'headers', label: 'Headers' },
  { id: 'body', label: 'Body' },
  { id: 'auth', label: 'Auth' },
  { id: 'docs', label: 'Docs' },
  { id: 'tests', label: 'Tests', disabled: true },
];

/**
 * 缺失 `endpoint_details` 行时的占位，让 effective 请求 memo 有类型可读。
 * 按引用共享：所有编辑器都是先展开再写，不会就地修改。
 */
const EMPTY_BODY: RequestBody = { mode: 'none', content: '' };
const EMPTY_AUTH: AuthConfig = { kind: 'none' };

type TabId = 'params' | 'headers' | 'body' | 'auth' | 'docs' | 'tests';

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
  // 工作区当前环境（顶栏的 ENV dev 药丸）。接口未自选环境时作兜底，让新建接口的
  // baseUrl + 变量解析直接可用，不必先走一遍选择器。
  const workspaceEnv = useActiveEnvironment();
  // 原始 store slice —— 接口的 project / ancestors 在本地用 useMemo 派生。
  // 逐个 `useAppStore` 取用，保持 Zustand v5「selector 不新建对象」规则。
  const allNodes = useAppStore((s) => s.nodes);
  const allProjects = useAppStore((s) => s.projects);
  const allEnvironments = useAppStore((s) => s.environments);
  // 供 Header 键补全：把用户自己用过的 header 名与内置常见列表一起喂给 datalist
  // （见 buildHeaderSuggestions）。
  const allEndpointDetails = useAppStore((s) => s.endpointDetails);
  const updateNode = useAppStore((s) => s.updateNode);
  const updateEndpointDetail = useAppStore((s) => s.updateEndpointDetail);
  const setRequestPending = useAppStore((s) => s.setRequestPending);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const addResponseHistory = useAppStore((s) => s.addResponseHistory);
  const setEndpointDraft = useAppStore((s) => s.setEndpointDraft);
  const clearEndpointDraft = useAppStore((s) => s.clearEndpointDraft);
  const saveDraftEndpoint = useAppStore((s) => s.saveDraftEndpoint);
  const updateDraftEndpoint = useAppStore((s) => s.updateDraftEndpoint);
  const draftSaveError = useAppStore((s) => s.draftSaveError);
  const requestPending = useAppStore((s) => s.requestPending);
  const requestSettings = useAppStore((s) => s.requestSettings);

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

  // EnvVarPicker 状态 —— 用户在 URL 输入框敲 `{{` 时打开。选择器是纯展示：选中项与
  // 过滤条目由 HttpTester 自己持有（据当前环境 + 最后一个 `{{` 之后输入的片段派生）。
  const [varPickerOpen, setVarPickerOpen] = useState(false);
  const [varSelected, setVarSelected] = useState(0);
  const urlInputRef = useRef<HTMLInputElement>(null);

  // 实时的 URL —— 输入框此刻的内容。通常只是 path（baseUrl 之后那段），自定义模式下
  // 是完整地址。取自 store 侧的草稿（切标签不丢），回落到已持久化的 detail URL。
  const liveUrl = draft?.url ?? detail?.url ?? '';

  // 接口所属项目，直接取反规范化的 `rootId`：单棵树上每行都带着这个答案。
  const project = useMemo(
    () => (endpoint ? allProjects.find((p) => p.id === endpoint.rootId) : undefined),
    [endpoint, allProjects],
  );
  // 页面顶部 `项目 › 模块 › 集合` 面包屑的祖先链，与设置面板（`设置 › 连接`）同一套
  // 锚定写法。沿 `parentId` 向上收集、根在前，故集合嵌套超过一层也无需额外分支。
  const ancestors = useMemo(() => {
    if (!endpoint) return [];
    const chain: { id: string; name: string; type: string }[] = [];
    const seen = new Set<string>([endpoint.id]);
    let cur = endpoint.parentId ? allNodes.find((n) => n.id === endpoint!.parentId) : undefined;
    while (cur && !seen.has(cur.id)) {
      seen.add(cur.id);
      chain.unshift({ id: cur.id, name: cur.name, type: cur.kind });
      cur = cur.parentId ? allNodes.find((n) => n.id === cur!.parentId) : undefined;
    }
    return chain;
  }, [endpoint, allNodes]);
  // 该接口 baseUrl 所属的环境：优先 detail 的 `environmentId`，否则回落到工作区当前
  // 环境（顶栏 ENV dev 药丸），免得新建接口因为还没指定环境就没有 baseUrl。
  const endpointEnv = useMemo(() => {
    if (detail?.environmentId) {
      return allEnvironments.find((e) => e.id === detail.environmentId);
    }
    return workspaceEnv;
  }, [detail, allEnvironments, workspaceEnv]);

  // 该接口的有效 baseUrl —— 同时驱动选择器标签与发送时的拼接。
  //
  // 服务列表属于项目，故选择器给出全部；按模块区分的是读 URL 的**单元格**。
  // `moduleId` 仍必需：它挑出本接口读的那一行 `env.moduleBaseUrls[defId]`，但不再
  // 收窄可选服务。
  //
  // `endpoint` 作为向上遍历的起始行传入而非按 id 查：草稿接口没有 `nodes` 行，只按
  // id 查会得到「无模块」而永远读不到该单元格（见 `moduleIdOfEndpoint` 的 `self`）。
  const moduleId = useMemo(
    () => moduleIdOfEndpoint(allNodes, endpoint?.id, endpoint),
    [allNodes, endpoint],
  );
  const resolved = useMemo(
    () =>
      resolveEndpointBaseUrl(
        detail ?? { environmentId: null, baseUrlDefinitionId: null, baseUrlOverride: null },
        endpointEnv,
        project,
        moduleId,
      ),
    [detail, endpointEnv, project, moduleId],
  );

  // 驱动选择器：反映接口上现存的选择。用户改选时 `commitBaseUrl` 写到这个接口存放
  // baseUrl 选择的位置 —— 已保存的写该行，未保存的写草稿文档。已保存的立即落库
  // （此选择没有 Save 按钮），草稿则带到 Save 时并入该行。
  //
  // `baseUrlOverride` 是三态，空串正是自定义的意义：`null` = 非自定义模式，
  // `''` = 自定义模式、URL 框承载完整地址。按真值判断会把两者混同：点自定义写入
  // `''` 后下次渲染读成 falsy，选择器弹回 项目默认，点击看起来毫无效果。
  const selection: BaseUrlSelection = useMemo(() => {
    if (!detail) return { kind: 'default' };
    if (detail.baseUrlOverride !== null && detail.baseUrlOverride !== undefined) {
      return { kind: 'override' };
    }
    if (detail.baseUrlDefinitionId) {
      return { kind: 'definition', definitionId: detail.baseUrlDefinitionId };
    }
    return { kind: 'default' };
  }, [detail]);
  // 自定义 让 URL 框承载完整地址，故不得再给它加任何前缀。`resolved.baseUrl` 也
  // 收敛为空串（拼接时 path 原样不变，见 joinBaseUrl），但这里读 selection，好让仍
  // 带 host 的历史 override 不掩盖这个意图。
  const isCustomUrl = selection.kind === 'override';

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
      headers: draft?.headers ?? detail?.headers ?? [],
      body: draft?.body ?? detail?.body ?? EMPTY_BODY,
      auth: draft?.auth ?? detail?.auth ?? EMPTY_AUTH,
      docs: draft?.docs ?? detail?.docs ?? '',
      tags: draft?.tags ?? detail?.tags ?? [],
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

  // env 变量选择器的条目：接口自己的环境（即本接口解析 `{{x}}` 所依据的那份变量）
  // + 用户在最后一个 `{{` 之后输入的片段。
  const allEnvEntries = useMemo(
    () =>
      endpointEnv
        ? Object.entries(endpointEnv.variables)
            .map(([k, v]) => ({ key: k, value: v }))
            .sort((a, b) => a.key.localeCompare(b.key))
        : [],
    [endpointEnv],
  );
  const partialVar = extractPartialVar(liveUrl);
  const filteredEntries = useMemo(() => {
    const q = (partialVar ?? '').toLowerCase();
    if (!q) return allEnvEntries;
    return allEnvEntries.filter((e) => (e.key ?? '').toLowerCase().includes(q));
  }, [allEnvEntries, partialVar]);
  // Header 补全从每个已加载 detail 收拢用户自己的 header 名。`Object.values` 会新建
  // 对象，故按该 map 的引用 memo，而不是每次渲染重建。
  const headerSuggestions = useMemo(
    () => buildHeaderSuggestions(Object.values(allEndpointDetails)),
    [allEndpointDetails],
  );

  const handleUrlChange = (next: string) => {
    if (!endpoint) return;
    setEndpointDraft(endpoint.id, { url: next });
    setVarPickerOpen(shouldShowVarPicker(next));
    if (!shouldShowVarPicker(next)) setVarSelected(0);
  };

  const insertVar = (key: string) => {
    // 把最后一个 `{{` 及其后的变量名片段（截至 `}}`、空白或又一个 `{`）替换为选中的
    // 键，并补上闭合大括号。
    if (!endpoint) return;
    const base = draft?.url ?? detail?.url ?? '';
    const next = base.replace(/\{\{[^}\s]*$/, `{{${key}}}`);
    setEndpointDraft(endpoint.id, { url: next });
    setVarPickerOpen(false);
    setVarSelected(0);
    // 恢复焦点并把光标移到插入 token 的末尾。
    window.setTimeout(() => {
      const el = urlInputRef.current;
      if (!el) return;
      el.focus();
      const caret = next.length;
      try {
        el.setSelectionRange(caret, caret);
      } catch {
        /* 数字输入框之类 —— type=text 用不到 */
      }
    }, 0);
  };

  // 自定义 把 URL 栏变成完整地址，故把光标放过去并全选旧内容 —— 直接输入即替换旧
  // path 而非追加。延后一拍执行，使其发生在切换模式的重渲染（以及 popover 关闭）之后。
  const focusUrlForFullAddress = () => {
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
  };

  const handleUrlKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (!varPickerOpen) return;
    // 放过带修饰键的组合，全局快捷键（Ctrl+Enter、Ctrl+S）才仍能触发。
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (filteredEntries.length > 0) {
        setVarSelected((s) => (s + 1) % filteredEntries.length);
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (filteredEntries.length > 0) {
        setVarSelected((s) => (s - 1 + filteredEntries.length) % filteredEntries.length);
      }
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      const entry = filteredEntries[varSelected] ?? filteredEntries[0];
      if (entry) insertVar(entry.key);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setVarPickerOpen(false);
    }
  };

  // 草稿与当前激活的接口同步
  const method = draft?.method ?? detail?.method ?? 'GET';

  // 请求的最终线上形态，只组装**一次**。
  //
  // 发送与 cURL 预览读同一个对象 —— 这正是此安排的全部意义：预览现在是发送所发字节的
  // 序列化，不可能描述出另一个请求。
  const prepared = useMemo(
    () =>
      prepareRequest({
        method,
        urlPath: liveUrl,
        baseUrl: resolved.baseUrl,
        params: effective.params,
        headers: effective.headers,
        body: effective.body,
        auth: effective.auth,
        env: endpointEnv,
      }),
    [method, liveUrl, resolved.baseUrl, effective, endpointEnv],
  );

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

  // C2：Send 按钮形变。`playIconRef` 是包住 Play 图标的 `inline-flex` span，就地
  // 旋转而不动按钮文字宽度；`sendBtnRef` 是整个 Button，用于请求返回后的微弹。
  const sendBtnRef = useRef<HTMLButtonElement>(null);
  const playIconRef = useRef<HTMLSpanElement>(null);
  // 上一次运行时是否真的有请求在途。`useGSAP` 挂载时也会跑一次，而挂载时
  // `requestPending` 为 false —— 不加这个门，每次打开接口标签都会弹一下，毫无意义。
  // 微弹要表达「响应到了」，故须以确实有请求可到达为前提。
  const wasPendingRef = useRef(false);

  useGSAP(() => {
    if (!playIconRef.current || !sendBtnRef.current) return;

    if (requestPending) {
      wasPendingRef.current = true;
      // 重新起转前先杀掉残留旋转（例如上次取消留下的）。gsap.fromTo 配 rotation +=360
      // + repeat:-1 得到连续匀速旋转。
      //
      // 受 reduced motion 限制，因为这个动画不会停：无尽循环是最强的前庭触发器，也是
      // 此处唯一在用户纯等待时运行的动画。不转不损失信息 —— 请求在途时按钮已
      // `disabled` 且标签是 发送中…，状态从不依赖旋转。
      gsap.killTweensOf(playIconRef.current);
      return motionSafe(() => {
        gsap.fromTo(
          playIconRef.current,
          { rotation: 0 },
          { rotation: '+=360', duration: 1, repeat: -1, ease: 'none' },
        );
      });
    }

    // 结束路径：杀掉旋转并把图标复位竖直，使下次渲染从 rotation 0 起。
    const requestArrived = wasPendingRef.current;
    wasPendingRef.current = false;
    gsap.killTweensOf(playIconRef.current);
    gsap.set(playIconRef.current, { rotation: 0 });
    if (!requestArrived) return;

    // 微弹按钮，确认响应已到。
    return motionSafe(() => {
      gsap.killTweensOf(sendBtnRef.current);
      gsap.fromTo(
        sendBtnRef.current,
        { scale: 1 },
        {
          keyframes: [
            { scale: 1.05, duration: 0.125, ease: 'power2.out' },
            { scale: 1, duration: 0.125, ease: 'power2.in' },
          ],
        },
      );
    });
  }, [requestPending]);

  // C3：复制 cURL 的反馈弹跳。仅在 curlFlash 由 false → true 时触发；false 那一跳
  // 刻意为空操作，免得已换成「已复制」的标签退出时再多闪一下。
  const copyCurlBtnRef = useRef<HTMLButtonElement>(null);
  useGSAP(() => {
    if (!curlFlash || !copyCurlBtnRef.current) return;
    return motionSafe(() => {
      gsap.killTweensOf(copyCurlBtnRef.current);
      gsap.fromTo(
        copyCurlBtnRef.current,
        { scale: 1 },
        {
          keyframes: [
            { scale: 1.06, duration: 0.1, ease: 'power2.out' },
            { scale: 1, duration: 0.1, ease: 'power2.in' },
          ],
        },
      );
    });
  }, [curlFlash]);

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
  const hasBody = effective.body.mode !== 'none' && effective.body.content;
  const hasAuth = effective.auth.kind !== 'none';

  const tabItems = TABS.map((t) => {
    let badge: React.ReactNode = null;
    if (t.id === 'params' && enabledParams)
      badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledParams}</span>;
    if (t.id === 'headers' && enabledHeaders)
      badge = <span className="rounded bg-muted px-1 text-[10px]">{enabledHeaders}</span>;
    if (t.id === 'body' && hasBody)
      badge = <span className="rounded bg-primary/20 px-1 text-[10px] text-primary">●</span>;
    if (t.id === 'auth' && hasAuth)
      badge = (
        <span className="rounded bg-warning/20 px-1 text-[10px] text-warning-foreground">●</span>
      );
    return { id: t.id, label: t.label, badge, disabled: t.disabled };
  });

  const handleSend = async () => {
    if (!endpoint) return;
    // 校验是请求的前置门。解析不了的 json body 不该让服务端去猜；照发只会让用户对着
    // 服务端解析错误，而这个错误本可以指出他敲错的那个字符。
    if (!runValidation()) return;
    // 先取消任何在途请求。
    inFlightRef.current?.abort();
    const ctrl = new AbortController();
    inFlightRef.current = ctrl;
    setRequestPending(true);
    try {
      // 请求的一切都在 `prepared` 里 —— 后端是纯执行器，不会被告知请求来自哪个接口。
      // 这正是未保存的草稿也能发送的原因：没有 `endpointId` 要查，也没有东西要先持久化。
      //
      // ENV 药丸只存在于 Zustand，数据库的 `is_active` 不跟随它，故解析必须基于界面上的
      // `endpointEnv` —— `prepareRequest` 已经折进去了。
      const resp = await sendPreparedRequest({
        request: prepared,
        // 与 `prepared` 用的是同一份 auth：OAuth2 要靠它取 token，`prepared` 本身
        // 不含（token 只有发出去的那一刻才拿得到）。
        auth: effective.auth,
        settings: requestSettings,
        signal: ctrl.signal,
      });
      setLastResponse(resp);
      addResponseHistory(endpoint.id, resp);
    } catch (e) {
      // sendRequest 有文档保证不抛 —— 这个 catch 是防御网，兜住 buildFetchInit 之类
      // 的意外。
      console.error('[HttpTester] sendRequest threw:', e);
    } finally {
      // 只有仍是当前 controller 才清空（途中可能被更新的发送顶替）。
      if (inFlightRef.current === ctrl) inFlightRef.current = null;
      setRequestPending(false);
    }
  };

  const handleSave = () => {
    if (!endpoint) return;
    // 与发送同门：持久化一个解析不了的 json body 等于故意存下坏请求。校验失败时草稿
    // 原样留着 —— 不写不清。
    if (!runValidation()) return;

    // 未保存的接口没有可 patch 的行，故整次写入一步完成：`saveDraftEndpoint` 把文档
    // 字段与待存的请求配置编辑并成一个 `detail` 写入，并把草稿标签换成真实接口标签。
    // 写入失败时保留草稿（与本标签），重试就是再按一次保存，错误经 `draftSaveError` 呈现。
    if (isDraft) {
      void saveDraftEndpoint(endpoint.id);
      return;
    }

    const patch = draft ? draftToPatch(draft) : {};
    if (Object.keys(patch).length > 0) updateEndpointDetail(endpoint.id, patch);
    clearEndpointDraft(endpoint.id);
  };

  const handleCopyCurl = async () => {
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
  };

  // 草稿天生未保存，故即使还没有任何编辑器触发过，按钮也必须可用。只按
  // `draft !== undefined` 判定，恰好会卡住这个功能要服务的接口：用户新建、输了 URL、
  // 发了请求，现在想留下它 —— 却没有任何 params / headers / body 编辑把它变脏。
  const dirty = isDraft || draft !== undefined;

  return (
    <section className="flex h-full min-w-0 flex-1 flex-col">
      {/* 面包屑 —— 与设置面板的 `设置 › 连接` 同一写法，在一屏内回答「我在树的哪里」。
          工作区标签承载同样信息时隐藏（见 `endpointHasTab`）。 */}
      {!endpointHasTab && (
        <nav
          aria-label="接口所属路径"
          className="flex shrink-0 items-center gap-1 px-4 pt-5 text-[11px] text-muted-foreground"
        >
          {project && ancestors.length > 0 ? (
            <>
              <button
                type="button"
                onClick={() => {
                  const id = useAppStore.getState().activeProjectId;
                  // 用 `openProject` 而非 `setActiveProject`：面包屑首节是到项目的链接，
                  // 应带出项目面板（并重新界定树的范围），而不只是重解析环境。
                  if (id) useAppStore.getState().openProject(id);
                }}
                className="truncate hover:text-foreground"
                title={project.name}
              >
                {project.name}
              </button>
              {ancestors.map((a) => (
                <span key={a.id} className="contents">
                  <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
                  <span className="truncate" title={a.name}>
                    {a.name}
                  </span>
                </span>
              ))}
              <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
              <span className="truncate text-foreground" title={endpoint.name}>
                {endpoint.name}
              </span>
            </>
          ) : (
            <span>接口</span>
          )}
        </nav>
      )}

      {/* Endpoint header */}
      <div className="flex shrink-0 items-start gap-2 border-b border-border px-4 pb-3 pt-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <InlineEdit
              key={`name-${endpoint.id}`}
              value={endpoint.name}
              onSave={saveEndpointName}
              testId="endpoint-name-input"
              // 只对草稿受控，由 `namingDraft` 在新建接口时打开编辑器。传 `undefined` 则
              // 已保存的接口维持编辑器自身的双击行为，不受影响。
              editing={isDraft ? namingDraft : undefined}
              onEditingChange={(next) => {
                if (isDraft) setNamingDraft(next);
              }}
              className="max-w-md text-sm font-semibold"
              display={() => (
                <h1
                  className="cursor-text truncate rounded-sm px-1 py-0.5 text-sm font-semibold hover:bg-accent/60"
                  title="双击重命名"
                >
                  {endpoint.name}
                </h1>
              )}
            />
            <TagInput
              value={effective.tags}
              onChange={(tags) => patchDraft({ tags })}
              placeholder="+ 标签"
              className="min-w-[140px] max-w-[320px] border-transparent bg-transparent px-1 py-0.5 focus-within:ring-0 focus-within:ring-offset-0"
            />
          </div>
          <InlineEdit
            key={`desc-${endpoint.id}`}
            value={endpoint.description ?? ''}
            onSave={saveEndpointDescription}
            // 描述允许清空 —— 否则空草稿会被当成取消，字段保留旧值而不是回到「无描述」。
            commitEmpty
            testId="endpoint-description-input"
            className="mt-0.5 h-6 max-w-2xl text-xs"
            placeholder="一句话说明这个接口的用途…"
            display={() => (
              <p
                className={cn(
                  'mt-0.5 cursor-text truncate rounded-sm px-1 text-xs text-muted-foreground',
                  'hover:bg-accent/60',
                  !endpoint.description && 'italic text-muted-foreground/50',
                )}
                title="双击编辑描述"
              >
                {endpoint.description || '双击添加描述…'}
              </p>
            )}
          />
        </div>
      </div>

      {/* URL bar */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/30 px-4 py-2.5">
        <MethodPicker
          value={method}
          onChange={(m) => {
            if (endpoint) setEndpointDraft(endpoint.id, { method: m });
          }}
          className="border-0"
        />
        <BaseUrlPicker
          // 项目的完整服务列表 —— 服务属于项目级，故每一个都是这里真实可选项。按服务不同的
          // 是读 URL 的**单元格**，不是可选性。
          definitions={project?.baseUrlDefinitions ?? []}
          defaultDefinitionId={project?.defaultBaseUrlDefinitionId ?? null}
          resolvedBaseUrl={resolved.baseUrl}
          selection={selection}
          onSelectionChange={(next) => {
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
          }}
        />
        <div className="relative flex min-w-0 flex-1 items-center">
          <Input
            ref={urlInputRef}
            value={liveUrl}
            onChange={(e) => handleUrlChange(e.target.value)}
            onKeyDown={handleUrlKeyDown}
            // 自定义 让本框承载完整地址，故须明示：placeholder 换成完整 URL、ring 标出
            // 模式 —— 选择器位于用户实际聚焦的框左侧，光靠它的标签不够。
            className={cn(
              'h-9 w-full font-mono text-sm',
              isCustomUrl && 'border-primary/40 ring-1 ring-inset ring-primary/40',
            )}
            placeholder={
              isCustomUrl || !resolved.baseUrl ? 'https://api.example.com/path' : '/path'
            }
            title={
              isCustomUrl
                ? '自定义：直接使用这里填写的完整地址，不再拼接 baseUrl 前缀'
                : resolved.baseUrl || undefined
            }
            data-testid="url-input"
          />
        </div>
        <Button
          ref={sendBtnRef}
          onClick={handleSend}
          disabled={requestPending}
          className="h-9 gap-1.5 px-4"
          style={{ backgroundColor: methodColorVar(method) }}
          data-testid="send-btn"
        >
          <span ref={playIconRef} className="inline-flex">
            <Play className="h-3.5 w-3.5 fill-current" />
          </span>
          {requestPending ? '发送中…' : 'Send'}
        </Button>
        <Tooltip content={curlFlash ? '已复制' : '复制为 cURL'} side="bottom">
          <Button
            ref={copyCurlBtnRef}
            variant="outline"
            size="sm"
            onClick={handleCopyCurl}
            className={cn('h-9 gap-1.5', curlFlash && 'text-success')}
            data-testid="copy-curl-btn"
          >
            {curlFlash ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
            {curlFlash ? '已复制' : 'cURL'}
          </Button>
        </Tooltip>
        <Button
          variant={dirty ? 'default' : 'outline'}
          size="sm"
          onClick={handleSave}
          disabled={!dirty}
          className="h-9 gap-1.5"
          data-testid="save-btn"
        >
          <Save className="h-3.5 w-3.5" />
          {isDraft ? '保存接口' : 'Save'}
        </Button>
      </div>

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
        {tab === 'params' && (
          <KeyValueEditor
            items={effective.params}
            onChange={(items) => patchDraft({ params: items })}
            keyPlaceholder="query 参数名"
            valuePlaceholder="值（支持 {{var}}）"
          />
        )}
        {tab === 'headers' && (
          <KeyValueEditor
            items={effective.headers}
            onChange={(items) => patchDraft({ headers: items })}
            keyPlaceholder="Header 名"
            valuePlaceholder="值"
            // 最近使用（项目范围收拢）与内置列表合并 —— 见
            // common-headers.ts::buildHeaderSuggestions。
            keySuggestions={headerSuggestions}
          />
        )}
        {tab === 'body' && (
          <BodyEditor
            value={effective.body}
            onChange={(body) => patchDraft({ body })}
            // 错误由父组件钉住：校验是 发送 / 保存 / 格式化 触发的动作，而非从输入派生
            // 出的值。
            validationError={validationError}
            // 格式化 已经解析过内容，其结果直接定徽标。此处再跑 `runValidation` 检查的
            // 是格式化前的草稿（本次渲染的闭包），即用户刚解析失败的那份内容。
            onFormatResult={(result) => setValidationError(result.ok ? null : result.error)}
          />
        )}
        {tab === 'auth' && (
          <AuthEditor value={effective.auth} onChange={(auth) => patchDraft({ auth })} />
        )}
        {tab === 'docs' && (
          <DocsEditor value={effective.docs} onChange={(docs) => patchDraft({ docs })} />
        )}
        {tab === 'tests' && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            Tests 将在 v0.3 推出
          </div>
        )}
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
