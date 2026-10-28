/**
 * 「跳过证书校验」打开时的那条警示。
 *
 * 设置面板（全局默认）与端点编辑器（端点级覆盖）都要在开关打开时告诉用户这件事的
 * 后果。此前两处各写一份，措辞与出现的条件都可能分叉，故合成这一个组件：调用方传入
 * 开关的取值，组件负责「开关打开就显示这条警示，关着就不显示」。
 *
 * 提成组件还为了让回归网不必依赖浏览器：这一段渲染不读 store，服务端渲染
 * （`renderToStaticMarkup`）就能把开关打开与关掉两种状态都渲染出来。
 */
import { TLS_SKIP_VERIFY } from '@/lib/tls';
import { cn } from '@/lib/utils';

export function SkipVerifyWarning({
  skipVerify,
  className,
}: {
  /** 「跳过证书校验」这个开关的取值。 */
  skipVerify: boolean;
  /** 额外的排版类名，追加在警示自己的样式之后。 */
  className?: string;
}) {
  if (!skipVerify) return null;

  return (
    <p
      className={cn(
        'rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-[11px] text-destructive',
        className,
      )}
    >
      {TLS_SKIP_VERIFY.warning}
    </p>
  );
}
