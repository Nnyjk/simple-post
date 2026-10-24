import { useEffect, useMemo, useRef, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import type { BodyMode, RequestBody } from '@/types/domain';
import { Tabs } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { AlertTriangle, ChevronDown, ChevronRight, Wand2 } from 'lucide-react';
import { cn, tryFormatJson, type FormatResult } from '@/lib/utils';
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
        <>
          <BodyContent value={value} onChange={onChange} validationError={validationError} />
          {/* Schema 与载荷分成两个编辑器：两者回答的是不同的问题，挤在同一段 JSON 里
              就没有「这份是定义、那份是取值」的分别。

              清空时写 `undefined` 而不是空串：空串会被存成 `Some("")`，导出 OpenAPI
              时它就是一份「解析不了的 schema」，于是每次导出都报一条没意义的诊断。 */}
          <SchemaSection
            value={value.schema ?? ''}
            onChange={(schema) => onChange({ ...value, schema: schema || undefined })}
          />
        </>
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

/**
 * 请求体的 Schema 编辑区 —— 文档里那份「参数长什么样」的定义。
 *
 * 为什么与载荷编辑器分开
 * ----------------------
 * 两者回答的是不同的问题：上面的编辑器是**这一次发什么**，本编辑器是**这个接口接受
 * 什么**。合成一个编辑器时，两种内容只能挤在同一段 JSON 里，于是「写接口定义」与
 * 「调试一次请求」在同一个输入框里互相覆盖 —— AI 定义接口时把 Schema 写进了载荷，
 * 调试时又把取值写成了定义。分开之后，两个字段各有一个编辑器、两句说明。
 *
 * Schema 不参与发包（导出 OpenAPI 时才读它），故它写坏了不阻止发送与保存：导出那侧
 * 遇到不可解析的 schema 会自己报一条诊断，导出不会因此失败。
 *
 * 折叠而非常驻：绝大多数接口只调试、不写文档，常驻一块编辑器只是噪音。已经写了
 * schema 的接口默认展开 —— 那份定义必须看得见，否则它就成了一个只有 MCP 侧能碰的
 * 隐藏字段。
 */
function SchemaSection({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const isDark = useIsDark();
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 已经有 schema 就展开：AI 通过 MCP 写进来的定义，用户必须看得见。
  useEffect(() => {
    if (value.trim()) setOpen(true);
  }, [value]);

  /** 这段文本是不是合法 JSON。空文本不算错 —— 那表示「没写」。 */
  const parseError = (text: string): string | null => {
    const trimmed = text.trim();
    if (!trimmed) return null;
    try {
      JSON.parse(trimmed);
      return null;
    } catch (e) {
      return (e as Error).message;
    }
  };

  return (
    <div className="mt-3">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        title="请求体的 Schema：文档里这份接口接受什么参数。不参与发送。"
        data-testid="body-schema-toggle"
        className={cn(
          'flex w-full items-center gap-1.5 rounded px-1 py-1 text-left text-xs transition-colors',
          'text-muted-foreground hover:bg-overlay-hover hover:text-foreground',
        )}
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0" aria-hidden />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden />
        )}
        <span>Schema（文档定义，不参与发送）</span>
        {!open && value.trim() && (
          <span className="ml-auto rounded bg-muted px-1 text-[10px]">已填写</span>
        )}
      </button>

      {open && (
        <div className="mt-1.5">
          <p className="mb-1.5 text-[11px] text-muted-foreground">
            上一块是这一次发送的载荷；这一块是这份接口的参数定义（JSON Schema），只用于文档与导出。
          </p>
          <div className="relative">
            <div className="overflow-hidden rounded-md border border-border">
              <CodeMirror
                value={value}
                onChange={(v) => {
                  onChange(v);
                  // 改对了就消掉上一轮的结论；还没改对时保留它，直到下一次离开编辑器。
                  const next = parseError(v);
                  if (!next) setError(null);
                }}
                // 离开编辑器时判一次合法性。逐键判会让编辑到一半的 JSON 一直亮着错误，
                // 那正是载荷编辑器不采用实时校验的原因。
                onBlur={() => setError(parseError(value))}
                extensions={[json()]}
                theme={isDark ? oneDark : lightCodeTheme}
                basicSetup={{
                  lineNumbers: true,
                  foldGutter: true,
                  highlightActiveLine: true,
                  highlightActiveLineGutter: true,
                }}
                height="200px"
                className="text-sm"
              />
            </div>
            {error && (
              <div
                role="status"
                aria-live="polite"
                className={cn(
                  'absolute right-2 top-2 z-10 flex max-w-[min(420px,60%)] items-center gap-1.5',
                  'rounded-full border px-2.5 py-1 text-[11px] shadow-md',
                  'border-destructive bg-destructive text-destructive-foreground',
                )}
                title={error}
                data-testid="schema-status-pill"
              >
                <AlertTriangle className="h-3.5 w-3.5 shrink-0" aria-hidden />
                <span className="truncate font-mono">Schema JSON 格式有误：{error}</span>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
