import { Plus, Trash2 } from 'lucide-react';
import type { KeyValue } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { uid } from '@/lib/utils';

interface Props {
  items: KeyValue[];
  onChange: (items: KeyValue[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
}

export function KeyValueEditor({
  items,
  onChange,
  keyPlaceholder = 'key',
  valuePlaceholder = 'value',
}: Props) {
  const update = (id: string, patch: Partial<KeyValue>) => {
    onChange(items.map((it) => (it.id === id ? { ...it, ...patch } : it)));
  };
  const remove = (id: string) => onChange(items.filter((it) => it.id !== id));
  const add = () =>
    onChange([...items, { id: uid(), key: '', value: '', enabled: true, description: '' }]);

  return (
    <div className="px-4 py-3">
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="text-[10px] uppercase tracking-wider text-muted-foreground">
            <th className="w-8 px-1 pb-1.5 text-left font-medium"></th>
            <th className="w-[35%] px-2 pb-1.5 text-left font-medium">Key</th>
            <th className="w-[45%] px-2 pb-1.5 text-left font-medium">Value</th>
            <th className="px-2 pb-1.5 text-left font-medium">说明</th>
            <th className="w-8 px-1 pb-1.5"></th>
          </tr>
        </thead>
        <tbody>
          {items.map((it) => (
            <tr key={it.id} className="group">
              <td className="py-1">
                <input
                  type="checkbox"
                  checked={it.enabled}
                  onChange={(e) => update(it.id, { enabled: e.target.checked })}
                  className="h-3.5 w-3.5 cursor-pointer accent-primary"
                />
              </td>
              <td className="px-1 py-1">
                <Input
                  value={it.key}
                  onChange={(e) => update(it.id, { key: e.target.value })}
                  placeholder={keyPlaceholder}
                  className="h-7 font-mono text-xs"
                />
              </td>
              <td className="px-1 py-1">
                <Input
                  value={it.value}
                  onChange={(e) => update(it.id, { value: e.target.value })}
                  placeholder={valuePlaceholder}
                  className="h-7 font-mono text-xs"
                />
              </td>
              <td className="px-1 py-1">
                <Input
                  value={it.description ?? ''}
                  onChange={(e) => update(it.id, { description: e.target.value })}
                  placeholder="可选"
                  className="h-7 text-xs"
                />
              </td>
              <td className="px-1 py-1 text-right">
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6 opacity-0 transition-opacity group-hover:opacity-100"
                  onClick={() => remove(it.id)}
                >
                  <Trash2 className="h-3 w-3 text-muted-foreground" />
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <Button variant="outline" size="sm" onClick={add} className="mt-2 gap-1.5 text-xs">
        <Plus className="h-3.5 w-3.5" />
        添加
      </Button>
    </div>
  );
}
