/**
 * BaseUrlPicker — the dropdown that sits between the method picker and
 * the URL field in HttpTester.
 *
 * The picker drives a baseUrl *service* (named slot like "鉴权服务" /
 * "数据服务") plus an optional per-endpoint override. The actual URL is
 * looked up from `env.moduleBaseUrls[defId][moduleId]` at resolve time
 * (see lib/url.ts), so flipping the active env changes the URL
 * automatically without touching the endpoint's selection.
 *
 * `definitions` is the project's whole service list — services belong to
 * the project, so every module sees all of them and every one of them is a
 * real choice here. `defaultDefinitionId` is therefore the project-wide ★
 * (`Project.defaultBaseUrlDefinitionId`), not a module's.
 *
 * The component is fully controlled. The parent owns:
 *   - `selection`  — which named slot is active (or "default" / "override")
 *
 * The list holds the project's services and 自定义, and nothing else.
 * There used to be a 项目默认 row above the definitions plus a "BaseUrl 定义"
 * section header: both duplicated what the row carrying the 默认 badge
 * already said, so "follow the project default" is now that row
 * itself (see the `isProjectDefault` branch in the map below).
 *
 * 自定义 (override) deliberately has **no input of its own**: picking it
 * hands the URL field over to the user as the whole address, so a second
 * host box here would be a second source of truth for the same request.
 * An override that carries a host (rows saved before this split) still
 * resolves as that host prefix — `resolvedBaseUrl` is non-empty in that
 * case — and the trigger's sub-label shows it.
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
  /**
   * The project's services — drives the dropdown rows. Not filtered by the
   * endpoint's module: the list is project-level, so every module's
   * endpoints can point at any of them.
   */
  definitions: readonly BaseUrlDefinition[];
  /** The project-wide default service id (may be null when it has none). */
  defaultDefinitionId: string | null;
  /**
   * The currently resolved baseUrl. Used only for the trigger's
   * secondary label (e.g. "· api.demo.dev") so the user can confirm
   * the actual host at a glance.
   */
  resolvedBaseUrl: string;
  selection: BaseUrlSelection;
  onSelectionChange: (next: BaseUrlSelection) => void;
  className?: string;
}

const TRIGGER_WIDTH = 'w-52';
const POPOVER_WIDTH = 'w-80';

export function BaseUrlPicker({
  definitions,
  defaultDefinitionId,
  resolvedBaseUrl,
  selection,
  onSelectionChange,
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

  // Default-row label: the project ★ by name when it has one, otherwise an
  // explicit "无" so the empty case is obvious rather than reading as a
  // name. The custom row always shows "自定义".
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
    // Following the project default reads as *that service*, not as a
    // separate "项目默认" choice: the list has no such row, and the
    // trigger has to name the same thing the list highlights.
    return defaultDef ? defaultDef.name : '项目无默认服务';
  }, [selection, selectedDef, defaultDef]);

  // The sub-label answers "what host will actually be used?". In
  // 自定义 mode there may be no host at all — the URL field holds the
  // whole address — so say that instead of rendering an empty row.
  const triggerSubLabel = useMemo(() => {
    if (selection.kind === 'override') {
      return resolvedBaseUrl ? `· ${hostOf(resolvedBaseUrl)}` : '· URL 栏直填完整地址';
    }
    if (!resolvedBaseUrl) return '';
    return `· ${hostOf(resolvedBaseUrl)}`;
  }, [selection, resolvedBaseUrl]);

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
            {definitions.map((def) => {
              // The row carrying the 默认 badge *is* the "follow the
              // project default" choice — a separate row used to sit
              // above it and say the same thing, so picking that row and
              // picking this one resolved to the same host. Selecting
              // this row therefore stores no definition id at all, which
              // keeps the endpoint tracking whichever service the project
              // stars later.
              const isProjectDefault = def.id === defaultDefinitionId;
              return (
                <Row
                  key={def.id}
                  selected={
                    selection.kind === 'default'
                      ? isProjectDefault
                      : selection.kind === 'definition' && selection.definitionId === def.id
                  }
                  onSelect={() => {
                    onSelectionChange(
                      isProjectDefault
                        ? { kind: 'default' }
                        : { kind: 'definition', definitionId: def.id },
                    );
                    setOpen(false);
                  }}
                  concept="baseurl"
                  icon={<Globe className="h-3.5 w-3.5" />}
                  primary={def.name}
                  secondary={def.description}
                  selectedHint={isProjectDefault ? '默认' : null}
                />
              );
            })}
            {selection.kind === 'default' && !defaultDef && (
              // Only reachable when the project has no starred service at
              // all: the endpoint is following the default, but there is
              // no default to follow. Without this the list would open
              // with nothing selected and no explanation.
              <li className="px-2 py-1.5 text-[11px] text-muted-foreground">
                该项目尚未设置默认服务
              </li>
            )}
            <Row
              selected={selection.kind === 'override'}
              onSelect={() => {
                onSelectionChange({ kind: 'override' });
                // Close: the row's whole job is to hand the URL field to
                // the user, and they are about to type into it — leaving a
                // popover open over the URL bar is what made this feel
                // inert before.
                setOpen(false);
              }}
              concept="variable"
              icon={<Pencil className="h-3.5 w-3.5" />}
              primary="自定义"
              secondary="URL 栏直接填完整地址，不拼接环境前缀"
              selectedHint={null}
            />
          </ul>
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
