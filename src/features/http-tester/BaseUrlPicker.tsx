/**
 * BaseUrlPicker — the dropdown that sits between the method picker and
 * the URL field in HttpTester.
 *
 * The picker drives a project-level *baseUrl definition* (named slot
 * like "鉴权服务" / "数据服务") plus an optional per-endpoint override.
 * The actual URL is looked up from `env.baseUrls[defId]` at resolve time
 * (see lib/url.ts), so flipping the active env changes the URL
 * automatically without touching the endpoint's selection.
 *
 * The component is fully controlled. The parent owns:
 *   - `selection`  — which named slot is active (or "default" / "override")
 *   - `customValue` — text inside the "自定义" inline input
 *
 * The parent also passes the resolved baseUrl for the trigger label;
 * we don't compute it here to keep the picker dumb.
 *
 * Visual style: card-style trigger (rounded-lg + accent icon container),
 * SettingRow-style dropdown rows (icon container + label + sub-label +
 * trailing badge), matching the new Settings category aesthetic.
 *
 * LOCATION: this lives in `features/http-tester/` rather than
 * `components/ui/` because it is not a primitive — it imports the
 * `BaseUrlDefinition` domain type, hard-codes "which named slot is
 * active" semantics, and has exactly one consumer (HttpTester). Its
 * three former siblings in `components/ui/` are the genuine primitives
 * it composes.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Globe, Pencil } from 'lucide-react';
import { cn, focusRing } from '@/lib/utils';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { IconTile } from '@/components/ui/icon-tile';
import type { HueConcept } from '@/lib/hue';
import type { BaseUrlDefinition } from '@/types/domain';

/**
 * What the trigger is currently showing. The `default` variant means
 * "follow the project's default def"; `definition` is an explicit
 * named slot; `override` is the per-endpoint custom URL.
 */
export type BaseUrlSelection =
  { kind: 'default' } | { kind: 'definition'; definitionId: string } | { kind: 'override' };

export interface BaseUrlPickerProps {
  /** The project's baseUrl definitions — drives the dropdown rows. */
  definitions: readonly BaseUrlDefinition[];
  /** The project's default def id (may be null). */
  defaultDefinitionId: string | null;
  /**
   * The currently resolved baseUrl. Used only for the trigger's
   * secondary label (e.g. "· api.demo.dev") so the user can confirm
   * the actual host at a glance.
   */
  resolvedBaseUrl: string;
  selection: BaseUrlSelection;
  customValue: string;
  onSelectionChange: (next: BaseUrlSelection) => void;
  onCustomValueChange: (next: string) => void;
  className?: string;
}

const TRIGGER_WIDTH = 'w-52';
const POPOVER_WIDTH = 'w-80';

export function BaseUrlPicker({
  definitions,
  defaultDefinitionId,
  resolvedBaseUrl,
  selection,
  customValue,
  onSelectionChange,
  onCustomValueChange,
  className,
}: BaseUrlPickerProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  // Close on outside click — mousedown so a click that lands on a row
  // is detected by the row first.
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      setOpen(false);
    };
    document.addEventListener('mousedown', onMouseDown);
    return () => document.removeEventListener('mousedown', onMouseDown);
  }, [open]);

  // Esc closes.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        setOpen(false);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [open]);

  // Default-row label: "项目默认" when the project actually has a
  // default def, otherwise "项目默认（无）" to make the empty case
  // obvious. The custom row always shows "自定义".
  const defaultDef = useMemo(
    () => definitions.find((d) => d.id === defaultDefinitionId) ?? null,
    [definitions, defaultDefinitionId],
  );
  const selectedDef = useMemo(() => {
    if (selection.kind !== 'definition') return null;
    return definitions.find((d) => d.id === selection.definitionId) ?? null;
  }, [selection, definitions]);

  const triggerLabel = useMemo(() => {
    if (selection.kind === 'definition' && selectedDef) return selectedDef.name;
    if (selection.kind === 'override') return '自定义';
    return defaultDef ? '项目默认' : '项目默认（无）';
  }, [selection, selectedDef, defaultDef]);

  const triggerSubLabel = useMemo(() => {
    if (!resolvedBaseUrl) return '';
    return `· ${hostOf(resolvedBaseUrl)}`;
  }, [resolvedBaseUrl]);

  // The accent used for the icon container and the "selected" hint.
  // baseUrl owns its own hue now, so it can no longer be confused with
  // the collection colour it used to share. The "自定义" row uses
  // `variable` — a value the user supplies — rather than borrowing
  // `module`'s amber, which would drag a tree concept into a picker.
  return (
    <div ref={rootRef} className={cn('relative inline-block', className)}>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        data-testid="baseurl-trigger"
        title={resolvedBaseUrl || '点击选择 baseUrl 来源'}
        className={cn(
          // Card-style trigger — matches the SettingsRow outline. We
          // nest an icon container so the affordance reads as "this is
          // the baseUrl selector", not "this is a chevron dropdown".
          'flex h-9 items-center gap-2 rounded-lg border border-input bg-background px-2 text-xs transition-colors',
          'hover:bg-accent/40',
          TRIGGER_WIDTH,
          !defaultDef && selection.kind === 'default' && 'text-muted-foreground',
          focusRing,
        )}
      >
        <IconTile size="xs" bordered concept="baseurl" icon={<Globe className="h-3 w-3" />} />
        <span className="min-w-0 flex-1 text-left leading-tight">
          <span className="block truncate font-medium">{triggerLabel}</span>
          {triggerSubLabel && (
            <span className="block truncate font-mono text-[10px] text-muted-foreground/70 leading-tight">
              {triggerSubLabel}
            </span>
          )}
        </span>
        <ChevronDown
          className={cn(
            'h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform',
            open && 'rotate-180',
          )}
        />
      </button>

      {open && (
        <div
          role="listbox"
          aria-label="选择 baseUrl 来源"
          className={cn(
            'absolute top-full left-0 z-50 mt-1 overflow-hidden rounded-lg border border-border bg-popover shadow-lg',
            POPOVER_WIDTH,
          )}
        >
          <ul className="max-h-80 overflow-auto p-1">
            <Row
              selected={selection.kind === 'default'}
              onSelect={() => {
                onSelectionChange({ kind: 'default' });
                setOpen(false);
              }}
              concept="baseurl"
              icon={<Globe className="h-3.5 w-3.5" />}
              primary={defaultDef ? '项目默认' : '项目默认（无）'}
              secondary={defaultDef ? `→ ${defaultDef.name}` : '该项目尚未设置默认 baseUrl'}
              selectedHint={defaultDef ? '默认' : null}
            />
            {definitions.length > 0 && (
              <li className="px-2 pt-2 pb-1 label-caps/60">BaseUrl 定义</li>
            )}
            {definitions.map((def) => (
              <Row
                key={def.id}
                selected={selection.kind === 'definition' && selection.definitionId === def.id}
                onSelect={() => {
                  onSelectionChange({ kind: 'definition', definitionId: def.id });
                  setOpen(false);
                }}
                concept="baseurl"
                icon={<Globe className="h-3.5 w-3.5" />}
                primary={def.name}
                secondary={def.description}
                selectedHint={def.id === defaultDefinitionId ? '默认' : null}
              />
            ))}
            <Row
              selected={selection.kind === 'override'}
              onSelect={() => {
                onSelectionChange({ kind: 'override' });
                // Don't close — the custom input appears and the user
                // needs to type something before the row is meaningful.
              }}
              concept="variable"
              icon={<Pencil className="h-3.5 w-3.5" />}
              primary="自定义"
              secondary={customValue || '在下方输入框填写'}
              selectedHint={null}
            />
          </ul>
          {selection.kind === 'override' && (
            <div className="border-t border-border/60 bg-background/30 p-2">
              <Input
                value={customValue}
                onChange={(e) => onCustomValueChange(e.target.value)}
                placeholder="https://my.custom.host"
                className="h-8 font-mono text-[11px]"
                autoFocus
                data-testid="baseurl-custom-input"
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

interface RowProps {
  selected: boolean;
  onSelect: () => void;
  /** Semantic hue for this row's icon tile. */
  concept: HueConcept;
  icon: React.ReactNode;
  primary: string;
  secondary?: string;
  /** "默认" badge shown on the row that resolves to the project default. */
  selectedHint?: string | null;
}

function Row({ selected, onSelect, concept, icon, primary, secondary, selectedHint }: RowProps) {
  return (
    <li>
      <button
        type="button"
        role="option"
        aria-selected={selected}
        onClick={onSelect}
        className={cn(
          'flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-xs transition-colors',
          'hover:bg-accent hover:text-foreground',
          selected && 'bg-accent text-foreground',
        )}
      >
        <IconTile size="md" bordered concept={concept} icon={icon} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-xs font-medium leading-tight">{primary}</span>
          {secondary && (
            <span className="block truncate font-mono text-[10px] text-muted-foreground/80 leading-tight">
              {secondary}
            </span>
          )}
        </span>
        {selectedHint && <Badge variant="success">{selectedHint}</Badge>}
        {selected && <Check className="h-3.5 w-3.5 shrink-0 text-primary" aria-hidden />}
      </button>
    </li>
  );
}

function hostOf(url: string): string {
  if (!url) return '';
  // Strip scheme + path, keep the host[:port] portion for display.
  const m = url.match(/^[a-z][a-z0-9+.-]*:\/\/([^/?#]+)/i);
  if (m) return m[1];
  return url.length > 28 ? `${url.slice(0, 28)}…` : url;
}
