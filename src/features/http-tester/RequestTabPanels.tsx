import { KeyValueEditor, type KeySuggestion } from './KeyValueEditor';
import { BodyEditor } from './BodyEditor-lazy';
import { AuthEditor } from './AuthEditor';
import { DocsEditor } from './DocsEditor-lazy';
import { ScriptPanel } from './ScriptPanel';
import type { EndpointDraft } from '@/stores/store-types';
import type { FormatResult } from '@/lib/utils';
import type { AuthConfig, KeyValue, RequestBody } from '@/types/domain';

/** 请求配置编辑器的页签 id。`script` 是请求脚本（前置 / 测试两段）。 */
export type TesterTabId = 'params' | 'headers' | 'body' | 'auth' | 'docs' | 'script';

/**
 * 页签内容区 —— 只渲染当前那一页编辑器。
 *
 * 每个编辑器都写草稿而非已存的 detail 行；错误徽标由父组件钉住（校验是
 * 发送 / 保存 / 格式化 触发的动作，而非从输入派生出的值）。
 */
export function RequestTabPanels({
  tab,
  params,
  headers,
  body,
  auth,
  docs,
  preRequestScript,
  testScript,
  headerSuggestions,
  validationError,
  patchDraft,
  onFormatResult,
}: {
  tab: TesterTabId;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  docs: string;
  /** 前置脚本的当前文本。 */
  preRequestScript: string;
  /** 测试脚本的当前文本。 */
  testScript: string;
  headerSuggestions: KeySuggestion[];
  validationError: string | null;
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
        />
      )}
      {tab === 'auth' && <AuthEditor value={auth} onChange={(auth) => patchDraft({ auth })} />}
      {tab === 'docs' && <DocsEditor value={docs} onChange={(docs) => patchDraft({ docs })} />}
      {tab === 'script' && (
        <ScriptPanel
          preRequest={preRequestScript}
          test={testScript}
          onPreRequestChange={(preRequestScript) => patchDraft({ preRequestScript })}
          onTestChange={(testScript) => patchDraft({ testScript })}
        />
      )}
    </div>
  );
}
