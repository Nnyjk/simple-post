import CodeMirror, { EditorState, type Extension } from '@uiw/react-codemirror';
import { json } from '@codemirror/lang-json';
import { oneDark } from '@codemirror/theme-one-dark';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';

/** 补全上下文：这里只用到文档文本与光标位置。 */
export interface GraphQlCompletionContext {
  pos: number;
  state: { doc: { toString(): string } };
}

export interface GraphQlCompletionOption {
  label: string;
  /** 候选的类型或参数名，显示在候选项右侧。 */
  detail?: string;
  type?: string;
}

export interface GraphQlCompletionResult {
  /** 候选替换的起点：光标前已经打出的半截名字要被它吃掉。 */
  from: number;
  options: GraphQlCompletionOption[];
}

export type GraphQlCompletionSource = (
  context: GraphQlCompletionContext,
) => GraphQlCompletionResult | null;

interface Props {
  value: string;
  onChange: (value: string) => void;
  /** `json` 只用于 Variables；GraphQL 查询本身没有语言包，按纯文本编辑。 */
  language: 'graphql' | 'json';
  height: string;
  placeholder?: string;
  /** 查询编辑器的补全候选来源；Variables 编辑器不传。 */
  completion?: GraphQlCompletionSource;
}

/**
 * GraphQL 查询 / variables 的编辑器。
 *
 * 查询不做语法检查：GraphQL 的校验在服务端，客户端只有用户拉来的 schema，故只按它给补全
 * 候选，不判断文档对不对。variables 用 JSON 高亮，解析结果由 `GraphQlPanel` 就地提示。
 *
 * 补全走 CodeMirror 的语言数据通道（`languageData` 的 `autocomplete` 条目）：查询没有语言
 * 包，挂不上 `language.data`，而自动补全扩展本身已由 basicSetup 装好，故只需在这里交出来源。
 */
export function GraphQlEditor({
  value,
  onChange,
  language,
  height,
  placeholder,
  completion,
}: Props) {
  const isDark = useIsDark();
  // 只在查询编辑器上挂补全：Variables 的 JSON 补全不在这个来源里。
  const completionExtension: Extension[] =
    language === 'graphql' && completion
      ? [EditorState.languageData.of(() => [{ autocomplete: completion }])]
      : [];
  return (
    <div className="overflow-hidden rounded-md border border-border">
      <CodeMirror
        value={value}
        onChange={onChange}
        extensions={language === 'json' ? [json()] : completionExtension}
        theme={isDark ? oneDark : lightCodeTheme}
        basicSetup={{
          lineNumbers: true,
          foldGutter: true,
          highlightActiveLine: true,
          highlightActiveLineGutter: true,
        }}
        placeholder={placeholder}
        height={height}
        className="text-sm"
      />
    </div>
  );
}
