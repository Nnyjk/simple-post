import { Play, Save, Copy, Check } from 'lucide-react';
import type { RefObject } from 'react';
import { Button } from '@/components/ui/button';
import { MethodPicker } from '@/components/ui/method-picker';
import { Tooltip } from '@/components/ui/tooltip';
import { BaseUrlPicker, type BaseUrlSelection } from './BaseUrlPicker';
import { VarHintInput, type VarEntry } from './var-hint';
import { cn, methodColorVar } from '@/lib/utils';
import type { HttpMethod } from '@/types/domain';

/**
 * 请求栏：方法、地址来源选择器、URL 输入框，以及发送 / 复制 cURL / 保存三个动作。
 *
 * 它渲染**当前有效**的请求（草稿叠已存的 detail）；编辑经回调回写草稿。请求本身的
 * 组装不在这里——它要与发送和 cURL 预览共用同一个对象，故留在 `HttpTester`。
 *
 * URL 输入框带变量候选提示：用户敲 `{{` 时浮出环境变量与动态变量，选中即插入
 * `{{key}}`（见 `var-hint`）。
 */
export function RequestUrlBar({
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
  isDraft,
  varEntries,
  envName,
  onMethodChange,
  onSelectionChange,
  onUrlChange,
  onSend,
  onSave,
  onCopyCurl,
}: {
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
  isDraft: boolean;
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName?: string;
  onMethodChange: (m: HttpMethod) => void;
  onSelectionChange: (next: BaseUrlSelection) => void;
  onUrlChange: (next: string) => void;
  onSend: () => void;
  onSave: () => void;
  onCopyCurl: () => void;
}) {
  return (
    <div className="flex shrink-0 items-center gap-2 border-b border-border bg-card/30 px-4 py-2.5">
      <MethodPicker value={method} onChange={onMethodChange} className="border-0" />
      <BaseUrlPicker
        resolvedBaseUrl={resolvedBaseUrl}
        selection={selection}
        onSelectionChange={onSelectionChange}
      />
      <div className="relative flex min-w-0 flex-1 items-center">
        <VarHintInput
          inputRef={urlInputRef}
          value={liveUrl}
          onChange={onUrlChange}
          entries={varEntries}
          envName={envName}
          // 自定义 让本框承载完整地址，故须明示：placeholder 换成完整 URL、ring 标出
          // 模式 —— 选择器位于用户实际聚焦的框左侧，光靠它的标签不够。
          className={cn(
            'h-9 w-full font-mono text-sm',
            isCustomUrl && 'border-primary/40 ring-1 ring-inset ring-primary/40',
          )}
          placeholder={isCustomUrl || !resolvedBaseUrl ? 'https://api.example.com/path' : '/path'}
          title={
            isCustomUrl
              ? '自定义：直接使用这里填写的完整地址，不再拼接模块地址'
              : resolvedBaseUrl || undefined
          }
          data-testid="url-input"
        />
      </div>
      <Button
        ref={sendBtnRef}
        onClick={onSend}
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
          onClick={onCopyCurl}
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
        onClick={onSave}
        disabled={!dirty}
        className="h-9 gap-1.5"
        data-testid="save-btn"
      >
        <Save className="h-3.5 w-3.5" />
        {isDraft ? '保存接口' : '保存'}
      </Button>
    </div>
  );
}
