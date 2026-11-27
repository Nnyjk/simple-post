import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import CodeMirror, { EditorView, Prec, type ViewUpdate } from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { xml } from '@codemirror/lang-xml';
import { oneDark } from '@codemirror/theme-one-dark';
import type { BodyMode, RequestBody } from '@/types/domain';
import { Tabs } from '@/components/ui/tabs';
import { EmptyHint } from '@/components/ui/empty-hint';
import { FloatToolbar, type FloatToolbarAction } from '@/components/ui/float-toolbar';
import { JsonTreeView } from '@/components/ui/json-tree-view';
import { AlertTriangle, Check, Copy, Wand2, WrapText } from 'lucide-react';
import { cn, tryFormatJson, type FormatResult } from '@/lib/utils';
import { copyText } from '@/lib/clipboard';
import { parseJsonTree } from '@/lib/json-tree';
import { listVars } from '@/lib/url';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
import { parseSchemaView } from '@/lib/schema-view';
import { SchemaTable } from './SchemaTable';
import { changedRange } from './model/changedRange';
import type { TabUploadProgress } from '@/stores/slices/ui-response';
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
   * 父组件钉住的格式错误，或 `null`。校验是动作而非派生值，结论保留到下一次触发
   * （见 `lib/request-validation`），所以徽标由父组件消，本组件只显示。
   */
  validationError?: string | null;
  /**
   * 格式化之后带着解析结果回调。父组件据此清除本次校验报出的错误，而不是重跑校验：
   * 回调触发时格式化后的内容已进入草稿但父组件还没重渲染，重查会重新报错。
   */
  onFormatResult?: (result: FormatResult) => void;
  /** 变量候选：环境变量与动态变量并进同一份列表。 */
  varEntries: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName?: string;
  /** 当前标签 id：文件分片的读取状态按它重置。 */
  endpointId?: string;
  /** 当前标签这一次发送的传输进度。 */
  uploadProgress?: TabUploadProgress | null;
}

/**
 * 请求体的七种形态，与 `BodyMode` 一一对应。标签用协议里的名字：`form` 发出的是
 * `x-www-form-urlencoded`，`multipart` 发出的是 `form-data`。
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
 * 请求体编辑区 —— 左右两栏。左栏是**这一次发什么**（模式页签、Content-Type 说明、
 * 载荷编辑器），右栏是**这个接口接受什么**（JSON Schema）。两者回答不同的问题，
 * 故各有一个编辑器；合成一个编辑器时两种内容会挤在同一段 JSON 里互相覆盖。
 */
export function BodyEditor({
  value,
  onChange,
  validationError,
  onFormatResult,
  varEntries,
  envName,
  endpointId,
  uploadProgress,
}: Props) {
  // 只有 JSON 模式有「树 / Raw」两副面孔。别的模式（尤其是带文件的 multipart）里 `content`
  // 是另一套结构的 JSON 文本，体积可达几十 MB，为它解一次树只为决定一个用不上的视图不值。
  const [bodyView, setBodyView] = useState<BodyViewMode>(() =>
    value.mode === 'json' && parseJsonTree(value.content).ok ? 'tree' : 'raw',
  );
  // 用户是否自己选过视图。选过之后视图不再由内容的解析状态决定，否则用户刚在 Raw 里
  // 把 JSON 改到合法就被拽去树视图，编辑器会从眼皮底下消失。
  const userPickedBodyView = useRef(false);

  // 内容改动后重新决定视图：解析不出来时只有 Raw 能改，视图落回 Raw。
  //
  // 判据只在 JSON 模式跑：`content` 在别的模式里不是这段视图的输入（multipart 的 content 是
  // 几十 MB 的分片表，解析树只是又一次全量扫描）。
  useEffect(() => {
    if (value.mode !== 'json') return;
    if (!parseJsonTree(value.content).ok) {
      setBodyView('raw');
      return;
    }
    if (userPickedBodyView.current) return;
    setBodyView('tree');
  }, [value.content, value.mode]);

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
        </div>
        <p className="text-nano text-muted-foreground">{contentTypeText(value.mode)}</p>
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
            // 换标签时重置整张表的界面状态（行槽位、在途读取）：分片内容随草稿变，行槽位
            // 与读取不该跨标签沿用。
            key={endpointId}
            content={value.content}
            onChange={(content) => onChange({ ...value, content })}
            varEntries={varEntries}
            envName={envName}
            uploadProgress={uploadProgress}
          />
        ) : value.mode === 'binary' ? (
          <BinaryBodyEditor
            content={value.content}
            onChange={(content) => onChange({ ...value, content })}
          />
        ) : value.mode === 'json' ? (
          <JsonBody
            value={value}
            onChange={onChange}
            validationError={validationError}
            onFormatResult={onFormatResult}
            varEntries={varEntries}
            envName={envName}
            view={bodyView}
            onViewChange={(view) => {
              userPickedBodyView.current = true;
              setBodyView(view);
            }}
            onUserPick={() => {
              userPickedBodyView.current = true;
            }}
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
    <EmptyHint className="flex h-32 items-center justify-center text-sm">此请求无 body</EmptyHint>
  );
}

/** 载荷区的两档视图：树只读，Raw 可编辑。 */
type BodyViewMode = 'tree' | 'raw';

/**
 * json 载荷区 —— 树 / Raw 两档。
 *
 * 树只读展示这份载荷的结构，Raw 是可编辑的 CodeMirror，故写回只发生在 Raw。工具条挂在
 * 本容器的右上角，两档共用同一排按钮。
 */
function JsonBody({
  value,
  onChange,
  validationError,
  onFormatResult,
  varEntries,
  envName,
  view,
  onViewChange,
  onUserPick,
}: {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  validationError?: string | null;
  onFormatResult?: (result: FormatResult) => void;
  varEntries: VarEntry[];
  envName?: string;
  view: BodyViewMode;
  onViewChange: (view: BodyViewMode) => void;
  /** 用户动过 Raw 或挑过视图，此后视图不再自动切换。 */
  onUserPick: () => void;
}) {
  // 自动换行默认关闭：树一旦折行，缩进层级就读不出来了。
  const [wrap, setWrap] = useState(false);
  // `busy` 是「剪贴板还没给结论」的那一段：这段时间里按钮要禁用，不然连点几次会排起
  // 一串写入，最后落到哪一个结果上取决于它们完成的先后。
  const [copyState, setCopyState] = useState<'idle' | 'busy' | 'ok' | 'fail'>('idle');
  const flashTimer = useRef<number | null>(null);

  // 卸载时清掉未到期的计时器，否则它会在已卸载的组件上 setState。
  useEffect(
    () => () => {
      if (flashTimer.current !== null) window.clearTimeout(flashTimer.current);
    },
    [],
  );

  /** 先等剪贴板给出结论再点亮按钮：写失败却报成功，用户会以为载荷已经在剪贴板里。 */
  const copyAll = async () => {
    if (copyState === 'busy') return;
    // 上一次的「已复制 / 复制失败」还没到期就重试：先撤掉它的计时器，否则它会在这次
    // 结论之后到点，把刚点亮的按钮又抹回 idle。
    if (flashTimer.current !== null) {
      window.clearTimeout(flashTimer.current);
      flashTimer.current = null;
    }
    setCopyState('busy');
    setCopyState((await copyText(value.content)) ? 'ok' : 'fail');
    flashTimer.current = window.setTimeout(() => setCopyState('idle'), 1500);
  };

  const handleFormat = () => {
    // 带占位符的模板不能格式化：格式化会把文本改写成解析出来的值，而占位符在模板阶段
    // 根本不是 JSON。
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

  const actions: FloatToolbarAction[] = [
    {
      id: 'copy',
      label:
        copyState === 'busy'
          ? '正在复制…'
          : copyState === 'ok'
            ? '已复制'
            : copyState === 'fail'
              ? '复制失败'
              : '复制请求体',
      icon:
        copyState === 'ok' ? (
          <Check className="h-3.5 w-3.5 text-success" />
        ) : (
          <Copy className={cn('h-3.5 w-3.5', copyState === 'fail' && 'text-destructive')} />
        ),
      onClick: copyAll,
      // 剪贴板还没回话：再点一次也只是排队，且可能以旧结论收尾。
      disabled: copyState === 'busy',
      testId: 'copy-body-btn',
    },
    {
      id: 'wrap',
      label: '自动换行',
      activeLabel: '取消自动换行',
      active: wrap,
      icon: <WrapText className="h-3.5 w-3.5" />,
      onClick: () => setWrap((v) => !v),
      testId: 'wrap-body-btn',
    },
  ];
  // 树本身已经是结构化的，格式化只对 Raw 里那段文本有意义。
  if (view === 'raw') {
    actions.push({
      id: 'format',
      label: '格式化',
      icon: <Wand2 className="h-3.5 w-3.5" />,
      onClick: handleFormat,
      disabled: !value.content.trim(),
      testId: 'format-body-btn',
    });
  }

  // 树能不能渲染，判据必须与真正建树的那一个解析器同源。只调 `JSON.parse` 的话，
  // `parseJsonTree` 拒掉的文本（比如结尾多一个字符）会先被判成合法，树于是空白一片。
  // 一份文本只解析一次：`valid` 与树根都从这个结果取，解析开销落在内容变化那一刻。
  const parsed = useMemo(() => parseJsonTree(value.content), [value.content]);
  const valid = parsed.ok;
  const treeRoot = parsed.ok ? parsed.root : null;

  return (
    <div className="relative flex min-h-0 flex-col gap-1.5">
      <FloatToolbar actions={actions} />
      {/* 这一行右端空着，工具条浮在那里；行高与工具条齐平，内容不会被它压住。 */}
      <div className="flex h-10 items-center">
        <Tabs
          variant="segmented"
          items={[
            { id: 'tree', label: '树' },
            { id: 'raw', label: 'Raw' },
          ]}
          value={view}
          onChange={(id) => onViewChange(id as BodyViewMode)}
        />
      </div>
      {/* 校验徽标住在 Raw 的编辑器顶部，树这一档看不到它。这里补一条同源的行内提示，
          让「发送被拦下」的原因在只读视图里也读得到。 */}
      {view === 'tree' && validationError && (
        <div
          role="status"
          aria-live="polite"
          data-testid="body-validation-inline"
          title={`JSON 格式有误：${validationError}`}
          className="flex items-start gap-1.5 rounded-md border border-destructive bg-destructive px-2.5 py-1 text-micro text-destructive-foreground"
        >
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" aria-hidden />
          <span className="truncate font-mono">JSON 格式有误：{validationError}</span>
        </div>
      )}
      {/*
        两档**都保持挂载**，靠 `display: none` 切显隐。窗口那一侧一卸载，CodeMirror 的
        撤销栈连同编辑器状态一起没了 —— 在 Raw 里把 `{"a":1}` 改成 `{"a":2}`，切到树里
        看一眼再回来，Ctrl+Z 就回不到 `1`。`display: none` 同时把不可见的一侧移出无障碍
        树与 Tab 序。（这里写 `hidden` 类而不是 `hidden` 属性：`flex` 这类 display 工具类
        属于作者样式表，按层叠规则会盖掉浏览器样式表里的 `[hidden] { display: none }`。）
      */}
      <div className={cn('min-h-0 flex-1 flex-col', view === 'tree' ? 'flex' : 'hidden')}>
        {valid ? (
          <JsonTreeView root={treeRoot} wrap={wrap} active={view === 'tree'} />
        ) : (
          <EmptyHint className="flex h-32 items-center justify-center text-sm">
            {value.content.trim() ? 'JSON 格式有误，切到 Raw 修改' : '还没有内容，切到 Raw 输入'}
          </EmptyHint>
        )}
      </div>
      <div className={cn('min-h-0 flex-1 flex-col', view === 'raw' ? 'flex' : 'hidden')}>
        <BodyContent
          value={value}
          onChange={(body) => {
            onUserPick();
            onChange(body);
          }}
          validationError={validationError}
          varEntries={varEntries}
          envName={envName}
          wrap={wrap}
        />
      </div>
    </div>
  );
}

/**
 * 载荷编辑器 —— 左栏那个 CodeMirror。变量候选提示接在编辑器里：敲 `{{` 时浮出候选，
 * 选中即把 `{{key}}` 写进文档。编辑器不是普通输入框，故写回与键盘导航在这里自己实现。
 */
function BodyContent({
  value,
  onChange,
  validationError,
  varEntries,
  envName,
  wrap = false,
}: {
  value: RequestBody;
  onChange: (body: RequestBody) => void;
  validationError?: string | null;
  varEntries: VarEntry[];
  envName?: string;
  /** 长行是否折行；由扩展实现，编辑器据此重算行高。 */
  wrap?: boolean;
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

  // 用户已经看不到 body 时还钉着错误，徽标就指错了位置。凡是离开 json 编辑器就作废它，
  // 下次 发送 / 保存 会重新检查此刻的内容。
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
      // 换行必须走扩展而不是 CSS：编辑器按扩展重算行高，样式表里的 `white-space` 与它
      // 的测量对不上。
      ...(wrap ? [EditorView.lineWrapping] : []),
      hintKeys,
    ],
    [value.mode, wrap, hintKeys],
  );

  // 浮层按光标位置定位：编辑器里没有「光标所在的那个输入框」，故按光标位置算出矩形。
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
        徽标浮在编辑器顶部：280 px 的 CodeMirror 会挡住用户和徽标，错误就没人看见。
        徽标常驻不自动消失也不手动关，只在校验通过后离场。
      */}
      {hintText && (
        <div
          ref={pillRef}
          role="status"
          aria-live="polite"
          className={cn(
            'absolute right-2 top-2 z-10 flex max-w-[min(420px,60%)] items-center gap-1.5',
            'rounded-full border px-2.5 py-1 text-micro shadow-md',
            // 实心 destructive，不用浅色底：`--destructive-foreground` 两个主题都是近白，
            // 配 `bg-background/85` 那种浅底就是白字白底。
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
 * Schema 编辑区初次打开时显示哪个视图。文本能解析成结构且非空就落在「结构」视图，
 * 解析不出来或还没写就落在「JSON」视图。
 */
function initialSchemaView(text: string): SchemaViewMode {
  const parsed = parseSchemaView(text);
  return parsed.ok && text.trim() ? 'structure' : 'json';
}

/**
 * 请求体的 Schema 编辑区 —— 文档里那份「参数长什么样」的定义。
 *
 * - 与载荷编辑器分开：左栏是这一次发什么，本区是这个接口接受什么。
 * - 不参与发包（导出 OpenAPI 时才读它），写坏了不阻止发送与保存。
 * - 常驻不折叠：模式切成 none 之后 body 里的 schema 仍在，藏起来用户就删不掉它。
 * - 「结构」与「JSON」两个视图读写同一份文本（`value.schema`），故它们不会各说各话。
 */
function SchemaSection({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const isDark = useIsDark();
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<SchemaViewMode>(() => initialSchemaView(value));
  // 用户是否自己选过视图。用户选过之后，视图不再由文本的解析状态决定，否则用户正在
  // JSON 视图里改字，文本一改合法就被拽去结构视图，编辑器会从眼皮底下消失。
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
        <span className="text-nano font-normal text-muted-foreground">
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
        <span className="text-nano text-muted-foreground">
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
                // 用户在这个视图里改字，视图就以这里为准。
                userPickedView.current = true;
                // 改对了就消掉上一轮的结论；还没改对时保留它，直到下一次离开编辑器。
                const next = parseError(v);
                if (!next) setError(null);
              }}
              // 离开编辑器时判一次合法性。逐键判会让编辑到一半的 JSON 一直亮着错误。
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
                'rounded-full border px-2.5 py-1 text-micro shadow-md',
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
