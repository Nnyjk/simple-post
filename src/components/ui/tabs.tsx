import { type ReactNode } from 'react';
import { cn, focusRing } from '@/lib/utils';

export interface TabItem {
  id: string;
  label: ReactNode;
  badge?: ReactNode;
  disabled?: boolean;
}

interface TabsBaseProps {
  items: TabItem[];
  value: string;
  onChange: (id: string) => void;
  className?: string;
  size?: 'sm' | 'md';
}

export type TabsProps =
  | (TabsBaseProps & { variant?: 'underline'; joinStripClass?: never })
  | (TabsBaseProps & {
      variant: 'connected';
      /**
       * strip 自身的底色——与包住本 `Tabs` 的那个元素、以及它背后面板用的类相同。
       *
       * 必填而非可选，因为圆角缺口就是用它画的：填错会让 tab 两侧肩部露出一个
       * 与周围都不匹配的颜色。它只需要与 strip 一致，且只需要与 strip 一致。
       */
      joinStripClass: string;
    })
  | (TabsBaseProps & { variant: 'segmented'; joinStripClass?: never });

/**
 * 下划线式页签栏——与应用顶部工作区页签条同一视觉语汇，但去掉了 FLIP 动画与关闭
 * 按钮。选中项用 `border-b-2` 下划线而非背景药丸，读起来是导航面（而非分段控件），
 * 也与页签条其余 chrome 对齐。
 *
 * `variant="connected"` 换成浏览器页签语汇：选中项填上面板自身的底色并*压在*分隔线
 * 之上，于是读起来属于下面的面板，而不是浮在上面的控件。肩部见 `TabJoinCorners`。
 *
 * `variant="segmented"` 又是另一层意思：它是*模式切换器*（Pretty / Raw / Headers）而
 * 不是导航。被选的一小组同类项该待在带凸起滑块的实心槽里，而不是文档位置。取自
 * WinShun 的 `ScopeTabs.qml`；几何细节见下方容器处的说明。
 */
export function Tabs({ items, value, onChange, className, size = 'sm', ...rest }: TabsProps) {
  const connected = rest.variant === 'connected';
  const segmented = rest.variant === 'segmented';
  const joinStripClass = rest.variant === 'connected' ? rest.joinStripClass : null;

  return (
    <div
      role="tablist"
      className={cn(
        // strip 自身在底部带一条发丝分隔线，好让选中项的下划线*压在其上*（tab 上用
        // -mb-px），读起来像从面上长出来——VS Code、浏览器、GitHub 文件页签都是
        // 这个模式。
        //
        // `connected` 自己没有这条线：包住 strip 的元素持有整宽分隔线（strip 通常
        // 只和它的标签一样宽，线短一截会读成失误），由选中项盖住。而这种遮盖只在
        // 包裹层*不裁剪*时成立——`overflow-hidden` 裁到 padding box，那在包裹层
        // 边框之上。
        //
        // `segmented` 则是一个自足的槽：它有自己的填充，与下方任何分隔线无关，所以
        // 不能继承另两者需要的 `-mb-px`。
        segmented
          ? 'flex items-center gap-0.5 rounded-lg bg-muted p-[3px]'
          : connected
            ? 'flex items-end gap-0'
            : 'flex items-end gap-0 border-b border-border/60',
        className,
      )}
    >
      {items.map((item) => {
        const active = item.id === value;
        return (
          <button
            key={item.id}
            role="tab"
            aria-selected={active}
            disabled={item.disabled}
            onClick={() => onChange(item.id)}
            className={cn(
              'relative inline-flex items-center gap-1.5 px-3 font-medium transition-colors',
              focusRing,
              'disabled:pointer-events-none disabled:opacity-50',
              size === 'sm' ? 'h-8 text-xs' : 'h-9 text-sm',
              // `-mb-px` 把 tab 抬高一像素压到分隔线上，两者合并成一道连续的笔画——在
              // `underline` 下这条笔画是下划线，在 `connected` 下是 tab 自身的底色。
              // segmented 槽没有分隔线可压
              !segmented && '-mb-px',
              segmented
                ? cn(
                    'rounded-md',
                    // 未选中项退回常规字重，好让这一行上只有选中段带强调；滑块的填充与发丝边负责
                    // 其余视觉
                    active
                      ? 'bg-chip font-semibold text-foreground ring-1 ring-inset ring-border'
                      : 'font-normal text-muted-foreground hover:bg-overlay-hover hover:text-foreground',
                  )
                : connected
                  ? cn(
                      'rounded-t-md',
                      // `z-10` 把选中项的填充抬到它本该抹掉的分隔线之上；未选中项不留痕，
                      // 好让那条线从它们身上连续穿过
                      active
                        ? 'z-10 bg-background text-foreground'
                        : 'text-muted-foreground hover:bg-accent/40 hover:text-foreground',
                    )
                  : cn(
                      'border-b-2',
                      active
                        ? 'border-primary text-foreground'
                        : 'border-transparent text-muted-foreground hover:border-border/60 hover:text-foreground',
                    ),
            )}
          >
            {item.label}
            {item.badge}
            {active && joinStripClass && <TabJoinCorners stripClass={joinStripClass} />}
          </button>
        );
      })}
    </div>
  );
}

/** 圆角缺口的半径。 */
const JOIN_R = 'h-2.5 w-2.5';
/** 停在角点*内侧*的 20px 圆盘，裁进窗口里。 */
const JOIN_DISC = 'absolute h-5 w-5 rounded-full';

/**
 * 让选中项读起来与下方面板*融为一体的*两处圆角缺口。
 *
 * 分隔线若只是停在 tab 两侧，tab 就读成一张摆在页面上的卡片。浏览器是把 tab 的
 * 肩部圆化进那条线，于是轮廓在交界处不断开——这个圆化就是「一整块」的全部效果。
 * 它必须画成*strip 的*颜色而不是交给填充，因为亮色主题下 strip 往往与面板同色，
 * 只有轮廓能把两者分开。
 *
 * 每侧是 tab 底角上一个零溢出的 `JOIN_R` 窗口，里面放一个以该角点为圆心的
 * `JOIN_DISC` 圆：圆落在窗口内的四分之一被涂成 strip 色，于是抹掉 tab 的填充
 * *与*侧边框的一枚四分圆盘——一个凹缺口，边框因此收在曲线上而非直角上。
 *
 * 圆必须被裁进窗口（而不是直接停在角点上），否则它外侧的四分之三会溢出到 tab
 * 之外，在下面的面板上画出可见的半圆盘。
 */
export function TabJoinCorners({ stripClass }: { stripClass: string }) {
  return (
    <>
      <span
        aria-hidden
        className={cn('pointer-events-none absolute bottom-0 left-0 overflow-hidden', JOIN_R)}
      >
        <span className={cn(JOIN_DISC, '-bottom-2.5 -left-2.5', stripClass)} />
      </span>
      <span
        aria-hidden
        className={cn('pointer-events-none absolute bottom-0 right-0 overflow-hidden', JOIN_R)}
      >
        <span className={cn(JOIN_DISC, '-bottom-2.5 -right-2.5', stripClass)} />
      </span>
    </>
  );
}
