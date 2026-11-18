import { useEffect, useRef, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { EditorView } from '@codemirror/view';
import { oneDark } from '@codemirror/theme-one-dark';
import { Check, Copy, Wand2, WrapText } from 'lucide-react';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';
import { cn, type FormatResult } from '@/lib/utils';
import { FloatToolbar, type FloatToolbarAction } from '@/components/ui/float-toolbar';
import { JsonTreeView } from '@/components/ui/json-tree-view';

/**
 * 响应体的 Pretty / Raw 两页。
 *
 * Pretty 页把合法 JSON 渲染成可折叠的键值树；Raw 页渲染格式化文本，并保留「原文 / 格式化」
 * 开关。响应不是合法 JSON 时两页都退回原文，并在上方给出同一条提示。
 *
 * 非 JSON 的文本响应也走这里：它两页都只有原文，但工具条照常浮在右上角。
 */
export function PrettyView({
  body,
  view,
  formatResult,
  expectJson = true,
  wrap,
  onToggleWrap,
  formatted,
  onToggleFormatted,
}: {
  body: string;
  view: 'pretty' | 'raw';
  formatResult: FormatResult;
  /** 响应按 Content-Type 是不是本该是 JSON；为 false 时不给「不是合法 JSON」的提示。 */
  expectJson?: boolean;
  /** 长行是否折行。与 `formatted` 一样由宿主持有：两个页签是两棵独立的树。 */
  wrap: boolean;
  onToggleWrap: () => void;
  /** Raw 页的「原文 / 格式化」档位；只在 `view === 'raw'` 时可用。 */
  formatted: boolean;
  onToggleFormatted: () => void;
}) {
  // 本该是 JSON 且确实解析成功，才谈得上树与格式化；纯文本响应即便恰好能解析也不当 JSON 画。
  const asJson = expectJson && formatResult.ok;
  const isDark = useIsDark();

  // 上面这两个开关都不放在这里。`ResponseViewer` 的 pretty 分支与 raw 分支各渲染一个
  // `PrettyView`，切换页签会卸载这一个、挂载那一个 —— 状态若住在实例里，用户在 Raw 里
  // 打开的换行一到 Pretty 就没了，而那个开关读起来是「这份响应体怎么读」，不是「这个页签
  // 怎么读」。故状态由宿主持有，这里只负责渲染与上报。同理，新响应到达时由宿主重置。

  const [copied, setCopied] = useState(false);
  const flashTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (flashTimer.current !== null) window.clearTimeout(flashTimer.current);
    },
    [],
  );

  const copyAll = () => {
    void navigator.clipboard?.writeText(body);
    setCopied(true);
    if (flashTimer.current !== null) window.clearTimeout(flashTimer.current);
    flashTimer.current = window.setTimeout(() => setCopied(false), 1500);
  };

  // Raw 页用 `formatResult` 的格式化文本；Pretty 页的树自己从原文解析，这里不再解析一遍。
  const displayed = formatted && asJson ? formatResult.formatted : body;

  const actions: FloatToolbarAction[] = [
    {
      id: 'copy',
      label: copied ? '已复制' : '复制响应体',
      icon: copied ? (
        <Check className="h-3.5 w-3.5 text-success" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      ),
      onClick: copyAll,
      testId: 'copy-response-body-btn',
    },
    {
      id: 'wrap',
      label: '自动换行',
      activeLabel: '取消自动换行',
      active: wrap,
      icon: <WrapText className="h-3.5 w-3.5" />,
      onClick: onToggleWrap,
      testId: 'wrap-response-btn',
    },
  ];
  if (view === 'raw' && asJson) {
    actions.push({
      id: 'format',
      label: formatted ? '显示原文' : '格式化',
      icon: <Wand2 className="h-3.5 w-3.5" />,
      onClick: onToggleFormatted,
      testId: 'raw-format-toggle',
    });
  }

  return (
    // `pt-11` 是工具条实际占的高度（`top-2` 8 + 边框加 `p-0.5` 与 `h-7` 共 34）：整块内容
    // 从它下沿开始排，否则不换行的长行会从按钮底下穿过去。四个分支共用这一处，不再各写一份。
    <div className="relative flex h-full min-h-0 flex-col pt-11">
      <FloatToolbar actions={actions} />
      {!asJson ? (
        <>
          {expectJson && <div className="alert-warning-inline">响应不是合法 JSON，仅展示原文</div>}
          <pre
            className={cn(
              'm-0 min-h-0 flex-1 overflow-auto p-4 font-mono text-xs leading-relaxed text-foreground/90',
              wrap ? 'whitespace-pre-wrap break-all' : 'whitespace-pre',
            )}
          >
            {body}
          </pre>
        </>
      ) : view === 'pretty' ? (
        <JsonTreeView text={body} wrap={wrap} />
      ) : (
        <div className="min-h-0 flex-1">
          <CodeMirror
            value={displayed}
            // 换行靠 `EditorView.lineWrapping` 这个扩展，不是 CSS —— CodeMirror 按扩展
            // 重算行高，样式表里改 `white-space` 会让测量与渲染对不上。
            extensions={wrap ? [json(), EditorView.lineWrapping] : [json()]}
            theme={isDark ? oneDark : lightCodeTheme}
            // 用 `readOnly` 而不是 `editable={false}`：后者去掉 content DOM 的 contenteditable，
            // 编辑器不再可聚焦 —— CodeMirror 自绘的光标只在聚焦时出现，原生光标又被它的
            // `caret-color: transparent` 关掉。`readOnly` 拦编辑：输入、粘贴、拖放、剪切全被拒。
            readOnly
            basicSetup={{
              // `history` 关掉：`@uiw/react-codemirror` 同步 `value` 时整篇替换，那笔改动会进
              // 撤销栈；而 `readOnly` 只拦 `undo`、不拦 `undoSelection`，Ctrl+U 会把响应体
              // 退回上一份，与 store 里那一份对不上。
              history: false,
              lineNumbers: true,
              foldGutter: true,
              highlightActiveLine: false,
            }}
            height="100%"
            className="text-xs"
          />
        </div>
      )}
    </div>
  );
}
