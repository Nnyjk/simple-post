import { InlineEdit } from '@/components/ui/InlineEdit';
import { TagInput } from '@/components/ui/tag-input';
import { cn } from '@/lib/utils';
import type { ResourceNode } from '@/types/domain';

/**
 * 接口头 —— 可改名的标题、标签输入与描述。它承载文档字段，与下面的 URL 栏
 * （承载请求配置）分开。
 */
export function EndpointHeader({
  endpoint,
  isDraft,
  namingDraft,
  onNamingDraftChange,
  tags,
  saveEndpointName,
  saveEndpointDescription,
  patchDraft,
}: {
  endpoint: ResourceNode;
  isDraft: boolean;
  namingDraft: boolean;
  onNamingDraftChange: (next: boolean) => void;
  tags: string[];
  saveEndpointName: (next: string) => void;
  saveEndpointDescription: (next: string) => void;
  patchDraft: (patch: { tags: string[] }) => void;
}) {
  return (
    <div className="flex shrink-0 items-start gap-2 border-b border-border px-4 pb-3 pt-2">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-1.5">
          <InlineEdit
            key={`name-${endpoint.id}`}
            value={endpoint.name}
            onSave={saveEndpointName}
            testId="endpoint-name-input"
            // 只对草稿受控，由 `namingDraft` 在新建接口时打开编辑器。传 `undefined` 则
            // 已保存的接口维持编辑器自身的双击行为，不受影响。
            editing={isDraft ? namingDraft : undefined}
            onEditingChange={(next) => {
              if (isDraft) onNamingDraftChange(next);
            }}
            className="max-w-md text-sm font-semibold"
            display={() => (
              <h1
                className="cursor-text truncate rounded-sm px-1 py-0.5 text-sm font-semibold hover:bg-accent/60"
                title="双击重命名"
              >
                {endpoint.name}
              </h1>
            )}
          />
          <TagInput
            value={tags}
            onChange={(tags) => patchDraft({ tags })}
            placeholder="+ 标签"
            className="min-w-[140px] max-w-[320px] border-transparent bg-transparent px-1 py-0.5 focus-within:ring-0 focus-within:ring-offset-0"
          />
        </div>
        <InlineEdit
          key={`desc-${endpoint.id}`}
          value={endpoint.description ?? ''}
          onSave={saveEndpointDescription}
          // 描述允许清空 —— 否则空草稿会被当成取消，字段保留旧值而不是回到「无描述」。
          commitEmpty
          testId="endpoint-description-input"
          className="mt-0.5 h-6 max-w-2xl text-xs"
          placeholder="一句话说明这个接口的用途…"
          display={() => (
            <p
              className={cn(
                'mt-0.5 cursor-text truncate rounded-sm px-1 text-xs text-muted-foreground',
                'hover:bg-accent/60',
                !endpoint.description && 'italic text-muted-foreground/50',
              )}
              title="双击编辑描述"
            >
              {endpoint.description || '双击添加描述…'}
            </p>
          )}
        />
      </div>
    </div>
  );
}
