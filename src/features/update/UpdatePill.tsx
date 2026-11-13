/**
 * 顶栏的更新入口：一枚小圆点 + 目标版本号。点开更新对话框。
 *
 * 不占位是硬约束：没有新版、检查失败、用户已经忽略了这个版本，三种情况下本组件返回
 * `null` —— 顶栏里不会留下一个空槽，其它控件的位置也不会跟着跳。
 */
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { useAppStore } from '@/stores/app-store';
import { UpdateDialog } from './UpdateDialog';

export function UpdatePill() {
  const info = useAppStore((s) => s.updateInfo);
  const dismissedVersion = useAppStore((s) => s.dismissedVersion);
  const [open, setOpen] = useState(false);

  const latest = info?.latest ?? null;
  // 三件事同时成立才是一个值得打断的入口：真的查到了新版本、版本号可读、用户没说过「稍后」。
  if (!info?.available || !latest || latest === dismissedVersion) return null;

  return (
    <>
      <Tooltip content="查看新版本" side="bottom">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setOpen(true)}
          aria-label={`有新版本 v${latest}`}
          className="gap-1.5"
          data-testid="update-available"
        >
          <span className="h-1.5 w-1.5 rounded-full bg-primary" aria-hidden="true" />
          <span className="font-mono text-[11px]">v{latest}</span>
        </Button>
      </Tooltip>
      <UpdateDialog open={open} onOpenChange={setOpen} />
    </>
  );
}
