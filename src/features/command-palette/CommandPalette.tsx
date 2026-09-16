/**
 * Command Palette — full-screen modal opened with Ctrl/Cmd + K.
 *
 * Architecture:
 *  - Receives an `open` flag and an `onClose` callback from the parent.
 *  - When `open` flips to `true`, the search input auto-focuses and the
 *    selection index resets to 0.
 *  - Commands come from `command-registry.buildCommands()`; per-render
 *    fuzzy filtering happens inside this component (avoids allocating in
 *    the Zustand selector — see app-store.ts header).
 *  - Click on backdrop or Esc key calls `onClose`. Outside click is
 *    detected by listening on the backdrop `mousedown` so that selection
 *    inside the dialog doesn't dismiss it.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { Search, CornerDownLeft, ArrowUp, ArrowDown, X } from 'lucide-react';
import { buildCommands, type Command } from './command-registry';
import { cn } from '@/lib/utils';

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
}

const MAX_RESULTS = 50;

/** Lowercase substring match with a small prefix / boundary bonus. */
function scoreCommand(cmd: Command, q: string): number {
  if (!q) return 1; // empty query: show everything in registry order
  const title = cmd.title.toLowerCase();
  const hint = (cmd.hint ?? '').toLowerCase();
  const kw = (cmd.keywords ?? []).join(' ').toLowerCase();
  const hay = `${title} ${hint} ${kw}`;
  const idx = hay.indexOf(q);
  if (idx < 0) return -1;
  // Earlier match is better; matches inside `title` are preferred.
  let bonus = 0;
  if (title.startsWith(q)) bonus += 50;
  else if (title.includes(q)) bonus += 20;
  if (idx === 0) bonus += 30;
  return 1000 - idx + bonus;
}

export function CommandPalette({ open, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Snapshot of commands — rebuild on open so newly-added endpoints show up.
  const allCommands = useMemo<Command[]>(() => (open ? buildCommands() : []), [open]);

  const results = useMemo(() => {
    const q = query.trim().toLowerCase();
    const scored = allCommands
      .map((cmd) => ({ cmd, score: scoreCommand(cmd, q) }))
      .filter((x) => x.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, MAX_RESULTS)
      .map((x) => x.cmd);
    return scored;
  }, [allCommands, query]);

  // Reset state when (re)opening.
  useEffect(() => {
    if (open) {
      setQuery('');
      setSelected(0);
      // Focus on next tick so the modal is in the DOM.
      const t = window.setTimeout(() => inputRef.current?.focus(), 0);
      return () => window.clearTimeout(t);
    }
    return undefined;
  }, [open]);

  // Clamp selection when the result list shrinks.
  useEffect(() => {
    if (selected >= results.length) setSelected(Math.max(0, results.length - 1));
  }, [results.length, selected]);

  // Auto-scroll the selected row into view.
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const el = list.querySelector<HTMLElement>(`[data-idx="${selected}"]`);
    if (el) el.scrollIntoView({ block: 'nearest' });
  }, [selected]);

  if (!open) return null;

  const run = (cmd: Command) => {
    cmd.run();
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (results.length > 0) setSelected((s) => (s + 1) % results.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (results.length > 0) setSelected((s) => (s - 1 + results.length) % results.length);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const cmd = results[selected];
      if (cmd) run(cmd);
    }
  };

  // Group consecutive commands by section for header rendering.
  const grouped: { section: string; items: { cmd: Command; idx: number }[] }[] = [];
  results.forEach((cmd, idx) => {
    const last = grouped[grouped.length - 1];
    if (last && last.section === cmd.section) {
      last.items.push({ cmd, idx });
    } else {
      grouped.push({ section: cmd.section, items: [{ cmd, idx }] });
    }
  });

  return (
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center px-4 pt-[12vh] animate-fade-in"
      onMouseDown={(e) => {
        // Click outside the dialog panel closes.
        // Note: `e.target === e.currentTarget` does NOT work here because
        // there is an inner backdrop div that captures the mousedown.
        // Use closest('[role="dialog"]') so any future DOM nesting still
        // resolves correctly.
        const dialog = (e.currentTarget as HTMLElement).querySelector(
          '[role="dialog"]',
        );
        if (dialog && !dialog.contains(e.target as Node)) onClose();
      }}
    >
      <div className="absolute inset-0 bg-black/50 backdrop-blur-sm" />
      <div
        role="dialog"
        aria-modal="true"
        aria-label="命令面板"
        className="relative w-full max-w-xl overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl"
        onKeyDown={onKeyDown}
      >
        {/* Search row */}
        <div className="flex items-center gap-2 border-b border-border px-3">
          <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索命令、接口、项目…"
            className="h-11 flex-1 bg-transparent text-sm text-foreground placeholder:text-muted-foreground/60 focus:outline-none"
            data-testid="command-palette-input"
            autoComplete="off"
            spellCheck={false}
          />
          <button
            type="button"
            onClick={onClose}
            className="inline-flex h-6 w-6 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
            aria-label="关闭"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>

        {/* Results */}
        <div
          ref={listRef}
          className="max-h-[55vh] overflow-y-auto py-1"
          data-testid="command-palette-results"
        >
          {results.length === 0 && (
            <div className="px-4 py-10 text-center text-sm text-muted-foreground">
              没有匹配的命令
            </div>
          )}
          {grouped.map((group) => (
            <div key={group.section} className="pb-1">
              <div className="px-3 pb-1 pt-2 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                {group.section}
              </div>
              {group.items.map(({ cmd, idx }) => {
                const active = idx === selected;
                return (
                  <button
                    key={cmd.id}
                    data-idx={idx}
                    onMouseEnter={() => setSelected(idx)}
                    onClick={() => run(cmd)}
                    className={cn(
                      'flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm',
                      active ? 'bg-primary/15 text-foreground' : 'text-foreground/90 hover:bg-accent/60',
                    )}
                  >
                    <span className="min-w-0 flex-1 truncate">{cmd.title}</span>
                    {cmd.hint && (
                      <span className="ml-2 shrink-0 truncate text-[11px] text-muted-foreground">
                        {cmd.hint}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          ))}
        </div>

        {/* Footer hint */}
        <div className="flex items-center gap-3 border-t border-border bg-card/40 px-3 py-1.5 text-[10px] text-muted-foreground">
          <span className="inline-flex items-center gap-1">
            <CornerDownLeft className="h-3 w-3" /> 触发
          </span>
          <span className="inline-flex items-center gap-1">
            <ArrowUp className="h-3 w-3" />
            <ArrowDown className="h-3 w-3" /> 选择
          </span>
          <span className="inline-flex items-center gap-1">
            <span className="kbd">Esc</span> 关闭
          </span>
        </div>
      </div>
    </div>
  );
}
