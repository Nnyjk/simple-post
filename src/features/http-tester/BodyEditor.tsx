import { useMemo, useRef } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import type { BodyMode, RequestBody } from '@/types/domain';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { AlertTriangle, Wand2 } from 'lucide-react';
import { cn, tryFormatJson, type FormatResult } from '@/lib/utils';
import { listVars } from '@/lib/url';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

interface Props {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  /**
   * 父组件钉住的格式错误，或 `null`。
   *
   * 归父组件所有，因为校验是**动作**而非派生值：它在 发送 / 保存 / 格式化 时跑，结论一直
   * 保留到下一次触发（见 `lib/request-validation`）。本组件因此不能自己消徽标，
   * 打完字把 JSON 改对并不等于它能解析。
   *
   * 不放进 `BodyContent` 自己的 state，是为了让错误活过切到别的 HTTP 标签再切回来：
   * 它属于请求，不属于本组件挂了多久。
   */
  validationError?: string | null;
  /**
   * 格式化之后带着解析结果回调。
   *
   * 回传结果而非一个 `ok` 布尔：父组件据此结算钉住的错误，而不是重跑一遍校验。
   * 回调触发时格式化后的内容已进入草稿但父组件还没重渲染，在那里重查等于去校验用户
   * 刚刚格式化失败的那份内容，再把已经修好的错误重新钉上。
   */
  onFormatResult?: (result: FormatResult) => void;
}

const BODY_TABS: { id: BodyMode; label: string }[] = [
  { id: 'none', label: 'none' },
  { id: 'json', label: 'json' },
  { id: 'form', label: 'form' },
  { id: 'multipart', label: 'multipart' },
  { id: 'raw', label: 'raw' },
];

export function BodyEditor({ value, onChange, validationError, onFormatResult }: Props) {
  const handleFormat = () => {
    // 带占位符的模板**不能**格式化：格式化会把文本改写成解析出来的值，而占位符在
    // 模板阶段根本不是 JSON。故这里直接说清楚，而不是拿 JSON 解析器的原话去说一件
    // 不是 JSON 的事。
    if (listVars(value.content).length > 0) {
      onFormatResult?.({ ok: false, error: '带 {{变量}} 的模板不能格式化' });
      return;
    }
    const result = tryFormatJson(value.content);
    if (result.ok) {
      onChange({ ...value, content: result.formatted });
    }
    // 无论成败都上报：父组件据此「消徽标」或「钉住」，与 发送 / 保存 同一套规则。
    onFormatResult?.(result);
  };

  return (
    <div className="px-4 py-3">
      <div className="mb-2 flex items-center gap-2">
        <Tabs
          items={BODY_TABS.map((t) => ({ id: t.id, label: t.label }))}
          value={value.mode}
          onChange={(id) => {
            onChange({ ...value, mode: id as BodyMode });
          }}
        />
        <span className="text-[10px] text-muted-foreground">
          {value.mode === 'json' && 'Content-Type: application/json'}
          {value.mode === 'form' && 'Content-Type: application/x-www-form-urlencoded'}
          {/* boundary 每次发送现生成，故这里不能写死整条 Content-Type */}
          {value.mode === 'multipart' &&
            'Content-Type: multipart/form-data（boundary 发送时生成）· 分片填 JSON 数组'}
          {value.mode === 'raw' && 'Content-Type: text/plain'}
        </span>
        <Button
          variant="outline"
          size="sm"
          className="ml-auto h-7"
          disabled={value.mode !== 'json' || !value.content.trim()}
          onClick={handleFormat}
          data-testid="format-body-btn"
        >
          <Wand2 className="h-3.5 w-3.5" />
          格式化
        </Button>
      </div>

      {value.mode === 'none' ? (
        <div className="flex h-32 items-center justify-center rounded-md border border-dashed border-border text-sm text-muted-foreground">
          此请求无 body
        </div>
      ) : (
        <BodyContent value={value} onChange={onChange} validationError={validationError} />
      )}
    </div>
  );
}

function BodyContent({ value, onChange, validationError }: Props) {
  const isDark = useIsDark();
  const pillRef = useRef<HTMLDivElement>(null);

  // 用户已经看不到 body 时还钉着错误，徽标就成了另一个国家的提示。凡是离开 json 编辑器
  // 就作废它，下次 发送 / 保存 会重新检查此刻的内容。
  const shownError = value.mode === 'json' ? (validationError ?? null) : null;

  // 显隐也交给 GSAP，与过渡同属一处：报错期间元素不条件卸载，同一个结论重跑一次也不会
  // 和 ref 抢先后。
  useGSAP(() => {
    const el = pillRef.current;
    if (!el || !shownError) return;
    return motionSafe(() => {
      gsap.fromTo(
        el,
        { autoAlpha: 0, y: -8, scale: 0.96 },
        { autoAlpha: 1, y: 0, scale: 1, duration: 0.28, ease: 'power3.out' },
      );
      // 抖一下就定住。动效的意义是「现在看我」，一直动的徽标反而没人读。
      gsap.fromTo(
        el,
        { x: 0 },
        {
          keyframes: [
            { x: -3, duration: 0.06 },
            { x: 3, duration: 0.06 },
            { x: -2, duration: 0.06 },
            { x: 0, duration: 0.06 },
          ],
        },
      );
    });
  }, [shownError]);

  const hintText = useMemo(
    () => (shownError ? `JSON 格式有误：${shownError}` : null),
    [shownError],
  );

  return (
    <div className="relative">
      <div className="overflow-hidden rounded-md border border-border">
        <CodeMirror
          value={value.content}
          onChange={(v) => onChange({ ...value, content: v })}
          // multipart 的 `content` 也是一个 JSON 文本（分片数组），同样要 json 高亮
          extensions={value.mode === 'json' || value.mode === 'multipart' ? [json()] : []}
          theme={isDark ? oneDark : lightCodeTheme}
          basicSetup={{
            lineNumbers: true,
            foldGutter: true,
            highlightActiveLine: true,
            highlightActiveLineGutter: true,
          }}
          height="280px"
          className="text-sm"
        />
      </div>

      {/*
        浮在编辑器顶部而非堆在下方：280 px 的 CodeMirror 挡在用户和徽标之间，错误就没人看见。
        常驻不自动消失也不手动关，只在校验通过后离场。`pointer-events-auto` 换来悬停时
        完整未截断的文案，代价只是编辑器右上角一小块死区。
      */}
      {hintText && (
        <div
          ref={pillRef}
          role="status"
          aria-live="polite"
          className={cn(
            'absolute right-2 top-2 z-10 flex max-w-[min(420px,60%)] items-center gap-1.5',
            'rounded-full border px-2.5 py-1 text-[11px] shadow-md',
            // 实心 destructive，不用浅色底。`--destructive-foreground` 两个主题都是近白，
            // 它是「压在饱和填充上」的文字色；配 `bg-background/85` 那种浅底就是白字白底。
            // 这对 token 两个主题解算一致，因为没有任何一侧覆写它们。
            'border-destructive bg-destructive text-destructive-foreground',
          )}
          title={hintText}
          data-testid="json-status-pill"
        >
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" aria-hidden />
          <span className="truncate font-mono">{hintText}</span>
        </div>
      )}
    </div>
  );
}
