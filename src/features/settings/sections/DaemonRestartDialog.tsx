import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Server } from 'lucide-react';

/**
 * MCP daemon 转入 `crashed` / `unhealthy` 时弹出的阻塞式确认框。两个动作：
 *   - 关闭 —— 不重启就关掉；McpStatusPanel 里的轮询会继续检查，daemon 持续崩溃时会再次
 *     弹出本框。
 *   - 重启 —— 走与 McpStatusPanel 按钮相同的重启路径。句柄由父级传入，好让「怎么重启」
 *     的逻辑不被复制。
 */
export interface DaemonRestartDialogProps {
  open: boolean;
  onClose: () => void;
  /** 接到 McpStatusPanel.handleRef.restart。 */
  onRestart: () => Promise<void> | void;
  /** daemon 的可选 lastError，显示在描述里。 */
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
