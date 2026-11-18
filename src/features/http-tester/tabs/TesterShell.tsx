import type { ReactNode, RefObject } from 'react';
import { Tabs, type TabItem } from '@/components/ui/tabs';
import type {
  GraphQlRequest,
  HttpMethod,
  Project,
  RequestProtocol,
  ResourceNode,
  TlsSettings,
} from '@/types/domain';
import type { EndpointDraft } from '@/stores/store-types';
import type { FormatResult } from '@/lib/utils';
import { EndpointBreadcrumb } from '../EndpointBreadcrumb';
import { EndpointHeader } from '../EndpointHeader';
import { RequestUrlBar } from '../RequestUrlBar';
import { RequestTabPanels } from '../RequestTabPanels';
import type { GraphQlPanelTools } from '../GraphQlPanel';
import { type VarEntry } from '../var-hint';
import type { BaseUrlSelection } from '../BaseUrlPicker';
import type { KeySuggestion } from '../KeyValueEditor';
import type { Ancestor } from '../useEndpointContext';
import type { EffectiveRequest } from '../model/effectiveRequest';
import type { TesterTabId } from './tabItems';

/**
 * 请求测试器的外壳 —— 面包屑、接口头、URL 栏、页签条与页签内容区。纯展示：所有 state、
 * handler 与 hook 留在 `HttpTester`，这里只按显式 props 把它们摆到界面上。
 */
export function TesterShell({
  project,
  protocol,
  graphql,
  graphqlTools,
  ancestors,
  endpoint,
  endpointHasTab,
  isDraft,
  namingDraft,
  onNamingDraftChange,
  saveEndpointName,
  saveEndpointDescription,
  patchDraft,
  effective,
  globalTls,
  method,
  resolvedBaseUrl,
  moduleBaseUrl,
  selection,
  isCustomUrl,
  liveUrl,
  pathParamUrl,
  urlInputRef,
  sendBtnRef,
  playIconRef,
  copyCurlBtnRef,
  requestPending,
  curlFlash,
  dirty,
  onMethodChange,
  onSelectionChange,
  onUrlChange,
  onSend,
  onSave,
  onCopyCurl,
  draftSaveError,
  dynamicError,
  tabItems,
  tab,
  onTabChange,
  headerSuggestions,
  validationError,
  pathParamError,
  onFormatResult,
  varEntries,
  envName,
  vars,
  onProtocolChange,
  wsSurface,
  responseSurface,
}: {
  project: Project | undefined;
  ancestors: Ancestor[];
  endpoint: ResourceNode;
  /** 请求协议：决定方法位是否只读、Body 页签是否换成 GraphQL。 */
  protocol: RequestProtocol;
  graphql: GraphQlRequest | null;
  graphqlTools: GraphQlPanelTools;
  /** 工作区标签已经显示了名称与集合 hint 时，面包屑不再重复。 */
  endpointHasTab: boolean;
  isDraft: boolean;
  namingDraft: boolean;
  onNamingDraftChange: (next: boolean) => void;
  saveEndpointName: (next: string) => void;
  saveEndpointDescription: (next: string) => void;
  patchDraft: (patch: Partial<EndpointDraft>) => void;
  /** 当前生效的请求配置：已存的 detail 叠上草稿。 */
  effective: EffectiveRequest;
  /** 全局默认那份 HTTPS 出站设置（设置面板里改的那份）。 */
  globalTls: TlsSettings;
  method: HttpMethod;
  resolvedBaseUrl: string;
  /** 环境里「环境 × 模块」那一格的地址。`null` = 没有那一格。 */
  moduleBaseUrl: string | null;
  selection: BaseUrlSelection;
  isCustomUrl: boolean;
  liveUrl: string;
  /** 用来派生路径参数名单的那份路径：改写之后的那一份。 */
  pathParamUrl: string;
  urlInputRef: RefObject<HTMLInputElement | null>;
  sendBtnRef: RefObject<HTMLButtonElement | null>;
  playIconRef: RefObject<HTMLSpanElement | null>;
  copyCurlBtnRef: RefObject<HTMLButtonElement | null>;
  requestPending: boolean;
  curlFlash: boolean;
  dirty: boolean;
  onProtocolChange: (next: RequestProtocol) => void;
  onMethodChange: (m: HttpMethod) => void;
  onSelectionChange: (next: BaseUrlSelection) => void;
  onUrlChange: (next: string) => void;
  onSend: () => void;
  onSave: () => void;
  onCopyCurl: () => void;
  /** 上一次保存没有落盘的原因：草稿写入失败，或已存接口的补丁被后端拒绝。 */
  draftSaveError: string | null;
  /** 动态变量的定义没加载上来 / 取数调用失败时的原因。 */
  dynamicError: string | null;
  tabItems: TabItem[];
  tab: TesterTabId;
  onTabChange: (id: TesterTabId) => void;
  headerSuggestions: KeySuggestion[];
  /** 校验失败的原因。 */
  validationError: string | null;
  /** 路径参数缺值时拦下发送的原因。 */
  pathParamError: string | null;
  onFormatResult: (result: FormatResult) => void;
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName: string | undefined;
  /** 变量池：取 OAuth2 token 时送给后端，让它替换配置里那四个字段的 `{{var}}`。 */
  vars: Record<string, string>;
  /** WebSocket 面板：协议是 websocket 时它替换 URL 栏与页签 / 响应区。 */
  wsSurface?: ReactNode;
  /** SSE 面板：协议是 sse 时它替换「响应页」的内容，URL 栏与其它页签仍是 HTTP 那一套。 */
  responseSurface?: ReactNode;
}) {
  return (
    <section className="flex h-full min-w-0 flex-1 flex-col">
      {/* 面包屑 —— 与设置面板的 `设置 › 连接` 同一写法，在一屏内回答「我在树的哪里」。
          工作区标签承载同样信息时隐藏（见 `endpointHasTab`）。 */}
      {!endpointHasTab && (
        <EndpointBreadcrumb project={project} ancestors={ancestors} endpoint={endpoint} />
      )}

      {/* 接口头：可改名的标题、标签与描述。 */}
      <EndpointHeader
        endpoint={endpoint}
        isDraft={isDraft}
        namingDraft={namingDraft}
        onNamingDraftChange={onNamingDraftChange}
        tags={effective.tags}
        saveEndpointName={saveEndpointName}
        saveEndpointDescription={saveEndpointDescription}
        patchDraft={patchDraft}
      />

      {/* URL 栏：方法、地址来源选择器、地址输入框与发送 / 保存 / 复制 cURL。
          WebSocket 面板自带地址与 Headers 行，故这一栏不出现。 */}
      {!wsSurface && (
        <RequestUrlBar
          protocol={protocol}
          method={method}
          resolvedBaseUrl={resolvedBaseUrl}
          moduleBaseUrl={moduleBaseUrl}
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
          varEntries={varEntries}
          envName={envName}
          onProtocolChange={onProtocolChange}
          onMethodChange={onMethodChange}
          onSelectionChange={onSelectionChange}
          onUrlChange={onUrlChange}
          onSend={onSend}
          onSave={onSave}
          onCopyCurl={onCopyCurl}
        />
      )}

      {/* 保存失败会保留草稿与标签，重试就是再按一次保存 —— 但用户必须知道刚才那下
          什么都没做，否则点击看起来是坏的。草稿写入失败时 `addNode` 把真实错误吞成
          `null` 只记日志；已存接口被后端拒绝时原因由调用方带上来，故这条横幅是 UI 能
          给的全部信号。 */}
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
            onClick={onSave}
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

      {/* WebSocket 把整页交给它自己的面板：帧不属于「请求 / 响应」任何一个页签。 */}
      {wsSurface}

      {/* 页签条 —— 与下方面板相连：本行独占那条分隔线（条内不再有），激活标签以面板底色
          填充自己，盖住分隔线并与之圆角相接。`bg-card/30` 与上方 URL 栏一致，
          `pt-1.5` 不配 `pb`，使条的下沿与分隔线齐平。 */}
      {!wsSurface && (
        <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/30 px-4 pt-1.5">
          <Tabs
            variant="connected"
            joinStripClass="bg-card/30"
            items={tabItems}
            value={tab}
            onChange={(id) => onTabChange(id as TesterTabId)}
          />
          <div className="ml-auto text-nano text-muted-foreground">
            <span className="kbd">Ctrl</span> + <span className="kbd">Enter</span> 发送
          </div>
        </div>
      )}

      {/* 页签内容区。SSE 只替换「响应页」的内容 —— 事件流不是响应定义编辑器该显示的东西，
          而 Params / Headers / Body / Auth / TLS / 路径参数这些页仍要能编辑，因为 SSE 复用
          HTTP 的请求流程。切回 HTTP 时这个位置回到 `RequestTabPanels` 的响应页，定义数据没动。 */}
      {!wsSurface &&
        (tab === 'response' && responseSurface ? (
          <div className="min-h-0 flex-1">{responseSurface}</div>
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <RequestTabPanels
              tab={tab}
              graphql={graphql}
              graphqlTools={graphqlTools}
              params={effective.params}
              pathParams={effective.pathParams}
              url={pathParamUrl}
              headers={effective.headers}
              body={effective.body}
              auth={effective.auth}
              docs={effective.docs}
              responses={effective.responses}
              preRequestScript={effective.preRequestScript}
              testScript={effective.testScript}
              // 端点自己那份 HTTPS 出站设置（`null` = 继承全局）与全局默认那份。
              tls={effective.tls}
              globalTls={globalTls}
              headerSuggestions={headerSuggestions}
              validationError={validationError}
              pathParamError={pathParamError}
              varEntries={varEntries}
              envName={envName}
              vars={vars}
              patchDraft={patchDraft}
              onFormatResult={onFormatResult}
            />
          </div>
        ))}
    </section>
  );
}
