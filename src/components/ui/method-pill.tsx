/**
 * MethodPill —— 代表 HTTP 动词的那枚小彩色徽标。
 *
 * 两档尺寸覆盖全部展示位置：
 *   `xs` → 16×16 单字母小片，用在资源树 endpoint 条目这类紧凑行里。只显示动词
 *          首字母——放不下更多，颜色承担主要识别。
 *   `sm` → 正常标签尺寸的药丸，放得下完整动词（"POST" / "DELETE"）。用在 HttpTester
 *          的 URL 栏、下拉行、未保存关闭对话框等处。
 *
 * 两种视觉口味：
 *   `solid`  → 满色底 + 白字。一眼可见的最高对比度——URL 栏里当前动词首选
 *              （读作主要的「动词控件」）。
 *   `tinted` → 该色以 15% alpha 作底 + 同色文字。视觉重量更低，用在下拉行与树行，
 *              那里一枚饱和药丸会比行内容更抢眼。
 *
 * 同一个 method 在任何尺寸/口味下都是同一个色相。HSL → alpha-15 的映射放在
 * `methodTint()` 里，好让邻近代码（如下拉行背景）复用而不必重新实现。
 */

import type { HttpMethod } from '@/types/domain';
import { cn, methodColorVar } from '@/lib/utils';

export type MethodPillSize = 'xs' | 'sm';
export type MethodPillVariant = 'solid' | 'tinted';

export interface MethodPillProps {
  method: HttpMethod;
  /** 单字母方块还是完整标签。默认 `sm`（完整标签）。 */
  size?: MethodPillSize;
  /** 视觉口味。默认 `solid`（最高对比）。 */
  variant?: MethodPillVariant;
  /** 隐藏文字，只留色块 + aria-label。 */
  hideLabel?: boolean;
  className?: string;
  /**
   * 覆盖显示的文字。默认是完整 method（`size="xs"` 时为首字母）。
   */
  label?: string;
  title?: string;
}

const SIZE_XS = 'h-4 w-4 rounded text-[10px]';
const SIZE_SM = 'h-6 min-w-[3.25rem] rounded-md px-1.5 text-[10px] tracking-wider';

const VARIANT_TINTED = 'border';

export function MethodPill({
  method,
  size = 'sm',
  variant = 'solid',
  hideLabel = false,
  className,
  label,
  title,
}: MethodPillProps) {
  const text = label ?? (size === 'xs' ? method.charAt(0).toUpperCase() : method);

  // solid 变体把颜色直接当背景配白字；tinted 变体用该色 15% alpha 作底、满饱和作
  // 字色——见下方 `methodTint`
  const style =
    variant === 'tinted'
      ? methodTint(method)
      : { backgroundColor: methodColorVar(method), color: 'white' };

  return (
    <span
      role="img"
      aria-label={method}
      title={title ?? method}
      className={cn(
        'inline-flex shrink-0 items-center justify-center font-mono font-bold uppercase tabular-nums',
        size === 'xs' ? SIZE_XS : SIZE_SM,
        variant === 'tinted' && VARIANT_TINTED,
        className,
      )}
      style={style}
    >
      {hideLabel ? '' : text}
    </span>
  );
}

/**
 * 着色的 "tinted" 一组：15% alpha 底 + 满饱和字色 + 30% alpha 边框（药丸边缘在
 * hover/focus 时仍看得见）。
 *
 * `methodColorVar` 返回不带逗号的现代 HSL 三元组（如 `hsl(142 71% 45%)`）。这里用
 * 斜杠语法补上 alpha 通道——现代浏览器（2023 起）在任何接受颜色的属性上都认。
 */
function methodTint(method: HttpMethod): {
  backgroundColor: string;
  color: string;
  borderColor: string;
} {
  const inner = methodColorVar(method).replace(/^hsl\(|\)$/g, '');
  return {
    backgroundColor: `hsl(${inner} / 0.15)`,
    color: `hsl(${inner})`,
    borderColor: `hsl(${inner} / 0.3)`,
  };
}
