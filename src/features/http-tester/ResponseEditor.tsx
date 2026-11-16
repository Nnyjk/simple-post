/**
 * 响应定义编辑区 —— 这个接口会回什么。
 *
 * 与 `BodyEditor` 右侧的 Schema 区对称：那边写请求的固定参考信息，这里写响应的。两者都
 * **只是文档**：不参与发送，也不从调用历史里反推，导出 OpenAPI 时才读它们。
 *
 * 字段规则不在这里实现第二份：状态码语法、同份内重复、`schema` 必须是对象、`contentType`
 * 的 `type/subtype`，都只在 `simple_post_core::domain::response_problems` 判一次。保存被拒时
 * 由调用方把后端原话显示出来。
 */
import { useEffect, useRef, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import { ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react';
import type { ResponseDefinition } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { EmptyHint } from '@/components/ui/empty-hint';
import { Input } from '@/components/ui/input';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import {
  appendResponse,
  moveResponse,
  patchResponse,
  removeResponse,
} from './model/responseDefinitions';

interface Props {
  value: ResponseDefinition[];
  onChange: (responses: ResponseDefinition[]) => void;
}

/**
 * 状态码的常用取值：具体码、整段、`default`。`<datalist>` 只是候选，输入框仍收任意文本 ——
 * 是否合法由后端判，界面不预先拦。
 */
const STATUS_SUGGESTIONS = [
  '200',
  '201',
  '202',
  '204',
  '301',
  '302',
  '304',
  '400',
  '401',
  '403',
  '404',
  '405',
  '409',
  '422',
  '429',
  '500',
  '502',
  '503',
  '504',
  '2XX',
  '4XX',
  '5XX',
  'default',
];

const CONTENT_TYPE_SUGGESTIONS = [
  'application/json',
  'application/problem+json',
  'application/xml',
  'application/octet-stream',
  'text/plain',
  'text/html',
  'text/csv',
];

// `<datalist>` 靠文档级 id 关联，故无论多少条定义都只输出一个列表元素。
const STATUS_DATALIST_ID = 'response-status-suggestions';
const CONTENT_TYPE_DATALIST_ID = 'response-content-type-suggestions';

// 两个 JSON 编辑器共用同一份语言支持与基础设置：每渲染一次新建一个实例会让 CodeMirror
// 重新配置一遍编辑器。
const JSON_EXTENSIONS = [json()];
const BASIC_SETUP = {
  lineNumbers: true,
  foldGutter: true,
  highlightActiveLine: true,
  highlightActiveLineGutter: true,
} as const;

export function ResponseEditor({ value, onChange }: Props) {
  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="text-xs font-medium">响应定义</p>
          <p className="text-[10px] text-muted-foreground">
            这里填的是文档用的响应定义，发送请求时不读它 —— 它不做断言，也不会替你发一次请求。
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="h-7 shrink-0"
          onClick={() => onChange(appendResponse(value))}
        >
          <Plus className="h-3.5 w-3.5" />
          新增响应
        </Button>
      </div>

      {value.length === 0 ? (
        <EmptyHint className="py-8">
          这里填的是文档用的响应定义，发送请求时不读它。导出 OpenAPI 时按这里的定义写出响应。
        </EmptyHint>
      ) : (
        <div className="flex flex-col gap-3">
          {value.map((row, index) => (
            <ResponseCard
              // 新行的 id 是空串（落盘时才由后端铸），故退回到位置做键。文本类状态都在
              // 受控输入里，位置换掉不会把内容留在旧卡片上。
              key={row.id || `new-${index}`}
              row={row}
              index={index}
              total={value.length}
              onChange={(patch) => onChange(patchResponse(value, index, patch))}
              onRemove={() => onChange(removeResponse(value, index))}
              onMove={(delta) => onChange(moveResponse(value, index, delta))}
            />
          ))}
        </div>
      )}

      <datalist id={STATUS_DATALIST_ID}>
        {STATUS_SUGGESTIONS.map((status) => (
          <option key={status} value={status} />
        ))}
      </datalist>
      <datalist id={CONTENT_TYPE_DATALIST_ID}>
        {CONTENT_TYPE_SUGGESTIONS.map((contentType) => (
          <option key={contentType} value={contentType} />
        ))}
      </datalist>
    </div>
  );
}

/** 一条响应定义。状态码与说明并排在上，Schema 与示例各自占一段。 */
function ResponseCard({
  row,
  index,
  total,
  onChange,
  onRemove,
  onMove,
}: {
  row: ResponseDefinition;
  index: number;
  total: number;
  onChange: (patch: Partial<ResponseDefinition>) => void;
  onRemove: () => void;
  onMove: (delta: -1 | 1) => void;
}) {
  return (
    <div className="rounded-md border border-border p-3">
      <div className="mb-2 flex items-center gap-1">
        <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
          响应 {index + 1}
        </span>
        <div className="ml-auto flex items-center gap-0.5">
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            disabled={index === 0}
            onClick={() => onMove(-1)}
            aria-label="上移这一条响应"
          >
            <ArrowUp className="h-3 w-3 text-muted-foreground" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            disabled={index === total - 1}
            onClick={() => onMove(1)}
            aria-label="下移这一条响应"
          >
            <ArrowDown className="h-3 w-3 text-muted-foreground" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={onRemove}
            aria-label="删掉这一条响应"
          >
            <Trash2 className="h-3 w-3 text-muted-foreground" />
          </Button>
        </div>
      </div>

      <div className="mb-2 grid grid-cols-[9rem_minmax(0,1fr)] gap-2">
        <label className="block">
          <span className="mb-1 block text-[10px] text-muted-foreground">状态码</span>
          <Input
            value={row.status}
            onChange={(e) => onChange({ status: e.target.value })}
            list={STATUS_DATALIST_ID}
            placeholder="200 / 2XX / default"
            autoComplete="off"
            className="h-7 font-mono text-xs"
            aria-label="响应状态码"
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-[10px] text-muted-foreground">Content-Type</span>
          <Input
            value={row.contentType}
            onChange={(e) => onChange({ contentType: e.target.value })}
            list={CONTENT_TYPE_DATALIST_ID}
            placeholder="application/json"
            autoComplete="off"
            className="h-7 font-mono text-xs"
            aria-label="响应 Content-Type"
          />
          <span className="mt-1 block text-[10px] text-muted-foreground">
            留空 = 不写 content 块
          </span>
        </label>
      </div>

      <label className="mb-2 block">
        <span className="mb-1 block text-[10px] text-muted-foreground">说明</span>
        <Input
          value={row.description}
          onChange={(e) => onChange({ description: e.target.value })}
          placeholder="这个状态码代表什么"
          className="h-7 text-xs"
          aria-label="响应说明"
        />
      </label>
      {!row.description.trim() && (
        <p className="mb-2 text-[10px] text-warning-foreground">
          OpenAPI 把说明列为必填，空着也能保存；导出时这一条会写成空说明。
        </p>
      )}

      <JsonObjectField
        label="Schema"
        hint="响应正文的 JSON Schema，只用于文档与导出"
        value={row.schema}
        onChange={(schema) => onChange({ schema })}
      />
      <ExampleField value={row.example ?? ''} onChange={(example) => onChange({ example })} />
    </div>
  );
}

/**
 * JSON 对象的文本框 ↔ 对象之间的桥。
 *
 * 文本框必须自己留一份原文：用户打到一半的 JSON 还解析不出来，若每键都把对象回写成规范
 * JSON，光标和刚输入的那几个字符会被整篇替换掉。故文本留在本地，只有解析成对象才回写；
 * 解析不出来时保留上一份对象不动，并把错误就地显示。
 */
function JsonObjectField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: Record<string, unknown> | undefined;
  onChange: (next: Record<string, unknown> | undefined) => void;
}) {
  const isDark = useIsDark();
  const canonical = value === undefined ? '' : JSON.stringify(value);
  const [text, setText] = useState(() =>
    value === undefined ? '' : JSON.stringify(value, null, 2),
  );
  const [error, setError] = useState<string | null>(null);
  // 上一次回写到文本框的那份定义。父组件换了一份（上下移动、换了端点）就整篇重置；
  // 用户自己的键入不会触发重置，否则打到一半的文本会被冲掉。
  const mirrored = useRef(canonical);

  useEffect(() => {
    if (canonical === mirrored.current) return;
    mirrored.current = canonical;
    setText(value === undefined ? '' : JSON.stringify(value, null, 2));
    setError(null);
  }, [canonical, value]);

  const handleChange = (next: string) => {
    setText(next);
    const trimmed = next.trim();
    if (!trimmed) {
      mirrored.current = '';
      setError(null);
      onChange(undefined);
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed);
    } catch (e) {
      setError((e as Error).message);
      return;
    }
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
      setError('这段 JSON 必须是对象');
      return;
    }
    mirrored.current = JSON.stringify(parsed);
    setError(null);
    onChange(parsed as Record<string, unknown>);
  };

  return (
    <div className="mb-2">
      <div className="mb-1 flex items-baseline gap-2 text-[10px] text-muted-foreground">
        <span className="font-medium text-foreground">{label}</span>
        <span>{hint}</span>
      </div>
      <div className="overflow-hidden rounded-md border border-border">
        <CodeMirror
          value={text}
          onChange={handleChange}
          extensions={JSON_EXTENSIONS}
          theme={isDark ? oneDark : lightCodeTheme}
          basicSetup={BASIC_SETUP}
          placeholder={'{\n  "type": "object"\n}'}
          height="160px"
          className="text-sm"
        />
      </div>
      {error && (
        <p role="status" aria-live="polite" className="mt-1 text-[10px] text-destructive">
          Schema JSON 有误：{error}（改对之前不会写进定义）
        </p>
      )}
    </div>
  );
}

/**
 * 示例正文。一个编辑器收两种内容：能解析成 JSON 的按 JSON 导出（OpenAPI 里写成值），
 * 解析不出来的按字符串导出，故这里不做 JSON 校验，也不阻止输入。
 */
function ExampleField({
  value,
  onChange,
}: {
  value: string;
  onChange: (next: string | null) => void;
}) {
  const isDark = useIsDark();
  return (
    <div>
      <div className="mb-1 flex items-baseline gap-2 text-[10px] text-muted-foreground">
        <span className="font-medium text-foreground">示例</span>
        <span>能解析成 JSON 就写成 JSON 值，否则写成纯文本</span>
      </div>
      <div className="overflow-hidden rounded-md border border-border">
        <CodeMirror
          value={value}
          onChange={(next) => onChange(next === '' ? null : next)}
          extensions={JSON_EXTENSIONS}
          theme={isDark ? oneDark : lightCodeTheme}
          basicSetup={BASIC_SETUP}
          placeholder={'{\n  "id": 42\n}'}
          height="140px"
          className="text-sm"
        />
      </div>
    </div>
  );
}
