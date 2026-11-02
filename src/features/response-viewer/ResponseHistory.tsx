import { useEffect, useMemo, useState } from 'react';
import { History, ChevronDown, Star } from 'lucide-react';
import { useAppStore, useActiveEndpointResource } from '@/stores/app-store';
import { DRAFT_ID_PREFIX } from '@/stores/store-types';
import type { ResponseHistoryEntry } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { formatDuration, methodColorVar, statusColorVar, cn } from '@/lib/utils';

/**
 * 响应查看器底部的可折叠「历史（n）」条。
 *
 * 各行来自 store 的 `responseHistory`，由 `response_history` 表支撑、按接口加载 —— 故能
 * 跨重启保留，且每行都带服务端盖的 `id` 与 `createdAt`（见 `ResponseHistoryEntry`）。
 *
 * 每行前面有一个星标：点了收藏这一条，收藏的行不参与保留条数的裁剪，故它永久保存。
 * 点一行则把那次调用的请求与响应一起放回响应区 —— 只放响应的话，用户看到的是一份
 * 没有出处的答案。
 *
 * selector 说明：用稳定的 selector 读原始 `responseHistory` 对象（单个状态字段，而非派生
 * 的 `.find()` / `.filter()` 结果），按接口的数组在组件内解析；最新在前的切片用 `useMemo`
 * 构建，以免每次渲染都新建数组。
 */
export function ResponseHistory() {
  // 历史按接口 id 作键，草稿接口用它的 `draft:` id —— 故未保存的接口同样会累积自己的发送
  // 历史，而不是什么都不显示。
  const endpoint = useActiveEndpointResource()?.node ?? null;
  // 原始引用 —— 只在这份字典本身被替换时重渲染。
  const history = useAppStore((s) => s.responseHistory);
  const setLastResponse = useAppStore((s) => s.setLastResponse);
  const setResponseHistoryFavorite = useAppStore((s) => s.setResponseHistoryFavorite);
  const loadResponseHistory = useAppStore((s) => s.loadResponseHistory);
  const clearResponseHistory = useAppStore((s) => s.clearResponseHistory);
  const [open, setOpen] = useState(false);

  const endpointId = endpoint?.id ?? null;

  // 当这个接口成为屏上那个时，读它存储的历史。
  //
  // 用组件 effect 而非某个动作级 hook 作触发器，这不是风格选择。`activeTabId` 有**三个**
  // 写入方 —— `openTab`（树与命令面板打开接口走它）、`setActiveTab`（标签条）以及
  // `closeTab` 的回落 —— 挂在其中任一个上的 hook 都会静默漏掉其余，而「只有用户切*回*
  // 接口时才加载」的历史与「从不加载」无法区分。按 UI 实际解析出的值来触发，所有路径
  // 都被构造性地覆盖。
  //
  // `id in history` 是「已经有了」的守卫：切回标签不该重取 store 已持有的行；而
  // `clearResponseHistory` 会删掉该键，故清空过的接口确实会被重读。
  useEffect(() => {
    if (!endpointId || endpointId.startsWith(DRAFT_ID_PREFIX)) return;
    if (endpointId in useAppStore.getState().responseHistory) return;
    void loadResponseHistory(endpointId);
  }, [endpointId, loadResponseHistory]);

  // 当前接口的数组引用。用户切换接口时这里会取到新数组（或 undefined）。
  const entries = endpointId ? history[endpointId] : undefined;
  const count = entries?.length ?? 0;

  // 仅供显示：最新在前。
  //
  // store 里最新在*后* —— `appendCapped` 往末尾追加并从头部丢弃，`mergeLoaded` 也把服务端
  // 读回的结果规范成同一顺序 —— 故这一次 reverse 是顺序翻转的唯一位置，也是 store 唯一
  // 可以与它收到的回复不一致的理由。别处再翻一次、或放行回复自带的 `created_at DESC`
  // 顺序，都会静默把整条倒过来：行按最旧在前绘制，而下一次发送会从缓存丢掉最新的一行、
  // 同时 SQLite 裁掉最旧的。
  const ordered = useMemo(() => (entries ? entries.slice().reverse() : []), [entries]);

  // 每次渲染读一次时钟，使下面各行对「3 分钟前」的理解不会互相分歧。不用 `useMemo`：它
  // 不依赖任何状态，memo 反而会把时间戳冻住直到别的什么触发重渲染。
  const now = Date.now();

  const onPick = (entry: ResponseHistoryEntry) => {
    // 解包：查看器展示的是响应，不是存储行。被截断的行带 `bodyTruncated`，但它交出的 body
    // 已是服务端存下的那个截断版本 —— 这里无需再裁。
    //
    // 请求快照一起放回去：响应区左侧画的就是它，故这一次写的是「那次调用的两面」。
    setLastResponse(entry.response, entry.request ?? null);
  };

  const onToggleFavorite = (entry: ResponseHistoryEntry) => {
    setResponseHistoryFavorite(entry.id, !entry.isFavorite);
  };

  const onClear = () => {
    if (!endpointId) return;
    void clearResponseHistory(endpointId);
  };

  return (
    <div className="shrink-0 border-t border-border bg-card/30">
      <div className="flex items-center">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className={cn(
            'flex flex-1 items-center gap-2 px-4 py-1.5 text-left text-xs text-muted-foreground',
            'hover:bg-accent/30 transition-colors',
          )}
          aria-expanded={open}
          data-testid="response-history-toggle"
        >
          <ChevronDown
            className={cn('h-3.5 w-3.5 transition-transform', open ? 'rotate-0' : '-rotate-90')}
          />
          <History className="h-3.5 w-3.5" />
          <span className="font-medium">历史</span>
          <span className="rounded bg-muted px-1 text-[10px] tabular-nums">{count}</span>
        </button>
        {/* 是切换器的兄弟而非子元素：把按钮嵌进切换器既是无效 HTML，也会让每次清空都
            顺带展开面板。`count > 0`：空条没什么可清，这个便利性只会是噪声。 */}
        {count > 0 && (
          <Button
            variant="ghost"
            size="xs"
            onClick={onClear}
            className="mr-2 shrink-0 text-muted-foreground"
            title="清掉自动保留的那些行；收藏的行永久保存，不受这个动作影响"
            data-testid="response-history-clear"
          >
            清空未收藏
          </Button>
        )}
      </div>

      {open && (
        <div className="max-h-56 overflow-y-auto border-t border-border/60">
          {ordered.length === 0 ? (
            <div className="px-4 py-3 text-center text-xs text-muted-foreground/70">
              暂无历史 — 发送请求后会出现在这里
            </div>
          ) : (
            <ul className="divide-y divide-border/40">
              {ordered.map((entry) => (
                <HistoryRow
                  key={entry.id}
                  entry={entry}
                  now={now}
                  onPick={onPick}
                  onToggleFavorite={onToggleFavorite}
                />
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * 「刚刚 / 12 秒前 / 3 分钟前」，「x 分钟前」不再携带信息后退回具体时刻。
 *
 * 留在本文件而非 `src/lib/utils.ts`：本组件是它唯一的调用方，而 `utils.ts` 其余都是载荷
 * 的格式化器（字节、毫秒、状态色相），没有「现在」的概念。等第二个调用方出现时再提升。
 */
function formatRelativeTime(createdAt: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - createdAt) / 1000));
  if (seconds < 10) return '刚刚';
  if (seconds < 60) return `${seconds} 秒前`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return new Date(createdAt).toLocaleString();
}

interface HistoryRowProps {
  entry: ResponseHistoryEntry;
  /** 列表中每一行共用的一次时钟读数。 */
  now: number;
  onPick: (entry: ResponseHistoryEntry) => void;
  onToggleFavorite: (entry: ResponseHistoryEntry) => void;
}

function HistoryRow({ entry, now, onPick, onToggleFavorite }: HistoryRowProps) {
  // method / url 不属于最初的 ApiResponse 契约，而是发送方填的可选字段。后端若忘了填，
  // 回落到占位值。
  const method = (entry.response.method ?? 'GET') as string;
  const url = entry.response.url ?? '';
  const status = entry.response.status;
  const truncated = url.length > 60 ? `${url.slice(0, 57)}…` : url;

  return (
    <li className="flex items-center">
      {/* 星标是这一行的兄弟而非子元素：嵌进那个按钮既是无效 HTML，也会让点星标变成
          「打开这条历史」。 */}
      <button
        type="button"
        onClick={() => onToggleFavorite(entry)}
        title={
          entry.isFavorite ? '取消收藏（这一条会重新受保留条数约束）' : '收藏这一条（永久保存）'
        }
        aria-pressed={!!entry.isFavorite}
        aria-label={entry.isFavorite ? '取消收藏' : '收藏'}
        className={cn(
          'flex h-7 w-7 shrink-0 items-center justify-center rounded transition-colors',
          'hover:bg-accent/40',
          entry.isFavorite ? 'text-warning-foreground' : 'text-muted-foreground/40',
        )}
        data-testid="response-history-favorite"
      >
        <Star className={cn('h-3.5 w-3.5', entry.isFavorite && 'fill-current')} />
      </button>
      <Button
        variant="ghost"
        size="sm"
        onClick={() => onPick(entry)}
        className={cn(
          'h-auto min-w-0 flex-1 justify-start gap-2 rounded-none px-2 py-1.5 text-xs font-normal',
          'hover:bg-accent/40',
        )}
        data-testid="response-history-item"
      >
        <span
          className="inline-flex h-5 w-12 shrink-0 items-center justify-center rounded font-mono text-[10px] font-semibold text-white"
          style={{ backgroundColor: methodColorVar(method) }}
        >
          {method}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-foreground/80">
          {truncated || '(无 URL)'}
        </span>
        {/* 服务端在 256 KiB 处截断了这个 body 并连同 `bodyJson` 一起丢弃，故查看器会
            显示半份文档。没有标记时，用户分不清这是被截断还是接口本就只返回了半份。
            `--warning-foreground` 是琥珀色阶的文字档（暗色作用域里被提亮）—— 见 AGENTS.md
            的语义色配对说明；把 `-foreground` 色标配到浅色*表面*上才是陷阱。 */}
        {entry.bodyTruncated && (
          <span
            className="shrink-0 rounded bg-warning/15 px-1.5 py-0.5 text-[10px] text-warning-foreground"
            title="响应体超过 256 KiB，已截断存储"
            data-testid="response-history-truncated"
          >
            已截断
          </span>
        )}
        <span
          className="shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px] tabular-nums"
          style={{ color: statusColorVar(status) }}
        >
          {status}
        </span>
        <span className="shrink-0 w-14 text-right tabular-nums text-muted-foreground">
          {formatDuration(entry.response.durationMs)}
        </span>
        {/* 第三级文字：有，但不可操作 —— 这一行带上时间戳，是因为能跨重启存活的历史记录
            的是*何时*，不只是回来了什么。 */}
        <span className="shrink-0 w-16 text-right tabular-nums text-faint">
          {formatRelativeTime(entry.createdAt, now)}
        </span>
      </Button>
    </li>
  );
}
