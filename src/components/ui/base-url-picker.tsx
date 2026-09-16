/**
 * BaseUrlPicker — the small fixed-width dropdown that sits between
 * the method picker and the URL field in HttpTester.
 *
 * The picker now drives a project-level *baseUrl definition* (named
 * slot like "鉴权服务" / "数据服务") plus an optional per-endpoint
 * override. The actual URL is looked up from
 * `env.baseUrls[defId]` at resolve time (see lib/url.ts), so flipping
 * the active env changes the URL automatically without touching the
 * endpoint's selection.
 *
 * The component is fully controlled. The parent owns:
 *   - `selection`  — which named slot is active (or "default" / "override")
 *   - `customValue` — text inside the "自定义" inline input
 *
 * The parent also passes the resolved baseUrl for the trigger label;
 * we don't compute it here to keep the picker dumb.
 */
import { useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Globe } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Input } from './input';
import { Badge } from './badge';
import type { BaseUrlDefinition } from '@/types/domain';

/**
 * What the trigger is currently showing. The `default` variant means
 * "follow the project's default def"; `definition` is an explicit
 * named slot; `override` is the per-endpoint custom URL.
 */
export type BaseUrlSelection =
  | { kind: 'default' }
  | { kind: 'definition'; definitionId: string }
  | { kind: 'override' };

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

const TRIGGER_WIDTH = 'w-44';
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

  return (
    <div ref={rootRef} className={cn('relative inline-block', className)}>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        data-testid="baseurl-trigger"
        className={cn(
          'flex h-9 items-center gap-1.5 rounded-md border border-input bg-background px-2 font-mono text-[11px] transition-colors',
          TRIGGER_WIDTH,
          !defaultDef && selection.kind === 'default' && 'text-muted-foreground',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background',
        )}
        title={resolvedBaseUrl || '点击选择 baseUrl 来源'}
      >
        <Globe className="h-3 w-3 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate text-left">
          <span className="block truncate leading-tight">{triggerLabel}</span>
          {triggerSubLabel && (
            <span className="block truncate text-[9px] text-muted-foreground/70 leading-tight">
              {triggerSubLabel}
            </span>
          )}
        </span>
        <ChevronDown
          className={cn(
            'h-3 w-3 shrink-0 text-muted-foreground transition-transform',
            open && 'rotate-180',
          )}
        />
      </button>

      {open && (
        <div
          role="listbox"
          aria-label="选择 baseUrl 来源"
          className={cn(
            'absolute top-full left-0 z-50 mt-1 overflow-hidden rounded-md border border-border bg-popover shadow-lg',
            POPOVER_WIDTH,
          )}
        >
          <ul className="max-h-72 overflow-auto py-1">
            <Row
              selected={selection.kind === 'default'}
              onSelect={() => {
                onSelectionChange({ kind: 'default' });
                setOpen(false);
              }}
              icon={<span className="text-muted-foreground">·</span>}
              primary={defaultDef ? '项目默认' : '项目默认（无）'}
              secondary={defaultDef ? `→ ${defaultDef.name}` : '该项目尚未设置默认 baseUrl'}
            />
            {definitions.length > 0 && (
              <li className="px-2 pt-1.5 pb-0.5 text-[9px] uppercase tracking-wider text-muted-foreground/60">
                BaseUrl 定义
              </li>
            )}
            {definitions.map((def) => (
              <Row
                key={def.id}
                selected={
                  selection.kind === 'definition' && selection.definitionId === def.id
                }
                onSelect={() => {
                  onSelectionChange({ kind: 'definition', definitionId: def.id });
                  setOpen(false);
                }}
                icon={<span className="text-blue-400/80">●</span>}
                primary={def.name}
                secondary={def.id === defaultDefinitionId ? '项目默认' : ''}
                trailing={def.id === defaultDefinitionId ? <Badge variant="success">默认</Badge> : null}
              />
            ))}
            <li className="my-1 border-t border-border/60" />
            <Row
              selected={selection.kind === 'override'}
              onSelect={() => {
                onSelectionChange({ kind: 'override' });
                // Don't close — the custom input appears and the user
                // needs to type something before the row is meaningful.
              }}
              icon={<span className="text-amber-400/80">●</span>}
              primary="自定义"
              secondary={customValue || '在下方输入框填写'}
            />
          </ul>
          {selection.kind === 'override' && (
            <div className="border-t border-border/60 bg-background/30 p-2">
              <Input
                value={customValue}
                onChange={(e) => onCustomValueChange(e.target.value)}
                placeholder="https://my.custom.host"
                className="h-7 font-mono text-[11px]"
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

function Row({
  selected,
  onSelect,
  icon,
  primary,
  secondary,
  trailing,
}: {
  selected: boolean;
  onSelect: () => void;
  icon: React.ReactNode;
  primary: string;
  secondary?: string;
  trailing?: React.ReactNode;
}) {
  return (
    <li>
      <button
        type="button"
        role="option"
        aria-selected={selected}
        onClick={onSelect}
        className={cn(
          'flex w-full items-center gap-2 px-2 py-1.5 text-left text-[11px] transition-colors',
          'hover:bg-accent hover:text-foreground',
          selected && 'bg-accent/60',
        )}
      >
        <span className="w-3 shrink-0 text-center">{icon}</span>
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium leading-tight">{primary}</span>
          {secondary && (
            <span className="block truncate font-mono text-[10px] text-muted-foreground/80 leading-tight">
              {secondary}
            </span>
          )}
        </span>
        {trailing}
        {selected && <Check className="h-3 w-3 shrink-0 text-primary" />}
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
