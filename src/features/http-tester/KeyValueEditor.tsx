import { Plus, Trash2 } from 'lucide-react';
import type { KeyValue } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { uid } from '@/lib/utils';
import { VarHintInput, type VarEntry } from './var-hint';

/**
 * 键列 `<datalist>` 中的一条建议。编辑器不直接引入 `CommonHeader`，好让调用方传入任何
 * 兼容的形状（Params 页不传，Headers 页传共享的 `COMMON_HEADERS`）。
 */
export interface KeySuggestion {
  name: string;
  /** 一行提示；在会展示它的浏览器（Chrome / Firefox）里渲染为选项的 `title`，同时作为
   *  选项的文本回退。 */
  description?: string;
}

interface Props {
  items: KeyValue[];
  onChange: (items: KeyValue[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  /**
   * 可选的自动补全列表。传入且非空时，编辑器渲染**一个** `<datalist>`（所有行共用同一
   * id），浏览器会把它接到每个键输入框上。留空或不传 = 纯自由文本输入。
   */
  keySuggestions?: ReadonlyArray<KeySuggestion>;
  /**
   * 变量候选：环境变量与动态变量并进同一份列表。
   *
   * 传了才给值栏接候选提示。OAuth2 的额外参数不会上线替换，故那处调用不传这个 prop，
   * 值栏仍是普通输入框。
   */
  varEntries?: VarEntry[];
  /** 当前环境名，显示在候选浮层的标题上。 */
  envName?: string;
}

// 单一共享 id —— `<datalist>` 的关联靠文档级 id，故无论行数多少都只需输出一个列表元素。
const SUGGESTIONS_DATALIST_ID = 'kve-key-suggestions';

export function KeyValueEditor({
  items,
  onChange,
  keyPlaceholder = 'key',
  valuePlaceholder = 'value',
  keySuggestions,
  varEntries,
  envName,
}: Props) {
  // 改一行：整表里按 id 换掉那一行，其余行原样留下。
  const update = (id: string, patch: Partial<KeyValue>) => {
    onChange(items.map((it) => (it.id === id ? { ...it, ...patch } : it)));
  };
  const add = () =>
    onChange([...items, { id: uid(), key: '', value: '', enabled: true, description: '' }]);

  const hasSuggestions = !!keySuggestions && keySuggestions.length > 0;

  return (
    <div className="px-4 py-3">
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="text-[10px] uppercase tracking-wider text-muted-foreground">
            <th className="w-8 px-1 pb-1.5 text-left font-medium"></th>
            <th className="w-[35%] px-2 pb-1.5 text-left font-medium">Key</th>
            <th className="w-[45%] px-2 pb-1.5 text-left font-medium">Value</th>
            <th className="px-2 pb-1.5 text-left font-medium">说明</th>
            <th className="w-8 px-1"></th>
          </tr>
        </thead>
        <tbody>
          {items.map((it) => (
            <KeyValueRow
              key={it.id}
              item={it}
              items={items}
              onChange={onChange}
              onUpdate={update}
              varEntries={varEntries}
              envName={envName}
              keyPlaceholder={keyPlaceholder}
              valuePlaceholder={valuePlaceholder}
              hasSuggestions={hasSuggestions}
            />
          ))}
        </tbody>
      </table>
      <Button variant="outline" size="sm" onClick={add} className="mt-2 gap-1.5 text-xs">
        <Plus className="h-3.5 w-3.5" />
        添加
      </Button>

      {/* datalist 放在同一容器的底部；任何输入框用 `list=` 引用它时，浏览器按 id 找到它。
          每条的 description 既作为选项的 title（Chrome / Firefox 的 tooltip），也作为不渲染
          <datalist> 弹层的浏览器的文本回退。 */}
      {hasSuggestions && (
        <datalist id={SUGGESTIONS_DATALIST_ID}>
          {keySuggestions!.map((s) => (
            <option key={s.name} value={s.name} title={s.description ?? ''}>
              {s.description ?? ''}
            </option>
          ))}
        </datalist>
      )}
    </div>
  );
}

/**
 * 键值表的一行。
 *
 * 值栏带变量候选提示，而提示的状态（浮层开不开、选中了哪一条）是**每行一份**，故整行
 * 抽成独立组件：提示的 hook 写在 `map` 回调里会跟着每次渲染重新挂载。
 */
function KeyValueRow({
  item,
  items,
  onChange,
  onUpdate,
  varEntries,
  envName,
  keyPlaceholder,
  valuePlaceholder,
  hasSuggestions,
}: {
  /** 这一行的数据。 */
  item: KeyValue;
  /** 整表的数据 —— 删行时按 id 过滤出新的表。 */
  items: KeyValue[];
  /** 写回整表。 */
  onChange: (items: KeyValue[]) => void;
  /** 改一行里的某个字段。 */
  onUpdate: (id: string, patch: Partial<KeyValue>) => void;
  varEntries?: VarEntry[];
  envName?: string;
  keyPlaceholder: string;
  valuePlaceholder: string;
  hasSuggestions: boolean;
}) {
  // 删这一行：整表按 id 过滤出新的表。
  const remove = () => onChange(items.filter((it) => it.id !== item.id));

  return (
    <tr className="group">
      <td className="py-1">
        <input
          type="checkbox"
          checked={item.enabled}
          onChange={(e) => onUpdate(item.id, { enabled: e.target.checked })}
          className="h-3.5 w-3.5 cursor-pointer accent-primary"
        />
      </td>
      <td className="px-1 py-1">
        <Input
          value={item.key}
          onChange={(e) => onUpdate(item.id, { key: e.target.value })}
          placeholder={keyPlaceholder}
          list={hasSuggestions ? SUGGESTIONS_DATALIST_ID : undefined}
          autoComplete="off"
          className="h-7 font-mono text-xs"
        />
      </td>
      <td className="px-1 py-1">
        {varEntries ? (
          <VarHintInput
            value={item.value}
            onChange={(next) => onUpdate(item.id, { value: next })}
            entries={varEntries}
            envName={envName}
            placeholder={valuePlaceholder}
            className="h-7 font-mono text-xs"
          />
        ) : (
          <Input
            value={item.value}
            onChange={(e) => onUpdate(item.id, { value: e.target.value })}
            placeholder={valuePlaceholder}
            className="h-7 font-mono text-xs"
          />
        )}
      </td>
      <td className="px-1 py-1">
        <Input
          value={item.description ?? ''}
          onChange={(e) => onUpdate(item.id, { description: e.target.value })}
          placeholder="可选"
          className="h-7 text-xs"
        />
      </td>
      <td className="px-1 py-1 text-right">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 opacity-0 transition-opacity group-hover:opacity-100"
          onClick={remove}
        >
          <Trash2 className="h-3 w-3 text-muted-foreground" />
        </Button>
      </td>
    </tr>
  );
}
