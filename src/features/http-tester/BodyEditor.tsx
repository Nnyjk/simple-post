import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import CodeMirror, { EditorView, Prec, type ViewUpdate } from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { xml } from '@codemirror/lang-xml';
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
import { parseSchemaView } from '@/lib/schema-view';
import { SchemaTable } from './SchemaTable';
import { changedRange } from './model/changedRange';
import { BinaryBodyEditor, FormFieldsEditor, MultipartPartsEditor } from './BodyTables';
import {
  rectAnchor,
  useVarHint,
  VarHintPicker,
  varHintKeyAction,
  type VarEntry,
  type VarHint,
} from './var-hint';

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
   * 回传结果而非一个 `ok` 布尔：父组件据此清除本次校验报出的错误，而不是重跑一遍校验。
   * 回调触发时格式化后的内容已进入草稿但父组件还没重渲染，在那里重查等于去校验用户
   * 刚刚格式化失败的那份内容，再把已经清除的错误重新显示出来。
   */
  onFormatResult?: (result: FormatResult) => void;
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName?: string;
}

/**
 * 请求体的模式页签。
 *
 * 七种形态与 `BodyMode` 一一对应：`none` / `json` / `form`（x-www-form-urlencoded）/
 * `multipart`（form-data）/ `xml` / `raw`（Text）/ `binary`。
 *
 * 标签用**协议里的名字**而不是内部代号：`form` 这个名字在界面上看不出它发出去的是
 * `x-www-form-urlencoded`，而 `multipart` 看不出它是 `form-data`。
 */
const BODY_TABS: { id: BodyMode; label: string }[] = [
  { id: 'none', label: '无' },
  { id: 'json', label: 'JSON' },
  { id: 'form', label: 'x-www-form-urlencoded' },
  { id: 'multipart', label: 'form-data' },
  { id: 'xml', label: 'XML' },
  { id: 'raw', label: 'Text' },
  { id: 'binary', label: 'binary' },
];

/**
 * 请求体编辑区 —— 左右两栏。
 *
 * 左栏是**这一次发什么**：模式页签、Content-Type 说明、格式化按钮与载荷编辑器。
 * 右栏是**这个接口接受什么**：文档里那份参数定义（JSON Schema）。
 *
 * 两栏回答不同的问题，故它们各有一个编辑器、各自一份说明；合成一个编辑器时两种内容
 * 只能挤在同一段 JSON 里，于是「写接口定义」与「调试一次请求」互相覆盖。
 */
export function BodyEditor({
  value,
  onChange,
  validationError,
  onFormatResult,
  varEntries,
  envName,
}: Props) {
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
    <div className="flex h-full min-h-0 max-h-[70vh] gap-4 px-4 py-3">
      {/* 左栏：这一次实际发出的载荷。 */}
      <div className="flex min-h-0 min-w-0 w-1/2 flex-col gap-2 overflow-y-auto">
        <div className="flex flex-wrap items-center gap-2">
          <Tabs
            items={BODY_TABS.map((t) => ({ id: t.id, label: t.label }))}
            value={value.mode}
            onChange={(id) => {
              onChange({ ...value, mode: id as BodyMode });
            }}
          />
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
        <p className="text-[10px] text-muted-foreground">{contentTypeText(value.mode)}</p>
        {value.mode === 'none' ? (
          <NoBodyHint />
        ) : value.mode === 'form' ? (
          <FormFieldsEditor
            content={value.content}
            onChange={(content) => onChange({ ...value, content })}
            varEntries={varEntries}
            envName={envName}
          />
        ) : value.mode === 'multipart' ? (
          <MultipartPartsEditor
            content={value.content}
            onChange={(content) => onChange({ ...value, content })}
            varEntries={varEntries}
            envName={envName}
          />
        ) : value.mode === 'binary' ? (
          <BinaryBodyEditor
            content={value.content}
            onChange={(content) => onChange({ ...value, content })}
          />
        ) : (
          <BodyContent
            value={value}
            onChange={onChange}
            validationError={validationError}
            varEntries={varEntries}
            envName={envName}
          />
        )}
      </div>

      {/* 右栏：文档里那份参数定义。 */}
      <div className="flex min-h-0 min-w-0 w-1/2 flex-col gap-2 overflow-y-auto border-l border-border pl-4">
        <SchemaSection
          value={value.schema ?? ''}
          onChange={(schema) => onChange({ ...value, schema: schema || undefined })}
        />
      </div>
    </div>
  );
}

/** 当前模式对应的 Content-Type 说明。 */
function contentTypeText(mode: BodyMode): string {
  if (mode === 'json') return 'Content-Type: application/json';
  if (mode === 'form') return 'Content-Type: application/x-www-form-urlencoded';
  // boundary 每次发送现生成，故这里不能写死整条 Content-Type
  if (mode === 'multipart') {
    return 'Content-Type: multipart/form-data（boundary 发送时生成）· 分片填表格';
  }
  if (mode === 'xml') return 'Content-Type: application/xml';
  if (mode === 'raw') return 'Content-Type: text/plain';
  if (mode === 'binary') return 'Content-Type: 取所选文件的类型，缺省 application/octet-stream';
  return '此请求没有请求体';
}

/** 左栏在 none 模式下的提示。 */
function NoBodyHint() {
  return (
    <div className="flex h-32 items-center justify-center rounded-md border border-dashed border-border text-sm text-muted-foreground">
      此请求无 body
    </div>
  );
}

/**
 * 载荷编辑器 —— 左栏那个 CodeMirror。
 *
 * 变量候选提示接在编辑器里：敲 `{{` 时浮出环境变量与动态变量，选中即把 `{{key}}` 写进
 * 文档。编辑器不是普通输入框，故 `var-hint` 的那条规则照用，写回与键盘导航在这里自己
 * 实现。
 */
function BodyContent({
  value,
  onChange,
  validationError,
  varEntries,
  envName,
}: {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  validationError?: string | null;
  varEntries: VarEntry[];
  envName?: string;
}) {
  const isDark = useIsDark();
  const pillRef = useRef<HTMLDivElement>(null);
  /** 编辑器实例：插入变量与定位浮层都要用它。 */
  const viewRef = useRef<EditorView | null>(null);
  /** 文档当前文本与光标位置 —— 提示的「半个变量名」取的是光标之前的那一段。 */
  const [doc, setDoc] = useState(value.content);
  const [caret, setCaret] = useState(0);
  /** 光标位置的副本：浮层每次定位都要读它，不能只靠 state。 */
  const caretRef = useRef(0);

  // CodeMirror 的回调要保持稳定，否则 @uiw 每次渲染都重新配置扩展。故最新的请求体、
  // 写回回调与提示状态放在这份 ref 里，回调按 ref 取当前值。
  const latest = useRef<{
    value: RequestBody;
    onChange: (b: RequestBody) => void;
    hint: VarHint | null;
  }>({ value, onChange, hint: null });

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

  /** 把新文本写回编辑器与父组件 —— 插入变量走的就是这条路径。 */
  const applyText = useCallback((next: string) => {
    const view = viewRef.current;
    if (view) {
      // 只替换真正变了的那一段。整篇替换会让编辑器重建语法树，折叠状态与选区一并丢掉。
      const range = changedRange(view.state.doc.toString(), next);
      view.dispatch({ changes: { from: range.from, to: range.to, insert: range.insert } });
    }
    setDoc(next);
    latest.current.onChange({ ...latest.current.value, content: next });
  }, []);

  /** 插入完成后把光标放到刚写入的 `{{key}}` 之后。 */
  const moveCaret = useCallback((pos: number) => {
    viewRef.current?.dispatch({ selection: { anchor: pos } });
  }, []);

  const hint = useVarHint({
    value: doc,
    onChange: applyText,
    onCaret: moveCaret,
    entries: varEntries,
    caret,
  });

  useEffect(() => {
    latest.current = { value, onChange, hint };
  });

  // 父组件改了内容（例如「格式化」）时本地这份跟着走：那条路径不经过编辑器自己的
  // onChange（编辑器收到的是外部改动）。
  useEffect(() => {
    setDoc(value.content);
  }, [value.content]);

  /** 编辑器里的文本变了：文档是编辑器自己改的，故只更新浮层开关，不把文本再写回一遍。 */
  const handleEditorChange = useCallback((v: string, vu: ViewUpdate) => {
    setDoc(v);
    latest.current.hint?.sync(v.slice(0, vu.state.selection.main.head));
    latest.current.onChange({ ...latest.current.value, content: v });
  }, []);

  /** 光标移动也要记下来：光标不在文末时，「半个变量名」取的是光标之前的那一段。 */
  const handleUpdate = useCallback((vu: ViewUpdate) => {
    if (!vu.docChanged && !vu.selectionSet) return;
    const head = vu.state.selection.main.head;
    caretRef.current = head;
    setCaret(head);
  }, []);

  /**
   * 键盘：↑↓ 换高亮、Enter / Tab 插入、Esc 关闭。
   *
   * `useVarHint` 的 `keyDown` 吃的是 React 事件，故这里按同样的规则自己实现一遍。
   */
  const handleKeyDown = useCallback((event: KeyboardEvent) => {
    const h = latest.current.hint;
    if (!h) return false;
    const entries = h.entries;
    const entry = entries[h.selected] ?? entries[0];
    // 规则只有一份：普通输入框与编辑器吃的是不同类型的事件，判定交给同一个纯函数。
    const action = varHintKeyAction(
      {
        key: event.key,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        altKey: event.altKey,
        isComposing: event.isComposing,
        keyCode: event.keyCode,
      },
      { open: h.open, hasEntry: entry !== undefined },
    );
    if (action === 'none') return false;
    event.preventDefault();
    if (action === 'next') {
      if (entries.length > 0) h.setSelected((h.selected + 1) % entries.length);
      return true;
    }
    if (action === 'prev') {
      if (entries.length > 0) h.setSelected((h.selected - 1 + entries.length) % entries.length);
      return true;
    }
    if (action === 'insert') {
      if (entry) h.insert(entry.key);
      return true;
    }
    if (action === 'close') {
      h.close();
      return true;
    }
    return false;
  }, []);

  // 键盘要抢在编辑器自己的键位表前面：`Prec.highest` 让这个处理器先跑，它返回 true 即
  // 拦下这次按键，编辑器不会再插入换行、也不会把焦点移出编辑器。
  const hintKeys = useMemo(
    () => Prec.highest(EditorView.domEventHandlers({ keydown: handleKeyDown })),
    [handleKeyDown],
  );

  const extensions = useMemo(
    () => [
      // json 与 xml 是两种语言，各自带高亮；其余模式是纯文本。
      ...(value.mode === 'json' ? [json()] : []),
      ...(value.mode === 'xml' ? [xml()] : []),
      hintKeys,
    ],
    [value.mode, hintKeys],
  );

  // 浮层锚在光标处：编辑器里没有「光标所在的那个输入框」，故按光标位置算出矩形。
  const anchor = useMemo(
    () =>
      rectAnchor(() => {
        const view = viewRef.current;
        if (!view) return null;
        return view.coordsAtPos(caretRef.current) ?? null;
      }),
    [],
  );

  return (
    <div className="relative">
      <div className="overflow-hidden rounded-md border border-border">
        <CodeMirror
          value={value.content}
          onCreateEditor={(view) => {
            viewRef.current = view;
          }}
          onChange={handleEditorChange}
          onUpdate={handleUpdate}
          extensions={extensions}
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

      <VarHintPicker
        open={hint.open}
        onClose={hint.close}
        entries={hint.entries}
        selected={hint.selected}
        onSelectedChange={hint.setSelected}
        onSelect={hint.insert}
        anchor={anchor}
        envName={envName}
      />
    </div>
  );
}

/** Schema 编辑区的两个视图。 */
type SchemaViewMode = 'structure' | 'json';

/**
 * Schema 编辑区初次打开时显示哪个视图。
 *
 * 文本能解析成结构、且不是空的，就落在「结构」视图 —— 读定义靠的是那张表，
 * 不是一坨 JSON。解析不出来或还没写，就落在「JSON」视图，因为那里才写得下去。
 */
function initialSchemaView(text: string): SchemaViewMode {
  const parsed = parseSchemaView(text);
  return parsed.ok && text.trim() ? 'structure' : 'json';
}

/**
 * 请求体的 Schema 编辑区 —— 文档里那份「参数长什么样」的定义。
 *
 * 为什么与载荷编辑器分开
 * ----------------------
 * 两者回答的是不同的问题：左栏那个编辑器是**这一次发什么**，本编辑器是**这个接口接受
 * 什么**。合成一个编辑器时，两种内容只能挤在同一段 JSON 里，于是「写接口定义」与
 * 「调试一次请求」在同一个输入框里互相覆盖。两个字段各有一个编辑器、两句说明。
 *
 * Schema 不参与发包（导出 OpenAPI 时才读它），故它写坏了不阻止发送与保存：导出那侧
 * 遇到不可解析的 schema 会自己报一条诊断，导出不会因此失败。
 *
 * 常驻不折叠：模式切成 none 之后 body 里的 schema 仍在（模式切换保留整个 body），
 * 藏起来用户就再也删不掉那份残留。
 *
 * 为什么有两个视图
 * --------------
 * 「结构」视图把定义画成一张属性表，读参数时不必逐字读 JSON；「JSON」视图保留编辑器，
 * 供用户继续改这段文本。两个视图读写的是同一份文本（`value.schema`），故它们不可能
 * 各说各话：用户在 JSON 视图改完，结构视图随即按新文本重画。
 */
function SchemaSection({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const isDark = useIsDark();
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<SchemaViewMode>(() => initialSchemaView(value));
  // 用户是否自己选过视图。用户选过之后，视图就不再由文本的解析状态决定 —— 否则用户
  // 正在 JSON 视图里改字，文本一改合法就被拽去结构视图，编辑器会从眼皮底下消失。
  const userPickedView = useRef(false);

  const parsed = useMemo(() => parseSchemaView(value), [value]);

  // 文本改动后重新决定视图。
  useEffect(() => {
    if (!parsed.ok) {
      // 解析不出来时只有 JSON 视图有东西可改，视图落回 JSON。
      setView('json');
      return;
    }
    if (userPickedView.current) return;
    setView(value.trim() ? 'structure' : 'json');
  }, [parsed, value]);

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
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        className="mb-1.5 flex items-center gap-2 text-xs font-medium"
        data-testid="body-schema-toggle"
      >
        Schema（文档定义，不参与发送）
        <span className="text-[10px] font-normal text-muted-foreground">
          这份接口接受什么参数，只用于文档与导出
        </span>
      </div>

      <div className="mb-1.5 flex items-center gap-2">
        <Tabs
          variant="segmented"
          items={[
            { id: 'structure', label: '结构' },
            { id: 'json', label: 'JSON' },
          ]}
          value={view}
          onChange={(id) => {
            userPickedView.current = true;
            setView(id as SchemaViewMode);
          }}
        />
        <span className="text-[10px] text-muted-foreground">
          {view === 'structure' ? '按字段列出这份定义' : '直接编辑这段 JSON'}
        </span>
      </div>

      {view === 'structure' ? (
        <SchemaTable result={parsed} />
      ) : (
        <div className="relative">
          <div className="overflow-hidden rounded-md border border-border">
            <CodeMirror
              value={value}
              onChange={(v) => {
                onChange(v);
                // 用户在这个视图里改字，视图就以这里为准：文本一改合法，结构视图
                // 也不该把编辑器从眼皮底下拽走。
                userPickedView.current = true;
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
      )}
    </div>
  );
}
