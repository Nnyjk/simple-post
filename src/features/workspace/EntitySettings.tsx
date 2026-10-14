/**
 * EntitySettings —— 项目 / 模块 / 集合标签背后的面板。
 *
 * 单列，无标签页
 * -------------------
 * 实体面板不该让用户在「这个东西是什么」与「这个东西怎么配置」之间*选择* —— 两者都短到
 * 直接摆着就行。于是面板是自上而下的单条瀑布：先是 基础信息，再是属于该类型的配置。
 *
 * 没有身份表头
 * ------------------
 * 紧邻上方的标签条已经带齐这三样：标签点名实体、图标说明类型。唯一真正丢掉的是类型作为
 * 一个*词*，故标签的 hover 标题现在把它拼出来（`项目 · ttt`），把标签放在眼睛本来就在的
 * 地方，而不是放在第二个会滚走的角落里。
 *
 * 这不只是布局偏好。分层会藏东西：选中 基础信息 时模块的地址不可见，唯一的线索只剩一行
 * 汇总（「1 个环境已填写」），还得再解释一遍。单列意味着模块的环境切换器与 baseUrl 表就在
 * 它名字的下方，滚过去就能看到。
 *
 * 此处没有重新实现的东西
 * -------------------------------
 * 环境 区块就是设置标签所挂载的**同一个** `EnvironmentSection`，经同一个 store 连接的
 * `ProjectEnvSection` 驱动；服务 区块是 `module` 模式的 `ModuleBaseUrlTable`。两个宿主，
 * 一份实现 —— 规则为何住在那里而非这里，见那些文件。
 *
 * 接口标签走 `HttpTester` 而非本组件，故 switch 的 default 分支返回 null。
 */

import { lazy, Suspense, useState, type ReactNode } from 'react';
import { Trash2 } from 'lucide-react';
import { useAppStore, type TabKind } from '@/stores/app-store';
import { Button } from '@/components/ui/button';
import { Tooltip } from '@/components/ui/tooltip';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { AutoSaveInput, AutoSaveTextarea, InfoBlock, InfoField } from './EntityInfoFields';

// 两个配置区块各自是独立 chunk。`EntitySettings` 本身就是 lazy（挂在任意实体标签上），
// 但环境区块会牵进它的切换器、两张表和对话框管线。它们随面板一起挂载而非藏在一次点击
// 之后，故此处 `lazy()` 只是拆 chunk，并不会推迟任何用户滚动时本来就看得见的东西。
const ProjectEnvSection = lazy(() =>
  import('@/features/settings/sections/ProjectEnvSection').then((m) => ({
    default: m.ProjectEnvSection,
  })),
);
const ModuleServiceSection = lazy(() =>
  import('@/features/settings/sections/ModuleServiceSection').then((m) => ({
    default: m.ModuleServiceSection,
  })),
);

interface EntitySettingsProps {
  kind: TabKind;
  entityId: string;
}

export function EntitySettings({ kind, entityId }: EntitySettingsProps) {
  switch (kind) {
    case 'project':
      return <ProjectWorkspace projectId={entityId} />;
    case 'module':
      return <ModuleWorkspace moduleId={entityId} />;
    case 'collection':
      return <CollectionWorkspace collectionId={entityId} />;
    default:
      // 接口标签走 HttpTester。
      return null;
  }
}

// ----------------------------------------------------------------------------
//  Shared chrome
// ----------------------------------------------------------------------------

/**
 * 瀑布中的一个带标题区块。
 *
 * 内缩与 `SettingsCategory` 为环境 区块产出的保持一致，故项目标签读起来是一列，而不是
 * 「我们的区块」外加「他们的区块，缩进还不一样」。
 */
function EntitySection({
  title,
  hint,
  children,
}: {
  title: string;
  /** 标题下的一行。讲一条规则，不复述标题。 */
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section className="px-6 pb-1 pt-5">
      <div className="mb-3">
        <h2 className="label-caps-strong">{title}</h2>
        {hint && <p className="mt-0.5 text-[11px] text-muted-foreground">{hint}</p>}
      </div>
      {children}
    </section>
  );
}

/** 尺寸与它所替代的区块相当的 Suspense 占位。 */
function BlockFallback() {
  return (
    <div className="flex min-h-32 items-center justify-center py-10 text-xs text-muted-foreground">
      加载中…
    </div>
  );
}

// ----------------------------------------------------------------------------
//  项目 — 基础信息 + 环境
// ----------------------------------------------------------------------------

function ProjectWorkspace({ projectId }: { projectId: string }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  if (!project) return <NotFound label="项目" />;

  return (
    <div className="h-full min-h-0 overflow-y-auto pb-6">
      <ProjectInfo projectId={projectId} />
      <EntitySection title="环境" hint="这个项目的环境变量与前置 URL · 与设置里的环境分区是同一份">
        <Suspense fallback={<BlockFallback />}>
          <ProjectEnvSection />
        </Suspense>
      </EntitySection>
      <ProjectDangerZone projectId={projectId} />
    </div>
  );
}

function ProjectInfo({ projectId }: { projectId: string }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  const updateProject = useAppStore((s) => s.updateProject);
  if (!project) return null;

  return (
    <EntitySection title="基础信息">
      <InfoBlock>
        <InfoField label="名称" hint="显示在资源树顶部、项目标签与欢迎页">
          <AutoSaveInput
            value={project.name}
            onSave={(name) => void updateProject(project.id, { name })}
            ariaLabel="项目名称"
            placeholder="项目名称"
          />
        </InfoField>
        <InfoField label="描述" hint="记下这个项目是什么">
          <AutoSaveTextarea
            value={project.description}
            onSave={(description) => void updateProject(project.id, { description })}
            ariaLabel="项目描述"
            placeholder="添加描述…"
          />
        </InfoField>
      </InfoBlock>
    </EntitySection>
  );
}

function ProjectDangerZone({ projectId }: { projectId: string }) {
  const project = useAppStore((s) => s.projects.find((p) => p.id === projectId));
  const deleteProject = useAppStore((s) => s.deleteProject);
  const [confirmDelete, setConfirmDelete] = useState(false);

  if (!project) return null;

  return (
    <div className="px-6 pt-6">
      <div className="flex items-center justify-center border-t border-border pt-4">
        <Tooltip content="删除该项目及其全部内容" side="top">
          <span>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setConfirmDelete(true)}
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
            >
              <Trash2 className="h-3.5 w-3.5" />
              删除项目
            </Button>
          </span>
        </Tooltip>
      </div>

      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={`删除项目「${project.name}」？`}
        description="此操作不可撤销，将同时删除该项目下的全部模块、集合、接口及其环境配置。"
        destructive
        confirmText="删除"
        onConfirm={() => void deleteProject(project.id)}
      />
    </div>
  );
}

// ----------------------------------------------------------------------------
//  模块 — 基础信息 + 服务
// ----------------------------------------------------------------------------

function ModuleWorkspace({ moduleId }: { moduleId: string }) {
  const mod = useAppStore((s) => s.nodes.find((n) => n.id === moduleId));
  const updateNode = useAppStore((s) => s.updateNode);
  if (!mod) return <NotFound label="模块" />;

  return (
    <div className="h-full min-h-0 overflow-y-auto pb-6">
      <EntitySection title="基础信息">
        <InfoBlock>
          <InfoField label="名称" hint="显示在资源树与模块标签">
            <AutoSaveInput
              value={mod.name}
              onSave={(name) => void updateNode(mod.id, { name })}
              ariaLabel="模块名称"
              placeholder="模块名称"
            />
          </InfoField>
          <InfoField label="描述" hint="记下这个模块负责什么">
            <AutoSaveTextarea
              value={mod.description}
              onSave={(description) => void updateNode(mod.id, { description })}
              ariaLabel="模块描述"
              placeholder="添加描述…"
            />
          </InfoField>
        </InfoBlock>
      </EntitySection>

      <EntitySection
        title="服务"
        hint="服务清单属于项目，这里填的是本模块自己的地址 · 各环境分别保存"
      >
        <Suspense fallback={<BlockFallback />}>
          <ModuleServiceSection moduleId={moduleId} />
        </Suspense>
      </EntitySection>
    </div>
  );
}

// ----------------------------------------------------------------------------
//  集合 — 基础信息
// ----------------------------------------------------------------------------

function CollectionWorkspace({ collectionId }: { collectionId: string }) {
  const col = useAppStore((s) => s.nodes.find((n) => n.id === collectionId));
  const updateNode = useAppStore((s) => s.updateNode);
  if (!col) return <NotFound label="集合" />;

  return (
    <div className="h-full min-h-0 overflow-y-auto pb-6">
      <EntitySection title="基础信息">
        <InfoBlock>
          <InfoField label="名称" hint="显示在资源树与集合标签">
            <AutoSaveInput
              value={col.name}
              onSave={(name) => void updateNode(col.id, { name })}
              ariaLabel="集合名称"
              placeholder="集合名称"
            />
          </InfoField>
          <InfoField label="描述" hint="记下这组接口的用途">
            <AutoSaveTextarea
              value={col.description}
              onSave={(description) => void updateNode(col.id, { description })}
              ariaLabel="集合描述"
              placeholder="添加描述…"
            />
          </InfoField>
        </InfoBlock>
      </EntitySection>
    </div>
  );
}

function NotFound({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-0 items-center justify-center bg-card/30 p-8 text-sm text-muted-foreground">
      该{label}已不存在
    </div>
  );
}
