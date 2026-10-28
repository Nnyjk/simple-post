import type { KeyboardEvent, RefObject } from 'react';
import { Tabs, type TabItem } from '@/components/ui/tabs';
import type { HttpMethod, Project, ResourceNode, TlsSettings } from '@/types/domain';
import type { EndpointDraft } from '@/stores/store-types';
import type { FormatResult } from '@/lib/utils';
import { EndpointBreadcrumb } from '../EndpointBreadcrumb';
import { EndpointHeader } from '../EndpointHeader';
import { RequestUrlBar } from '../RequestUrlBar';
import { RequestTabPanels } from '../RequestTabPanels';
import { EnvVarPicker, type VarEntry } from '../EnvVarPicker';
import type { BaseUrlSelection } from '../BaseUrlPicker';
import type { KeySuggestion } from '../KeyValueEditor';
import type { Ancestor } from '../useEndpointContext';
import type { EffectiveRequest } from '../model/effectiveRequest';
import type { TesterTabId } from './tabItems';

/**
 * 请求测试器的外壳 —— 面包屑、接口头、URL 栏、页签条、页签内容区与变量选择器。
 *
 * 纯展示：所有 state、handler 与 hook 留在 `HttpTester`，这里只按显式 props 把它们
 * 摆到界面上。
 */
export function TesterShell({
  project,
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
  selection,
  isCustomUrl,
  liveUrl,
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
  onUrlKeyDown,
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
  varPickerOpen,
  onCloseVarPicker,
  varEntries,
  varSelected,
  onVarSelectedChange,
  onInsertVar,
  envName,
}: {
  project: Project | undefined;
  ancestors: Ancestor[];
  endpoint: ResourceNode;
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
  selection: BaseUrlSelection;
  isCustomUrl: boolean;
  liveUrl: string;
  urlInputRef: RefObject<HTMLInputElement | null>;
  sendBtnRef: RefObject<HTMLButtonElement | null>;
  playIconRef: RefObject<HTMLSpanElement | null>;
  copyCurlBtnRef: RefObject<HTMLButtonElement | null>;
  requestPending: boolean;
  curlFlash: boolean;
  dirty: boolean;
  onMethodChange: (m: HttpMethod) => void;
  onSelectionChange: (next: BaseUrlSelection) => void;
  onUrlChange: (next: string) => void;
  onUrlKeyDown: (e: KeyboardEvent<HTMLInputElement>) => void;
  onSend: () => void;
  onSave: () => void;
  onCopyCurl: () => void;
  /** 草稿保存失败的原因。 */
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
  varPickerOpen: boolean;
  onCloseVarPicker: () => void;
  varEntries: VarEntry[];
  varSelected: number;
  onVarSelectedChange: (next: number) => void;
  onInsertVar: (key: string) => void;
  envName: string | undefined;
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

      {/* URL 栏：方法、baseUrl 选择器、地址输入框与发送 / 保存 / 复制 cURL。 */}
      <RequestUrlBar
        method={method}
        definitions={project?.baseUrlDefinitions ?? []}
        defaultDefinitionId={project?.defaultBaseUrlDefinitionId ?? null}
        resolvedBaseUrl={resolvedBaseUrl}
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
        onMethodChange={onMethodChange}
        onSelectionChange={onSelectionChange}
        onUrlChange={onUrlChange}
        onUrlKeyDown={onUrlKeyDown}
        onSend={onSend}
        onSave={onSave}
        onCopyCurl={onCopyCurl}
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

      {/* 页签条 —— 与下方面板相连：本行独占那条分隔线（条内不再有），激活标签以面板底色
          填充自己，盖住分隔线并与之圆角相接。`bg-card/30` 与上方 URL 栏一致，也是
          激活标签得以凸显的背景。`pt-1.5` 不配 `pb`，使条的下沿与分隔线齐平。 */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/30 px-4 pt-1.5">
        <Tabs
          variant="connected"
          joinStripClass="bg-card/30"
          items={tabItems}
          value={tab}
          onChange={(id) => onTabChange(id as TesterTabId)}
        />
        <div className="ml-auto text-[10px] text-muted-foreground">
          <span className="kbd">Ctrl</span> + <span className="kbd">Enter</span> 发送
        </div>
      </div>

      {/* 页签内容区 */}
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
          // 端点自己那份 HTTPS 出站设置（`null` = 继承全局）与全局默认那份。
          tls={effective.tls}
          globalTls={globalTls}
          headerSuggestions={headerSuggestions}
          validationError={validationError}
          pathParamError={pathParamError}
          patchDraft={patchDraft}
          onFormatResult={onFormatResult}
        />
      </div>

      {/* Env 变量选择器 —— 锚在 URL 输入框上，在选择器内部用 getBoundingClientRect
          定位，故滚动 / resize 时跟随输入框。 */}
      <EnvVarPicker
        open={varPickerOpen}
        onClose={onCloseVarPicker}
        entries={varEntries}
        selected={varSelected}
        onSelectedChange={onVarSelectedChange}
        onSelect={onInsertVar}
        anchorRef={urlInputRef}
        envName={envName}
      />
    </section>
  );
}
