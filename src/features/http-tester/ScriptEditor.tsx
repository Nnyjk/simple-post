import CodeMirror from '@uiw/react-codemirror';
import { javascript } from '@codemirror/lang-javascript';
import { oneDark } from '@codemirror/theme-one-dark';
import { useIsDark } from '@/lib/theme';
import { lightCodeTheme } from '@/lib/codemirror-themes';

interface Props {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
}

/**
 * 一段请求脚本的编辑器。
 *
 * 脚本在**后端**的沙箱里跑（嵌入式 QuickJS，宿主能力只有 `pm` / `console` / `expect`），
 * 故这里只管文本的高亮与编辑，不做任何本地求值——本地跑一遍与沙箱里的行为必然对不上，
 * 那种「预览」会骗人。
 */
export function ScriptEditor({ value, onChange, placeholder }: Props) {
  const isDark = useIsDark();
  return (
    <div className="overflow-hidden rounded-md border border-border">
      <CodeMirror
        value={value}
        onChange={onChange}
        extensions={[javascript()]}
        theme={isDark ? oneDark : lightCodeTheme}
        basicSetup={{
          lineNumbers: true,
          foldGutter: true,
          highlightActiveLine: true,
          highlightActiveLineGutter: true,
        }}
        placeholder={placeholder}
        height="240px"
        className="text-sm"
      />
    </div>
  );
}
