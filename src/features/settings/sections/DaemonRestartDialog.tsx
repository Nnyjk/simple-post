import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Server } from 'lucide-react';

/**
 * Blocking confirm that surfaces when the MCP daemon transitions to a
 * `crashed` / `unhealthy` state. Offers two actions:
 *   - Close — dismiss the dialog without restarting; the polling loop
 *     in McpStatusPanel will keep checking, and reopen this dialog if
 *     the daemon stays crashed.
 *   - Restart — invoke the same restart path as McpStatusPanel's
 *     button. The parent passes the handle so we don't duplicate the
 *     "how to restart" logic.
 */
export interface DaemonRestartDialogProps {
  open: boolean;
  onClose: () => void;
  /** Wired to McpStatusPanel.handleRef.restart. */
  onRestart: () => Promise<void> | void;
  /** Optional lastError from the daemon, surfaced in the description. */
  lastError?: string;
}

export function DaemonRestartDialog({
  open,
  onClose,
  onRestart,
  lastError,
}: DaemonRestartDialogProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      title={
        <span className="inline-flex items-center gap-2">
          <Server className="h-4 w-4 text-destructive" />
          MCP daemon 异常退出
        </span>
      }
      description={
        lastError
          ? `daemon 进程已停止运行。错误：${lastError}`
          : 'daemon 进程已停止运行。是否重启？'
      }
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            关闭
          </Button>
          <Button
            onClick={() => {
              void onRestart();
              onClose();
            }}
          >
            重启
          </Button>
        </>
      }
    >
      <p className="text-xs text-muted-foreground">
        重启后 MCP server 会重新注册 stdio 接口；如果持续异常，请检查后端日志或代理二进制路径。
      </p>
    </Dialog>
  );
}
