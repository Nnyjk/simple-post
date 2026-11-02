import { type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { IconTile } from '@/components/ui/icon-tile';

interface SettingsCategoryProps {
  /**
   * @deprecated 锚点 id 不再渲染（抽屉改用标签页）。保留仅为兼容既有调用点。
   */
  id?: string;
  title?: string;
  description?: string;
  /**
   * 小节标题的前导图标，与标题配对让每个分类有个可辨认的标志 —— 该图标同时是侧栏的
   *   颜色锚点，故两个视图是同一份真相。
   */
  icon?: ReactNode;
  /**
   * 标题末尾的元素，通常是「恢复默认」「添加变量」这类主动作。渲染为标题块右侧的 flex
   *   子项；预留出按钮所需的纵向空间而不撑高整行。
   */
  actions?: ReactNode;
  /**
   * 描述下方的次级行，如「3 项 · 影响所有 HTTP 请求」。与描述同用弱化字号，色调再低
   *   一档。
   */
  meta?: ReactNode;
  /**
   * 为 true 时本小节不渲染自己的下边框。末尾的分隔线（或不留）由外层列表
   * （`SettingsPanel`）负责，免得滚动区底部多出一条孤线。
   */
  hideDivider?: boolean;
  /**
   * 内容变体 —— `stacked`（默认）给子元素加纵向间距，`card` 把子元素包进一张圆角卡片
   *   壳（用于本小节*就是*一个完整表面的场合，如环境面板）。
   */
  variant?: 'stacked' | 'card';
  children: ReactNode;
}

/**
 * 设置标签详情页中的一个条目。
 *
 * 结构：表头（图标 + 小标题 + 可选弱化描述）后接内容井。负的横向 margin 把外层分隔线
 * （若有）拉到主面板两缘，内层 `px-12` 则让正文保持舒适的内缩。
 *
 * 内容默认是柔和卡片（`variant="card"`），使每个设置分组读起来是一个完整表面；子元素
 * 已经是自足的卡片或行、只差一个表头时，传 `variant="stacked"`。
 */
export function SettingsCategory({
  title,
  description,
  icon,
  actions,
  meta,
  hideDivider = false,
  variant = 'card',
  children,
}: SettingsCategoryProps) {
  const hasHeader = !!title || !!description || !!icon || !!meta || !!actions;
  return (
    <section
      className={cn(
        // 外层块：把底部分隔线拉到面板两缘，正文留在舒适的阅读列上。48px 的左右内缩与
        // 设置标签其余部分一致。
        '-mx-6 px-12 py-5',
        !hideDivider && 'border-b border-border/60',
      )}
    >
      {hasHeader && (
        <header className="mb-3 flex items-start gap-3">
          {icon && <IconTile icon={icon} className="mt-0.5" />}
          <div className="min-w-0 flex-1">
            {title && (
              <h2 className="text-sm font-semibold tracking-tight text-foreground">{title}</h2>
            )}
            {(description || meta) && (
              <p className="mt-0.5 text-[11px] text-muted-foreground">
                {description}
                {description && meta ? ' · ' : ''}
                {meta}
              </p>
            )}
          </div>
          {actions && <div className="shrink-0">{actions}</div>}
        </header>
      )}
      {variant === 'card' ? (
        <div className="rounded-lg border border-border/70 bg-card/40 shadow-[0_1px_0_0_rgba(255,255,255,0.02)_inset]">
          {children}
        </div>
      ) : (
        <div className="space-y-3">{children}</div>
      )}
    </section>
  );
}

// ----------------------------------------------------------------
// SettingRow —— SettingsCategory 内的单条键值行。
//
// 布局：
//   [ icon ]  Label                        [ control ]   ← 第 1 行
//             description text                       hint  ← 第 2 行（可选）
//   ──────────────────────────────────────────────────────  ← 分隔线（可选）
//
// 作为 `variant="card"` 的 `SettingsCategory` 的直接子元素使用 —— 每一行自带一条浅分隔
// 线（上边框），故它们在卡片壳里读起来是一个纵向列表。
// ----------------------------------------------------------------

export interface SettingRowProps {
  /** 可选的前导图标 —— 通常与小节表头同一个，保持小巧以免与控件争夺注意力。 */
  icon?: ReactNode;
  label: string;
  description?: ReactNode;
  /** 右对齐的控件（输入框 / 开关 / 选择器 / 按钮组）。 */
  control: ReactNode;
  /** 控件下方的可选副标签，如「默认 30000ms」或「v0.2」。 */
  hint?: ReactNode;
  /** 可选的末尾徽标，如状态药丸。 */
  badge?: ReactNode;
  /** 视觉强调 —— `danger` 把标签染成破坏色，用于不可撤销或删除类动作。 */
  tone?: 'default' | 'danger';
  /** 为 true 时隐藏本行的上边框（用于卡片内的首行，免得顶部多出一条孤线）。 */
  flush?: boolean;
  disabled?: boolean;
}

export function SettingRow({
  icon,
  label,
  description,
  control,
  hint,
  badge,
  tone = 'default',
  flush = false,
  disabled = false,
}: SettingRowProps) {
  return (
    <div
      className={cn(
        'flex items-start gap-4 px-4 py-3',
        !flush && 'border-t border-border/60 first:border-t-0',
        disabled && 'opacity-60',
      )}
    >
      {icon && (
        <IconTile icon={icon} className="mt-0.5" tone={tone === 'danger' ? 'danger' : 'default'} />
      )}
      <div className="min-w-0 flex-1">
        <div
          className={cn(
            'text-xs font-medium',
            tone === 'danger' ? 'text-destructive' : 'text-foreground',
          )}
        >
          {label}
          {badge && <span className="ml-2 align-middle">{badge}</span>}
        </div>
        {(description || hint) && (
          <div className="mt-0.5 text-[11px] text-muted-foreground">
            {description}
            {description && hint ? (
              <>
                {' · '}
                <span className="text-muted-foreground/80">{hint}</span>
              </>
            ) : (
              hint
            )}
          </div>
        )}
      </div>
      <div className="shrink-0">{control}</div>
    </div>
  );
}
