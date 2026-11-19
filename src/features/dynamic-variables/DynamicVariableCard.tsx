/**
 * 一条动态变量的胶囊卡片：一行摘要，悬浮出三个图标操作，左键点开当前值面板。
 *
 * 数据源、取数环境、抽取规则、处理脚本与缓存时长都不在这行里——它们属于「详情」表单，用户
 * 要改定义时点铅笔。这行只回答两件事：这条定义叫什么、它此刻的值是什么。
 *
 * 卡片自带覆盖值编辑器的状态，而值是 (定义 × 取数环境) 的一格：换环境就是另一格，故调用方
 * 用 `key={`${变量 id}:${环境 id}`}` 让卡片重挂。重挂之前，`overrideEditor` 判定列表内容还
 * 属于上一个环境，那时值面板只读。
 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import { AlertCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import type { DynamicVariable, DynamicVariableView } from '@/types/domain';
import { Tooltip } from '@/components/ui/tooltip';
import { LAYER } from '@/components/ui/layers';
import { cn, focusRing, iconButton } from '@/lib/utils';
import { gsap, motionSafe, useGSAP } from '@/lib/gsap';
import { dynamicValueScope, type DynamicVariableDraft } from '@/stores/dynamic-slice';
import { DynamicVariableForm } from './DynamicVariableForm';
import { overrideEditor } from './override-editor';

interface DynamicVariableCardProps {
  variable: DynamicVariableView;
  /** 归属项目。取值桶按项目分，故查失败原因时要用它当键。 */
  projectId: string;
  resolving: boolean;
  editing: boolean;
  onEdit: () => void;
  onCancelEdit: () => void;
  /** 编辑表保存成功：提交的那份草稿与落库的那份定义一起交回去。 */
  onFinishEdit: (
    submitted: DynamicVariableDraft,
    saved: DynamicVariable,
    patch: Partial<DynamicVariable>,
  ) => void;
  onRefresh: () => void;
  onClearOverride: () => void;
  onDelete: () => void;
}

export function DynamicVariableCard({
  variable,
  projectId,
  resolving,
  editing,
  onEdit,
  onCancelEdit,
  onFinishEdit,
  onRefresh,
  onClearOverride,
  onDelete,
}: DynamicVariableCardProps) {
  const [valueOpen, setValueOpen] = useState(false);
  const [draft, setDraft] = useState(variable.value ?? '');
  // 用户是否亲手改过框里的文本。关面板、失焦、Esc 都走保存，但只有改过才发写入：取数刷新会
  // 换掉列表里的值，框里还留着旧文本，不该把旧值写成覆盖值。
  const [dirty, setDirty] = useState(false);
  const setDynamicOverride = useAppStore((s) => s.setDynamicOverride);
  // 取数失败的原因。列表视图本身不带它（那是一次纯读），故取数时记进 store 的那份。
  // 失败原因也按 (项目 × 取数环境) 分桶：查的是当前环境那一格。
  const activeEnvironmentId = useAppStore((s) => s.activeEnvironmentId);
  const dynamicError = useAppStore(
    (s) =>
      s.dynamicErrors[dynamicValueScope(projectId, activeEnvironmentId || null)]?.[variable.name],
  );
  const error = variable.error ?? dynamicError;
  // 这批「当前值」是不是当前这个环境拉到的。切换环境的那一刻列表还在路上，内容仍是上一个
  // 环境的：此刻编辑框里的文本是 A 的值，保存却会写进 B。
  const listScope = useAppStore((s) => s.dynamicVariablesScope);
  const editor = overrideEditor(listScope, projectId, activeEnvironmentId || null, variable.value);
  const pillRef = useRef<HTMLDivElement>(null);
  const pillButtonRef = useRef<HTMLButtonElement>(null);
  const clearButtonRef = useRef<HTMLButtonElement>(null);

  /**
   * 离开输入框即保存。提交空串表示「清掉覆盖值，回到自动取数」：后端把 `null` 读成删除那条
   * 覆盖值记录，把空串读成一条值为空的覆盖值——后者会永久盖住取数结果，用户删光文本多半不
   * 是这个意思。
   */
  const commitValue = async () => {
    if (!editor.editable || !dirty) return;
    setDirty(false);
    await setDynamicOverride(variable.id, draft === '' ? null : draft);
  };

  const openValuePanel = () => {
    setDraft(editor.initialValue);
    setDirty(false);
    setValueOpen(true);
  };

  /**
   * 面板的所有关闭路径都先存一次：用户点了外面或按了 Esc，不等于撤回刚敲的字。
   *
   * `restoreFocus` 只对「Esc、点外面、再点一次胶囊」为真：那三条是收面板，焦点该回到胶囊。
   * 点「详情」转去开表单、点「恢复自动获取」这两条是用户已经在别处动手，还回去反而抢走焦点。
   */
  const closeValuePanel = (restoreFocus: boolean) => {
    void commitValue();
    setValueOpen(false);
    if (!restoreFocus) return;
    requestAnimationFrame(() => {
      const active = document.activeElement;
      // 这一帧里焦点已经落到别的按钮或输入框（用户点了另一条胶囊、另一个表单）就不抢。
      if (active && active !== document.body && active !== pillButtonRef.current) return;
      pillButtonRef.current?.focus();
    });
  };

  const toggleValuePanel = () => {
    if (valueOpen) {
      closeValuePanel(true);
      return;
    }
    openValuePanel();
  };

  const handleEdit = () => {
    closeValuePanel(false);
    onEdit();
  };

  return (
    // 容器查询量的是这一列的宽度，不是胶囊自己的：胶囊现在按内容宽度收窄，而 `inline-size`
    // 容器不按内容定自身尺寸——两者放同一个元素上，胶囊会塌成 padding 宽、内容溢出。
    // 所以 `@container` 放在这层包上。`group` 也在这层：指针落在这一行任何位置（包括胶囊
    // 右侧的空白）都要能照亮行尾的图标。
    <div className="@container group flex flex-col">
      {/* 窄栏目里先丢摘要、再丢 `{{name}}`，名字与来源始终在。
          图标只收间距、不隐藏，它们的位置固定，悬停时胶囊不会往右跳。 */}
      <div
        ref={pillRef}
        className={cn(
          'flex h-8 w-fit max-w-full items-center gap-2 self-start rounded-pill border border-border bg-card/40 px-3',
          // 正在编辑的那一条：胶囊自身要有选中态，否则下面展开的表单看不出属于哪一行。
          editing && 'border-primary/50 bg-primary/5',
        )}
      >
        <button
          ref={pillButtonRef}
          type="button"
          onClick={toggleValuePanel}
          aria-expanded={valueOpen}
          title={editor.editable ? '查看并编辑这条的当前值' : '列表还在换成当前环境的值：此刻只读'}
          className={cn(
            'flex min-w-0 flex-1 items-center gap-2 rounded-pill text-left text-xs',
            focusRing,
          )}
        >
          {/* 停用的定义不参与替换：名字压暗一档，一眼看出这条不生效。 */}
          <span
            className={cn(
              'shrink-0 font-mono text-sm',
              !variable.enabled && 'text-muted-foreground',
            )}
          >
            {variable.name}
          </span>
          {/* 常驻一段宽度：取数失败时才出图标，出现时名字不跟着挪。 */}
          <span className="flex w-3.5 shrink-0 items-center justify-center">
            {error && (
              <AlertCircle
                className="h-3.5 w-3.5 text-destructive"
                role="img"
                aria-label={`取数失败：${error.kind}`}
              />
            )}
          </span>
          <code className="@max-[26rem]:hidden shrink-0 rounded bg-muted px-1 py-0.5 font-mono text-micro text-muted-foreground">
            {`{{${variable.name}}}`}
          </code>
          {!variable.enabled && <span className="shrink-0 text-nano text-faint">已停用</span>}
          <StatusBadge source={variable.source} plain />
          {/* 值为空时不渲染这一格：同一行的来源标记已经说了「没有值」，再说一遍就成了
              「没有值（没有值）」。 */}
          {variable.value && (
            <span className="@max-[20rem]:hidden min-w-0 truncate font-mono text-micro text-muted-foreground">
              {summarize(variable.value)}
            </span>
          )}
        </button>

        {/* 图标操作压在行尾：`opacity-0` 但仍在布局里，悬浮现形时胶囊不会往右跳。键盘 Tab 进
            这一行同样把它们照出来（`group-focus-within`）。 */}
        <span
          className={cn(
            'flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100',
            // 正在编辑的那一条：图标常显，不用先悬停才找得到「详情 / 删除」。
            editing && 'opacity-100',
          )}
        >
          <Tooltip content={resolving ? '正在取数' : '跳过缓存，重新取一次数'}>
            <button
              type="button"
              aria-label="刷新"
              className={iconButton}
              onClick={onRefresh}
              disabled={resolving}
            >
              <RefreshCw className="h-3.5 w-3.5" />
            </button>
          </Tooltip>
          <Tooltip content="详情：数据源、抽取规则、处理脚本与缓存">
            <button type="button" aria-label="详情" className={iconButton} onClick={handleEdit}>
              <Pencil className="h-3.5 w-3.5" />
            </button>
          </Tooltip>
          <Tooltip content="删除这条定义">
            <button type="button" aria-label="删除" className={iconButton} onClick={onDelete}>
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </Tooltip>
        </span>
      </div>

      <ValuePanel anchorRef={pillRef} open={valueOpen} onClose={() => closeValuePanel(true)}>
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium">当前值</span>
            <StatusBadge source={variable.source} />
            <span className="ml-auto text-nano text-faint">
              {variable.value ? `${variable.value.length} 字符` : '空'}
            </span>
            {variable.isOverride && (
              <button
                ref={clearButtonRef}
                type="button"
                onClick={() => {
                  setValueOpen(false);
                  onClearOverride();
                }}
                disabled={!editor.editable}
                title={
                  editor.editable
                    ? '删掉这条手工指定的值，回到自动取数'
                    : '列表还在换成当前环境的值：此刻恢复自动获取，删掉的是另一个环境的覆盖值'
                }
                className={cn(
                  'rounded px-1.5 py-0.5 text-micro text-muted-foreground transition-colors hover:text-foreground disabled:pointer-events-none disabled:opacity-40',
                  focusRing,
                )}
              >
                恢复自动获取
              </button>
            )}
          </div>
          <textarea
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setDirty(true);
            }}
            onBlur={(e) => {
              // 焦点交给「恢复自动获取」时别先存一遍：那次保存会在清覆盖值之后落地，把值又写回
              // 成覆盖值。
              if (e.relatedTarget === clearButtonRef.current) return;
              void commitValue();
            }}
            readOnly={!editor.editable}
            autoFocus={editor.editable}
            spellCheck={false}
            aria-label="当前值"
            className={cn(
              'max-h-60 min-h-24 w-full min-w-0 resize-y overflow-auto whitespace-pre-wrap break-all rounded-md border border-input bg-background px-2 py-1 font-mono text-xs',
              !editor.editable && 'opacity-60',
            )}
          />
          {editor.editable ? (
            <p className="text-micro text-muted-foreground">
              离开输入框即保存为覆盖值，过期时间沿用 TTL。清空后再离开输入框，就会删掉覆盖值、
              回到自动取数。
            </p>
          ) : (
            <p className="text-micro text-warning">
              列表还在换成当前环境的值，此刻不该照着它写覆盖值。
            </p>
          )}
          {/* 取数时刻与取数失败都在面板里：胶囊只留一个警示点，列表才能是一叠 32px 的单行。 */}
          <p className="min-w-0 break-words text-nano text-faint">
            {variable.fetchedAt ? `取数于 ${formatTime(variable.fetchedAt)}` : '尚未取数'}
            {variable.expiresAt ? ` · 过期于 ${formatTime(variable.expiresAt)}` : ''}
          </p>
          {error && (
            <p className="min-w-0 break-words text-nano text-destructive">
              {`${error.kind}：${error.message}`}
            </p>
          )}
        </div>
      </ValuePanel>

      {editing && (
        <div className="mt-1.5">
          <DynamicVariableForm
            variable={variable}
            projectId={projectId}
            onCancel={onCancelEdit}
            onDone={onFinishEdit}
          />
        </div>
      )}
    </div>
  );
}

interface ValuePanelProps {
  anchorRef: RefObject<HTMLDivElement | null>;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

/**
 * 贴着胶囊下方浮出的面板。挂到 body 上而不是胶囊里：列表容器是滚动区，放在里面会被裁掉。
 *
 * 位置在挂载后按锚点与面板实测尺寸算：下方放不下就翻到上方，左右收进视口。
 */
function ValuePanel({ anchorRef, open, onClose, children }: ValuePanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number; width: number } | null>(null);
  // 关闭回调每次渲染都换新（它要读到最新的输入框文本），而监听器只在打开时挂一次：故经 ref
  // 取最新那一份。
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  const place = useCallback(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const panelHeight = panelRef.current?.getBoundingClientRect().height ?? 0;
    const width = Math.min(Math.max(rect.width, 320), window.innerWidth - 16);
    const left = Math.min(Math.max(rect.left, 8), Math.max(8, window.innerWidth - width - 8));
    let top = rect.bottom + 6;
    if (panelHeight > 0 && top + panelHeight > window.innerHeight - 8) {
      const above = rect.top - panelHeight - 6;
      top = above >= 8 ? above : Math.max(8, window.innerHeight - panelHeight - 8);
    }
    setPos({ left, top, width });
  }, [anchorRef]);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    place();
  }, [open, place]);

  // 面板的位置由 `pos` 决定，`pos` 之前那一帧还整块藏着（`visibility: hidden`）：入场动画等
  // 它落地后再播，否则补间会先在一个贴在左上角的空壳上跑完。
  const placed = pos !== null;
  useGSAP(
    () => {
      if (!open || !placed || !panelRef.current) return;
      return motionSafe(() => {
        gsap.fromTo(
          panelRef.current,
          { opacity: 0, y: -4 },
          { opacity: 1, y: 0, duration: 0.14, ease: 'power2.out', clearProps: 'transform' },
        );
      });
    },
    { dependencies: [open, placed], scope: panelRef },
  );

  useEffect(() => {
    if (!open) return;
    // 面板高度在挂载后才量得出来，故挂载后的一帧再对齐一次。
    const raf = requestAnimationFrame(place);
    const handlePointerDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (panelRef.current?.contains(target)) return;
      // 锚点自己交给胶囊的点击处理：它既开也关，这里再关一次会把刚开的又收掉。
      if (anchorRef.current?.contains(target)) return;
      onCloseRef.current();
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCloseRef.current();
    };
    window.addEventListener('pointerdown', handlePointerDown, true);
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('scroll', place, true);
    window.addEventListener('resize', place);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener('pointerdown', handlePointerDown, true);
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('scroll', place, true);
      window.removeEventListener('resize', place);
    };
  }, [open, place, anchorRef]);

  if (!open) return null;

  return createPortal(
    <div
      ref={panelRef}
      role="dialog"
      aria-label="当前值"
      className={cn(
        'fixed max-h-[70vh] overflow-auto rounded-lg border border-border bg-popover p-3 shadow-lg',
        LAYER.popover,
      )}
      style={{
        left: pos?.left ?? 0,
        top: pos?.top ?? 0,
        width: pos?.width,
        visibility: pos ? 'visible' : 'hidden',
      }}
    >
      {children}
    </div>,
    document.body,
  );
}

/** 当前值的来源。`plain` 给胶囊用：那里已经有一层底，再套一块 `bg-muted` 会读成两层药丸。 */
function StatusBadge({
  source,
  plain = false,
}: {
  source: DynamicVariableView['source'];
  plain?: boolean;
}) {
  const label =
    source === 'override'
      ? '手动改的'
      : source === 'stale'
        ? '已过期'
        : source === 'auto'
          ? '自动取回'
          : '没有值';
  return (
    <span
      className={cn(
        'shrink-0 text-micro',
        plain ? 'text-faint' : 'rounded bg-muted px-1.5 py-0.5 text-muted-foreground',
      )}
    >
      {label}
    </span>
  );
}

/** 值的摘要：长度加开头。全文只在值面板里出现。 */
function summarize(value: string): string {
  const head = value.length > 80 ? `${value.slice(0, 80)}…` : value;
  return `${head}（${value.length} 字符）`;
}

/** 时间戳 → 本地时刻。 */
function formatTime(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}
