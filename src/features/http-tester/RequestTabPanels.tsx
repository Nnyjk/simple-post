import { KeyValueEditor, type KeySuggestion } from './KeyValueEditor';
import { PathParamsEditor } from './PathParamsEditor';
import { BodyEditor } from './BodyEditor-lazy';
import { AuthEditor } from './AuthEditor';
import { DocsEditor } from './DocsEditor-lazy';
import { ScriptPanel } from './ScriptPanel';
import { TlsPanel } from './TlsPanel';
import type { EndpointDraft } from '@/stores/store-types';
import type { FormatResult } from '@/lib/utils';
import type { AuthConfig, KeyValue, RequestBody, TlsSettings } from '@/types/domain';
import type { VarEntry } from './var-hint';

/** 请求配置编辑器的页签 id。`path` 是路径参数，`script` 是请求脚本（前置 / 测试两段）。 */
export type TesterTabId =
  'params' | 'path' | 'headers' | 'body' | 'auth' | 'docs' | 'script' | 'tls';

/**
 * 页签内容区 —— 只渲染当前那一页编辑器。
 *
 * 每个编辑器都写草稿而非已存的 detail 行；错误徽标由父组件钉住（校验是
 * 发送 / 保存 / 格式化 触发的动作，而非从输入派生出的值）。
 */
export function RequestTabPanels({
  tab,
  params,
  pathParams,
  url,
  headers,
  body,
  auth,
  docs,
  preRequestScript,
  testScript,
  tls,
  globalTls,
  headerSuggestions,
  validationError,
  pathParamError,
  varEntries,
  envName,
  patchDraft,
  onFormatResult,
}: {
  tab: TesterTabId;
  params: KeyValue[];
  pathParams: KeyValue[];
  /** 当前生效的 URL 文本。路径参数的名单从它派生。 */
  url: string;
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  docs: string;
  /** 前置脚本的当前文本。 */
  preRequestScript: string;
  /** 测试脚本的当前文本。 */
  testScript: string;
  /** 端点自己那份 HTTPS 出站设置。`null` = 继承全局默认。 */
  tls: TlsSettings | null;
  /** 全局默认那份 HTTPS 出站设置。 */
  globalTls: TlsSettings;
  headerSuggestions: KeySuggestion[];
  validationError: string | null;
  /** 路径参数缺值时拦下发送的原因。 */
  pathParamError: string | null;
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName: string | undefined;
  patchDraft: (patch: Partial<EndpointDraft>) => void;
  onFormatResult: (result: FormatResult) => void;
}) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      {tab === 'params' && (
        <KeyValueEditor
          items={params}
          onChange={(items) => patchDraft({ params: items })}
          keyPlaceholder="query 参数名"
          valuePlaceholder="值（支持 {{var}}）"
          varEntries={varEntries}
          envName={envName}
        />
      )}
      {tab === 'path' && (
        <PathParamsEditor
          url={url}
          items={pathParams}
          error={pathParamError}
          onChange={(items) => patchDraft({ pathParams: items })}
        />
      )}
      {tab === 'headers' && (
        <KeyValueEditor
          items={headers}
          onChange={(items) => patchDraft({ headers: items })}
          keyPlaceholder="Header 名"
          valuePlaceholder="值"
          // 最近使用（项目范围收拢）与内置列表合并 —— 见
          // common-headers.ts::buildHeaderSuggestions。
          keySuggestions={headerSuggestions}
          varEntries={varEntries}
          envName={envName}
        />
      )}
      {tab === 'body' && (
        <BodyEditor
          value={body}
          onChange={(body) => patchDraft({ body })}
          // 错误由父组件钉住：校验是 发送 / 保存 / 格式化 触发的动作，而非从输入派生
          // 出的值。
          validationError={validationError}
          // 格式化 已经解析过内容，其结果直接定徽标。此处再跑 `runValidation` 检查的
          // 是格式化前的草稿（本次渲染的闭包），即用户刚解析失败的那份内容。
          onFormatResult={onFormatResult}
          varEntries={varEntries}
          envName={envName}
        />
      )}
      {tab === 'auth' && (
        <AuthEditor
          value={auth}
          onChange={(auth) => patchDraft({ auth })}
          varEntries={varEntries}
          envName={envName}
        />
      )}
      {tab === 'docs' && <DocsEditor value={docs} onChange={(docs) => patchDraft({ docs })} />}
      {tab === 'script' && (
        <ScriptPanel
          preRequest={preRequestScript}
          test={testScript}
          onPreRequestChange={(preRequestScript) => patchDraft({ preRequestScript })}
          onTestChange={(testScript) => patchDraft({ testScript })}
        />
      )}
      {tab === 'tls' && (
        <TlsPanel
          value={tls}
          globalSettings={globalTls}
          onChange={(next) => patchDraft({ tls: next })}
        />
      )}
    </div>
  );
}
