/**
 * 「导出接口文档」弹窗：挑接口 → 只读预览 → 保存。
 *
 * 结构照 `TransferDialog`：同一个 `Dialog`（标题 / 说明 / 正文 / 页脚），同一套工具条小字按钮，
 * 连勾选树都是同一个 `TransferTree` —— 用户在这里看到的分组、METHOD 与路径，跟导入/导出那屏
 * 一模一样，不需要再学一套。
 *
 * 只读预览是一段 `srcDoc` + `sandbox=""` 的 iframe：**没有** `allow-scripts`，于是产物里就算
 * 混进了脚本也跑不起来；iframe 里也没有任何编辑入口。它同时是「打开后的样子」的保真预览——
 * 快照本身就是这个文件。
 */
import { FileDown, RotateCcw } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Dialog } from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';
import { cn, focusRingBare } from '@/lib/utils';
import { TransferTree } from '@/features/interchange/TransferTree';
import type { DocsShareController } from './useDocsShare';

export function DocsShareDialog({ controller }: { controller: DocsShareController }) {
  const { target, phase, busy, error, close, back } = controller;
  if (!target) return null;

  return (
    <Dialog
      open
      // 勾掉的选择与已生成的快照都在这里，误点背景丢掉它们太亏。
      disableBackdropClose
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      title={`导出接口文档 · 模块「${target.moduleName}」`}
      description={
        phase === 'select'
          ? '选中的接口会被冻结进一份自包含 HTML 快照。数据源是已保存的定义——刚改完还没保存的内容不会进来。'
          : '这是生成那一刻的快照：之后在工作区里怎么改，都不会影响这份文件。'
      }
      className="max-w-2xl"
      footer={
        phase === 'select' ? (
          <>
            <Button variant="ghost" size="sm" disabled={busy} onClick={close}>
              取消
            </Button>
            <Button
              size="sm"
              disabled={busy || controller.chosen === 0}
              onClick={() => void controller.generate()}
            >
              {busy ? '生成中…' : `生成文档（${controller.chosen} 个接口）`}
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" size="sm" disabled={busy} onClick={back}>
              <RotateCcw className="h-3.5 w-3.5" />
              重新选择
            </Button>
            <Button size="sm" disabled={busy} onClick={() => void controller.save()}>
              <FileDown className="h-3.5 w-3.5" />
              {busy ? '保存中…' : '保存 HTML'}
            </Button>
          </>
        )
      }
    >
      {error && (
        <p className="mb-3 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-[11px] text-destructive">
          {error}
        </p>
      )}
      {phase === 'select' ? (
        <PickStep controller={controller} />
      ) : (
        <PreviewStep controller={controller} />
      )}
    </Dialog>
  );
}

function PickStep({ controller }: { controller: DocsShareController }) {
  const { groups, selected, chosen, total } = controller;
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Badge variant="muted">{total} 个接口</Badge>
        <div className="flex-1" />
        <ToolbarButton onClick={controller.selectAll}>全选</ToolbarButton>
        <ToolbarButton onClick={controller.selectNone}>全不选</ToolbarButton>
        <ToolbarButton onClick={controller.invert}>反选</ToolbarButton>
        <span className="ml-1 text-[11px] tabular-nums text-muted-foreground">
          已选 {chosen} / {total}
        </span>
      </div>

      <div className="max-h-[52vh] overflow-y-auto rounded-md border border-border bg-background/40">
        {total === 0 ? (
          <p className="px-3 py-6 text-center text-[11px] text-muted-foreground">
            这个模块里还没有已保存的接口。
          </p>
        ) : (
          <TransferTree
            groups={groups}
            selected={selected}
            onToggleEndpoint={controller.toggleEndpoint}
            onToggleGroup={controller.toggleGroup}
          />
        )}
      </div>

      <p className="text-[11px] leading-relaxed text-muted-foreground">
        文档只写名字、说明与结构：请求头 / 查询参数 / 路径参数 / 鉴权 / 环境变量都
        <span className="font-medium text-foreground">不会输出取值</span>
        ，响应里你写的
        <code className="mx-0.5 rounded-sm bg-muted px-1">example</code>
        会原样输出。
      </p>
    </div>
  );
}

function PreviewStep({ controller }: { controller: DocsShareController }) {
  const { doc, savedName } = controller;
  if (!doc) return null;
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="muted">{doc.endpoints} 个接口</Badge>
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">
          {doc.fileName}
        </span>
      </div>

      <iframe
        title="接口文档预览"
        // 空 sandbox = 最严：脚本、表单、同源全部关掉。产物本身也不含脚本，这里是第二道锁。
        sandbox=""
        srcDoc={doc.html}
        className="h-[52vh] w-full rounded-md border border-border bg-white"
      />

      <p
        className={cn(
          'text-[11px] leading-relaxed',
          savedName ? 'text-foreground' : 'text-muted-foreground',
        )}
      >
        {savedName
          ? `已保存 ${savedName}。这份文件删掉工作区也回不来——它只是那一刻的拷贝。`
          : '预览里没有任何编辑入口，脚本被禁用。保存后得到的就是你看到的这份文件。'}
      </p>
    </div>
  );
}

/** 与 `TransferDialog` 里同名的工具条按钮同一形状：小字、ghost、不吃视觉主位。 */
function ToolbarButton({ children, onClick }: { children: React.ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'rounded-sm px-1.5 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground',
        focusRingBare,
      )}
    >
      {children}
    </button>
  );
}
