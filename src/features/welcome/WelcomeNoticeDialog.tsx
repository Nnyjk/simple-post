/**
 * 欢迎页的「文档中心」占位提示。
 *
 * 独立成文件是为了让 `WelcomeScreen` 能用 `lazy()` 引它。欢迎页在 entry chunk 里，
 * 而对话框会经 `ModalSurface` 拉进 GSAP（约 38 kB gzip）—— 那是冷启动关键路径上
 * 不该替一个几乎没人点开的按钮付的成本。挂载条件是提示真的打开了，下载也就只发生在
 * 用户点了「查看文档」之后。
 */
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

export function WelcomeNoticeDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      title="文档中心"
      description="完整使用指南与快捷键手册计划在 v0.2 上线，届时这里改为可直达的文档入口。"
      footer={
        <Button variant="ghost" onClick={onClose}>
          知道了
        </Button>
      }
    />
  );
}
