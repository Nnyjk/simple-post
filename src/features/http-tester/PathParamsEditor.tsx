import { Trash2 } from 'lucide-react';
import { useMemo } from 'react';
import type { KeyValue } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { applyPathParamValue, pathPlaceholders } from '@/lib/path-params';

/**
 * 路径参数的编辑器 —— URL 里那些 `{占位符}` 各自的值。
 *
 * ## 界面为什么按 URL 派生，而不是一张自由的键值表
 *
 * 占位符的名字写在 URL 里（`/users/{id}` 的 `id`），值写在这里。编辑器因此把
 * URL 当作**名单的来源**：URL 里有几个占位符，界面就列几行；用户改 URL，这几行跟着
 * 变。反过来做（先列一张表，让用户自己写名字）的话，URL 与表里的名字对不上时，
 * 请求会带着字面量 `{id}` 发出去，而界面看上去一切正常。
 *
 * ## 存进草稿的是什么
 *
 * 只有**填过值**的占位符才进存储。改 URL 本身不该把标签弄脏 —— 弄脏标签的是
 * 「我改了请求的内容」，而改 URL 里的占位符名字并没有改任何请求内容。
 *
 * 用户第一次给某个占位符填值时，存储里还没有它 —— 那一行是**新建**出来的条目，
 * 不是去改一条已有的记录。清空值时条目被删掉，回到「没填」的状态。
 *
 * 存储里那些在当前 URL 里已经找不到对应占位符的条目**保留着**（不静默丢弃），
 * 并在界面上单独列出来：用户改过占位符的名字，值还在，只是暂时没有对应的位置。
 */
export function PathParamsEditor({
  url,
  items,
  error,
  onChange,
}: {
  /** 当前生效的 URL 文本。占位符名单从它派生。 */
  url: string;
  /** 已存的路径参数（占位符名 → 值）。 */
  items: KeyValue[];
  /** 发送被拦下时的原因。`null` 表示没有拦下。 */
  error?: string | null;
  onChange: (items: KeyValue[]) => void;
}) {
  const placeholders = useMemo(() => pathPlaceholders(url), [url]);

  /** URL 里已经找不到对应占位符的存有条目。 */
  const orphans = useMemo(
    () => items.filter((it) => !placeholders.includes(it.key)),
    [items, placeholders],
  );

  /** 那一行在存储里的条目（没填过的占位符没有条目）。 */
  const itemOf = (name: string): KeyValue | undefined => items.find((it) => it.key === name);

  /**
   * 写一个占位符的值。
   *
   * 重排与「只改被改的那一个」这两条规则住在 `lib/path-params` 的
   * [`applyPathParamValue`] 里，纯函数才有地方钉测试。
   */
  const setValue = (name: string, value: string) => {
    onChange(applyPathParamValue(items, placeholders, name, value));
  };

  const removeOrphan = (id: string) => {
    onChange(items.filter((it) => it.id !== id));
  };

  // 发送被拦下的原因。空状态里也要出现：占位符可能只存在于前置 URL 里，这时
  // 本页一行都没有，不显示这句话，用户就不知道该去哪里改。
  const errorBanner = error ? (
    <div
      role="status"
      aria-live="polite"
      data-testid="path-params-error"
      className="mb-2 rounded-md border border-destructive bg-destructive px-2.5 py-1.5 text-[11px] text-destructive-foreground"
    >
      {error}
    </div>
  ) : null;

  if (placeholders.length === 0 && items.length === 0) {
    return (
      <div className="px-4 py-3">
        {errorBanner}
        <div className="flex h-24 items-center justify-center rounded-md border border-dashed border-border text-sm text-muted-foreground">
          URL 里没有 {'{占位符}'}，因此没有路径参数
        </div>
        <p className="mt-2 text-[11px] text-muted-foreground">
          把地址写成 /users/{'{id}'} 这样的形式，占位符就会出现在这里。
        </p>
      </div>
    );
  }

  return (
    <div className="px-4 py-3">
      {errorBanner}

      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="text-[10px] uppercase tracking-wider text-muted-foreground">
            <th className="w-[30%] px-2 pb-1.5 text-left font-medium">占位符</th>
            <th className="w-[45%] px-2 pb-1.5 text-left font-medium">值</th>
            <th className="px-2 pb-1.5 text-left font-medium">说明</th>
          </tr>
        </thead>
        <tbody>
          {placeholders.map((name) => {
            const item = itemOf(name);
            const value = item?.value ?? '';
            // 缺值有两种：根本没填，或填了但那一条是停用的。`prepareRequest` 只认
            // 启用的那一条，停用的条目即便有值也不算填了。
            const missing = value.trim() === '';
            const disabled = !missing && item?.enabled === false;
            return (
              <tr key={name}>
                <td className="px-2 py-1 font-mono text-xs text-muted-foreground">
                  {'{'}
                  {name}
                  {'}'}
                </td>
                <td className="px-1 py-1">
                  <Input
                    value={value}
                    onChange={(e) => setValue(name, e.target.value)}
                    placeholder="值（支持 {{var}}）"
                    className="h-7 font-mono text-xs"
                    data-testid={`path-param-${name}`}
                  />
                </td>
                <td className="px-2 py-1 text-[11px]">
                  {missing ? (
                    <span className="text-destructive">必填，填了才能发送</span>
                  ) : disabled ? (
                    <span className="text-muted-foreground">
                      已停用，不会进入请求；改一下这个值就会启用它
                    </span>
                  ) : (
                    <span className="text-muted-foreground">已填写</span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      {orphans.length > 0 && (
        <div className="mt-4">
          <p className="mb-1 text-[11px] text-muted-foreground">
            下面这些路径参数在当前 URL 里没有对应的占位符，它们不会进入请求。
          </p>
          <table className="w-full table-fixed border-collapse text-sm">
            <tbody>
              {orphans.map((it) => (
                <tr key={it.id} className="group">
                  <td className="w-[30%] px-2 py-1 font-mono text-xs text-muted-foreground">
                    {it.key}
                  </td>
                  <td className="w-[45%] px-1 py-1">
                    <Input
                      value={it.value}
                      onChange={(e) =>
                        onChange(
                          items.map((x) => (x.id === it.id ? { ...x, value: e.target.value } : x)),
                        )
                      }
                      placeholder="值"
                      className="h-7 font-mono text-xs"
                    />
                  </td>
                  <td className="px-2 py-1 text-[11px] text-muted-foreground">未被 URL 使用</td>
                  <td className="w-8 px-1 py-1 text-right">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 opacity-0 transition-opacity group-hover:opacity-100"
                      onClick={() => removeOrphan(it.id)}
                    >
                      <Trash2 className="h-3 w-3 text-muted-foreground" />
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
