import { ChevronRight } from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import type { ResourceNode, Project } from '@/types/domain';

/**
 * 接口的面包屑 —— 回答「我在树的哪里」。
 *
 * 与设置面板的 `设置 › 连接` 用同一套分隔符写法。工作区标签承载同样信息时隐藏
 * （由调用方按 `endpointHasTab` 决定）。
 */
export function EndpointBreadcrumb({
  project,
  ancestors,
  endpoint,
}: {
  project: Project | undefined;
  ancestors: { id: string; name: string; type: string }[];
  endpoint: ResourceNode;
}) {
  return (
    <nav
      aria-label="接口所属路径"
      className="flex shrink-0 items-center gap-1 px-4 pt-5 text-[11px] text-muted-foreground"
    >
      {project && ancestors.length > 0 ? (
        <>
          <button
            type="button"
            onClick={() => {
              const id = useAppStore.getState().activeProjectId;
              // 用 `openProject` 而非 `setActiveProject`：面包屑首节是到项目的链接，
              // 应带出项目面板（并重新界定树的范围），而不只是重解析环境。
              if (id) useAppStore.getState().openProject(id);
            }}
            className="truncate hover:text-foreground"
            title={project.name}
          >
            {project.name}
          </button>
          {ancestors.map((a) => (
            <span key={a.id} className="contents">
              <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
              <span className="truncate" title={a.name}>
                {a.name}
              </span>
            </span>
          ))}
          <ChevronRight className="h-3 w-3 shrink-0 text-muted-foreground/50" />
          <span className="truncate text-foreground" title={endpoint.name}>
            {endpoint.name}
          </span>
        </>
      ) : (
        <span>接口</span>
      )}
    </nav>
  );
}
