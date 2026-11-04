import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { HUE, type HueConcept } from '@/lib/hue';

/**
 * IconTile —— 图标所坐的那块着色方块。尺寸档（调用方按最近的档选）：xs 24px（设置侧栏、
 * 环境切换器）、sm 20px（资源树行、GroupHeader）、md 28px（设置行 / 分类头）、lg 36px
 * （表单行）、xl 40px（ConnectionSection 的 hero 卡片）。
 *
 * 概念色相（`concept`）表达身份，`tone` 表达状态或应用的主要上下文（environment），
 * 两者不是一回事，也不要混用。
 */

export type IconTileSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl';

const SIZE_CLASS: Record<IconTileSize, string> = {
  xs: 'h-6 w-6',
  sm: 'h-5 w-5',
  md: 'h-7 w-7',
  lg: 'h-9 w-9',
  xl: 'h-10 w-10',
};

/**
 * tone → 上色。`default` 是唯一能被 `concept` 覆盖的条目；其余四种是 tile 在*报告的状态*，
 * 不是它代表的身份，所以谁也不许改画它们。
 *
 * `success` / `warning` 取 `-foreground` 那个 token 而非基础 token：15% alpha 的淡色底上，
 * 基础值作为描边可读，作为浅色主题铺在其上的近白底色里的字形就太淡了。
 *
 * `danger` 刻意不守这条规则：`--destructive-foreground` 是**实心**填色上的文字档，配淡色底
 * 在亮色模式下就是白底白字。
 */
const TONE: Record<NonNullable<IconTileProps['tone']>, { bg: string; text: string }> = {
  default: { bg: 'bg-muted/60', text: 'text-muted-foreground' },
  danger: { bg: 'bg-destructive/10', text: 'text-destructive' },
  primary: { bg: 'bg-primary/15', text: 'text-primary' },
  success: { bg: 'bg-success/15', text: 'text-success-foreground' },
  warning: { bg: 'bg-warning/15', text: 'text-warning-foreground' },
};

const PRIMARY_BORDER = 'border-primary/30';
const NEUTRAL_BORDER = 'border-border';

export interface IconTileProps {
  icon: ReactNode;
  /**
   * 这个 tile 代表哪个语义概念。省略即为中性 tile（设置行的静止态）。
   * 见 `src/lib/hue.ts`。
   */
  concept?: HueConcept;
  size?: IconTileSize;
  /** 加上概念的 30% alpha 描边。表单尺寸的行用。 */
  bordered?: boolean;
  /**
   * 用语义状态 token 上色，而不是概念色相。
   *
   * `primary` 供那两个代表 *environment* 的 tile 用——environment 刻意不占色相
   * （见 `src/lib/hue.ts`），它保留 `--primary` 是因为那是应用的主要上下文。
   *
   * `success` / `warning` / `danger` 给报告某个运行中事物*状态*的 tile。见
   * `src/lib/mcp-status-tone.ts`。
   */
  tone?: 'default' | 'danger' | 'primary' | 'success' | 'warning';
  /** 设置侧栏上的中性 tile 也带一条发丝边。 */
  neutralBorder?: boolean;
  className?: string;
  /** 镜像到包裹元素上，供「对读屏隐藏」的调用点使用。 */
  'aria-hidden'?: boolean;
}

export function IconTile({
  icon,
  concept,
  size = 'md',
  bordered = false,
  tone = 'default',
  neutralBorder = false,
  className,
  ...rest
}: IconTileProps) {
  const hue = tone === 'default' && concept ? HUE[concept] : null;
  // 概念色相压过中性静止色调，任何显式 tone 再压过两者。一次查表，所以没有调用方
  // 会拿到 hue 的 `bg` 配 tone 的 `text`
  const paint = hue ?? TONE[tone];
  return (
    <span
      aria-hidden={rest['aria-hidden'] ?? true}
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-md',
        SIZE_CLASS[size],
        paint.bg,
        paint.text,
        bordered && tone === 'primary' ? PRIMARY_BORDER : bordered && hue ? hue.border : null,
        neutralBorder ? NEUTRAL_BORDER : null,
        className,
      )}
    >
      {icon}
    </span>
  );
}
