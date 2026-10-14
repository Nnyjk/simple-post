/**
 * 命令面板 —— Ctrl/Cmd + K 打开的全屏模态。
 *
 * - 命令来自 `command-registry.buildCommands()`，模糊过滤在本组件内做：
 *   放进 Zustand selector 会在其中新建数组，触发无限重渲染。
 * - 点遮罩或按 Esc 关闭。判定「点在面板外」靠 backdrop 上的 `mousedown`
 *   加 `closest('[role="dialog"]')`，故面板内选中一项不会误关。
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { Search, CornerDownLeft, ArrowUp, ArrowDown, X } from 'lucide-react';
import { buildCommands, type Command } from './command-registry';
import { cn } from '@/lib/utils';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
}

const MAX_RESULTS = 50;

/** 小写子串匹配，前缀 / 词首命中另加一点分。 */
function scoreCommand(cmd: Command, q: string): number {
  if (!q) return 1; // 空查询：按注册表原序全部列出
  const title = cmd.title.toLowerCase();
  const hint = (cmd.hint ?? '').toLowerCase();
  const kw = (cmd.keywords ?? []).join(' ').toLowerCase();
  const hay = `${title} ${hint} ${kw}`;
  const idx = hay.indexOf(q);
  if (idx < 0) return -1;
  // 命中位置越靠前越好，命中 `title` 内的额外加分。
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

  // 命令快照 —— 每次打开重建，好让新建的接口立刻出现在面板里。
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

  // 每次打开重置状态。
  useEffect(() => {
    if (open) {
      setQuery('');
      setSelected(0);
      // 延后一拍，好让模态先进 DOM。
      const t = window.setTimeout(() => inputRef.current?.focus(), 0);
      return () => window.clearTimeout(t);
    }
    return undefined;
  }, [open]);

  // 结果变短时把选中项收进范围，否则回车会落在已消失的行上。
  useEffect(() => {
    if (selected >= results.length) setSelected(Math.max(0, results.length - 1));
  }, [results.length, selected]);

  // 把选中行滚进视野。
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const el = list.querySelector<HTMLElement>(`[data-idx="${selected}"]`);
    if (el) el.scrollIntoView({ block: 'nearest' });
  }, [selected]);

  const backdropRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // 入场动画，每次挂载（`open` 翻 true）都跑一遍；无退场动画 —— `open=false`
  // 直接卸载整棵树。reduced motion 下退化为直接出现，自动聚焦与键盘行为不变。
  useGSAP(
    () => {
      return motionSafe(() => {
        if (backdropRef.current) {
          gsap.fromTo(
            backdropRef.current,
            { opacity: 0 },
            { opacity: 1, duration: 0.18, ease: 'none' },
          );
        }
        if (panelRef.current) {
          gsap.fromTo(
            panelRef.current,
            { opacity: 0, scale: 0.98, y: -8 },
            { opacity: 1, scale: 1, y: 0, duration: 0.22, ease: 'power3.out' },
          );
        }
      });
    },
    { scope: panelRef, dependencies: [open] },
  );

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

  // 按 section 把相邻命令并成组，好渲染分组标题。
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
      className="fixed inset-0 z-[100] flex items-start justify-center px-4 pt-[12vh]"
      onMouseDown={(e) => {
        // 点面板外关闭。不能用 `e.target === e.currentTarget`：外层还有一层
        // backdrop 会吃掉 mousedown。改用 `closest('[role="dialog"]')`，
        // 日后 DOM 再嵌一层也仍能判对。
        const dialog = (e.currentTarget as HTMLElement).querySelector('[role="dialog"]');
        if (dialog && !dialog.contains(e.target as Node)) onClose();
      }}
    >
      <div ref={backdropRef} className="absolute inset-0 bg-black/50 backdrop-blur-sm" />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label="命令面板"
        className="relative w-full max-w-xl overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl"
        onKeyDown={onKeyDown}
      >
        {/* 搜索行 */}
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

        {/* 结果列表 */}
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
              <div className="px-3 pb-1 pt-2 label-caps">{group.section}</div>
              {group.items.map(({ cmd, idx }) => {
                const active = idx === selected;
                return (
                  <button
                    type="button"
                    key={cmd.id}
                    data-idx={idx}
                    onMouseEnter={() => setSelected(idx)}
                    onClick={() => run(cmd)}
                    className={cn(
                      'flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm',
                      active
                        ? 'bg-primary/15 text-foreground'
                        : 'text-foreground/90 hover:bg-accent/60',
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

        {/* 底部快捷键提示 */}
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
