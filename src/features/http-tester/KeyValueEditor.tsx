import { Plus, Trash2 } from 'lucide-react';
import type { KeyValue } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { uid } from '@/lib/utils';

/**
 * A single suggestion shown in the key column's `<datalist>`. The
 * editor doesn't import `CommonHeader` directly so callers can pass
 * any compatible shape (Params tab passes nothing, Headers tab passes
 * the shared `COMMON_HEADERS`).
 */
export interface KeySuggestion {
  name: string;
  /** One-line hint; rendered as the option's `title` for browsers that
   *  surface it (Chrome / Firefox) and as the option's text fallback. */
  description?: string;
}

interface Props {
  items: KeyValue[];
  onChange: (items: KeyValue[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  /**
   * Optional autocomplete list. When provided AND non-empty, the
   * editor renders a single `<datalist>` (id shared across all rows)
   * that the browser wires up to every key input. Empty / omitted =
   * plain free-text input.
   */
  keySuggestions?: ReadonlyArray<KeySuggestion>;
}

// Single shared id — `<datalist>` association is by document-wide id,
// so we only need to emit one list element regardless of row count.
const SUGGESTIONS_DATALIST_ID = 'kve-key-suggestions';

export function KeyValueEditor({
  items,
  onChange,
  keyPlaceholder = 'key',
  valuePlaceholder = 'value',
  keySuggestions,
}: Props) {
  const update = (id: string, patch: Partial<KeyValue>) => {
    onChange(items.map((it) => (it.id === id ? { ...it, ...patch } : it)));
  };
  const remove = (id: string) => onChange(items.filter((it) => it.id !== id));
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
                  list={hasSuggestions ? SUGGESTIONS_DATALIST_ID : undefined}
                  autoComplete="off"
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

      {/* The datalist lives at the bottom of the same container; the
          browser locates it by id when any input references it via
          `list=`. Each row's description shows up as the option's
          title (Chrome / Firefox tooltip) and as the option's text
          fallback for browsers that don't render <datalist> popups. */}
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