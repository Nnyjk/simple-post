import { useEffect, useMemo, useRef, type FormEvent } from 'react';
import { Package, FolderPlus, FilePlus } from 'lucide-react';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { useAppStore, type NewType } from '@/stores/app-store';
import { cn } from '@/lib/utils';

const KIND_LABEL: Record<NewType, string> = {
  project: '项目',
  module: '模块',
  collection: '集合',
  endpoint: '接口',
};

const PROJECT_COLORS = ['#60a5fa', '#a78bfa', '#34d399', '#fbbf24'];

const TYPE_CHIPS: { type: NewType; label: string; icon: React.ReactNode }[] = [
  { type: 'project', label: '项目', icon: <Package className="h-4 w-4" /> },
  { type: 'module', label: '模块', icon: <FolderPlus className="h-4 w-4" /> },
  { type: 'collection', label: '集合', icon: <FolderPlus className="h-4 w-4" /> },
  { type: 'endpoint', label: '接口', icon: <FilePlus className="h-4 w-4" /> },
];

/**
 * Global "new item" dialog.
 *
 * Renders into the DOM as long as the parent renders it, but the actual
 * `Dialog` is only mounted (and portals) when `newItem.open === true`.
 *
 * Two stages:
 *   1. `type === null`  — type chooser (4 chips).
 *   2. `type !== null`  — form fields for that type; chips become a
 *                         readonly breadcrumb with the current type
 *                         highlighted.
 *
 * The submit guard is intentionally strict: name must be non-empty
 * (after trim) and the appropriate parent (module / collection) must
 * be present. The footer "创建" button is disabled while invalid.
 *
 * On successful submit:
 *   - `submitNewItem()` is called, which writes the new row to the
 *     store and sets `lastCreatedId`.
 *   - The modal closes (state reset by `closeNewItem`).
 *   - External consumers (e.g. ProjectTree) watch `lastCreatedId`
 *     and decide whether to enter inline-rename mode. This component
 *     does NOT touch `editing` — that's owned by each entry surface.
 */
export function NewItemModal() {
  // ---- store reads (raw refs only — no derived arrays) ----
  const newItem = useAppStore((s) => s.newItem);
  const modules = useAppStore((s) => s.modules);
  const collections = useAppStore((s) => s.collections);
  const activeProjectId = useAppStore((s) => s.activeProjectId);

  const closeNewItem = useAppStore((s) => s.closeNewItem);
  const setNewItemType = useAppStore((s) => s.setNewItemType);
  const setNewItemName = useAppStore((s) => s.setNewItemName);
  const setNewItemDescription = useAppStore((s) => s.setNewItemDescription);
  const setNewItemBaseUrl = useAppStore((s) => s.setNewItemBaseUrl);
  const setNewItemColor = useAppStore((s) => s.setNewItemColor);
  const setNewItemParentModule = useAppStore((s) => s.setNewItemParentModule);
  const setNewItemParentCollection = useAppStore((s) => s.setNewItemParentCollection);
  const submitNewItem = useAppStore((s) => s.submitNewItem);

  // ---- derived (useMemo; never inside a selector) ----
  const projectModules = useMemo(
    () => modules.filter((m) => m.projectId === activeProjectId).sort((a, b) => a.sortOrder - b.sortOrder),
    [modules, activeProjectId],
  );
  const moduleCollections = useMemo(
    () =>
      collections
        .filter((c) => c.moduleId === newItem.parentModuleId)
        .sort((a, b) => a.sortOrder - b.sortOrder),
    [collections, newItem.parentModuleId],
  );

  // ---- auto-fill parents when type / module change ----
  // collection: default parentModuleId to first projectModule if unset.
  useEffect(() => {
    if (newItem.type !== 'collection') return;
    if (newItem.parentModuleId) return;
    if (projectModules.length === 0) return;
    setNewItemParentModule(projectModules[0].id);
  }, [newItem.type, newItem.parentModuleId, projectModules, setNewItemParentModule]);

  // endpoint: default parentModuleId to first projectModule, then
  // parentCollectionId to first collection within that module.
  useEffect(() => {
    if (newItem.type !== 'endpoint') return;
    if (!newItem.parentModuleId && projectModules.length > 0) {
      setNewItemParentModule(projectModules[0].id);
      return;
    }
    if (newItem.parentModuleId && !newItem.parentCollectionId && moduleCollections.length > 0) {
      setNewItemParentCollection(moduleCollections[0].id);
    }
  }, [
    newItem.type,
    newItem.parentModuleId,
    newItem.parentCollectionId,
    projectModules,
    moduleCollections,
    setNewItemParentModule,
    setNewItemParentCollection,
  ]);

  // endpoint: when parentModuleId changes, snap parentCollectionId to
  // the first collection in the new module (or clear if none).
  useEffect(() => {
    if (newItem.type !== 'endpoint' || !newItem.parentModuleId) return;
    const stillValid = moduleCollections.some((c) => c.id === newItem.parentCollectionId);
    if (stillValid) return;
    setNewItemParentCollection(moduleCollections[0]?.id ?? '');
  }, [
    newItem.type,
    newItem.parentModuleId,
    moduleCollections,
    newItem.parentCollectionId,
    setNewItemParentCollection,
  ]);

  // ---- submit guard ----
  const canSubmit = useMemo(() => {
    if (!newItem.type) return false;
    if (!newItem.name.trim()) return false;
    if (newItem.type === 'collection' || newItem.type === 'endpoint') {
      if (projectModules.length === 0) return false;
    }
    if (newItem.type === 'collection' && !newItem.parentModuleId) return false;
    if (newItem.type === 'endpoint' && !newItem.parentCollectionId) return false;
    return true;
  }, [newItem, projectModules]);

  // ---- focus management ----
  const nameInputRef = useRef<HTMLInputElement>(null);
  const firstChipRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!newItem.open) return;
    const t = window.setTimeout(() => {
      if (newItem.type === null) {
        firstChipRef.current?.focus();
      } else {
        nameInputRef.current?.focus();
      }
    }, 0);
    return () => window.clearTimeout(t);
  }, [newItem.open, newItem.type]);

  // ---- submit handler ----
  const handleSubmit = (e?: FormEvent) => {
    if (e) e.preventDefault();
    if (!canSubmit) return;
    submitNewItem();
    closeNewItem();
  };

  const isStage1 = newItem.type === null;
  const title = isStage1 ? '新建' : `新建${KIND_LABEL[newItem.type!]}`;

  const moduleOptions = useMemo(
    () => projectModules.map((m) => ({ value: m.id, label: m.name })),
    [projectModules],
  );
  const collectionOptions = useMemo(
    () => moduleCollections.map((c) => ({ value: c.id, label: c.name })),
    [moduleCollections],
  );

  return (
    <Dialog
      open={newItem.open}
      onOpenChange={(o) => {
        if (!o) closeNewItem();
      }}
      title={title}
      description={isStage1 ? '选择要创建的类型' : undefined}
      className="max-w-lg"
      footer={
        isStage1 ? undefined : (
          <>
            <Button variant="ghost" size="sm" onClick={closeNewItem}>
              取消
            </Button>
            <Button size="sm" onClick={() => handleSubmit()} disabled={!canSubmit}>
              创建
            </Button>
          </>
        )
      }
    >
      <form onSubmit={handleSubmit} className="space-y-4">
        {/* Stage 1 + Stage 2 chip row (chip in stage 2 is read-only) */}
        <div className="flex flex-wrap items-center gap-2">
          {TYPE_CHIPS.map((chip, idx) => {
            const isActive = newItem.type === chip.type;
            const isInactiveOther = !isStage1 && newItem.type !== null && !isActive;
            return (
              <Button
                key={chip.type}
                ref={idx === 0 ? firstChipRef : undefined}
                type="button"
                size="sm"
                variant={isActive ? 'default' : 'outline'}
                disabled={isInactiveOther}
                aria-pressed={isActive}
                data-type-chip={chip.type}
                onClick={() => {
                  // In stage 1, always allow switching. In stage 2, also
                  // allow switching — the modal just changes which form
                  // is shown. (Reset is done by openNewItem on next open.)
                  if (isInactiveOther) return;
                  setNewItemType(chip.type);
                }}
                className={cn(isInactiveOther && 'cursor-default')}
              >
                {chip.icon}
                <span>{chip.label}</span>
              </Button>
            );
          })}
        </div>

        {newItem.type === 'project' && (
          <ProjectFields
            newItem={newItem}
            onNameChange={setNewItemName}
            onDescriptionChange={setNewItemDescription}
            onBaseUrlChange={setNewItemBaseUrl}
            onColorChange={setNewItemColor}
            nameInputRef={nameInputRef}
          />
        )}
        {newItem.type === 'module' && (
          <ModuleFields
            newItem={newItem}
            onNameChange={setNewItemName}
            onDescriptionChange={setNewItemDescription}
            nameInputRef={nameInputRef}
          />
        )}
        {newItem.type === 'collection' && (
          <CollectionFields
            newItem={newItem}
            onNameChange={setNewItemName}
            onDescriptionChange={setNewItemDescription}
            onParentModuleChange={setNewItemParentModule}
            moduleOptions={moduleOptions}
            disabledModules={projectModules.length === 0}
            nameInputRef={nameInputRef}
          />
        )}
        {newItem.type === 'endpoint' && (
          <EndpointFields
            newItem={newItem}
            onNameChange={setNewItemName}
            onParentModuleChange={setNewItemParentModule}
            onParentCollectionChange={setNewItemParentCollection}
            moduleOptions={moduleOptions}
            collectionOptions={collectionOptions}
            disabledModules={projectModules.length === 0}
            nameInputRef={nameInputRef}
          />
        )}

        {newItem.type === 'collection' && projectModules.length === 0 && (
          <p className="text-xs text-muted-foreground">
            当前项目还没有模块，请先新建一个模块。
          </p>
        )}
        {newItem.type === 'endpoint' && projectModules.length === 0 && (
          <p className="text-xs text-muted-foreground">
            当前项目还没有模块 / 集合，请先新建模块和集合。
          </p>
        )}
        {newItem.type === 'endpoint' &&
          projectModules.length > 0 &&
          moduleCollections.length === 0 && (
            <p className="text-xs text-muted-foreground">所选模块下还没有集合，请先新建集合。</p>
          )}
      </form>
    </Dialog>
  );
}

// ---------- field sub-components ----------

interface NewItemNameProps {
  newItem: ReturnType<typeof useAppStore.getState>['newItem'];
  onNameChange: (n: string) => void;
  nameInputRef: React.RefObject<HTMLInputElement>;
}

function NameField({ newItem, onNameChange, nameInputRef, autoFocus }: NewItemNameProps & { autoFocus?: boolean }) {
  return (
    <Field label="名称" required>
      <Input
        ref={nameInputRef}
        value={newItem.name}
        onChange={(e) => onNameChange(e.target.value)}
        placeholder="输入名称"
        autoFocus={autoFocus}
        data-testid="new-item-name"
      />
    </Field>
  );
}

function ProjectFields(props: {
  newItem: ReturnType<typeof useAppStore.getState>['newItem'];
  onNameChange: (n: string) => void;
  onDescriptionChange: (d: string) => void;
  onBaseUrlChange: (u: string) => void;
  onColorChange: (c: string) => void;
  nameInputRef: React.RefObject<HTMLInputElement>;
}) {
  return (
    <div className="space-y-3">
      <NameField {...props} autoFocus />
      <Field label="描述">
        <Input
          value={props.newItem.description}
          onChange={(e) => props.onDescriptionChange(e.target.value)}
          placeholder="一句话说明（可选）"
        />
      </Field>
      <Field label="Base URL">
        <Input
          value={props.newItem.baseUrl}
          onChange={(e) => props.onBaseUrlChange(e.target.value)}
          placeholder="https://api.example.com"
        />
      </Field>
      <Field label="颜色">
        <ColorPicker value={props.newItem.color} onChange={props.onColorChange} />
      </Field>
    </div>
  );
}

function ModuleFields(props: {
  newItem: ReturnType<typeof useAppStore.getState>['newItem'];
  onNameChange: (n: string) => void;
  onDescriptionChange: (d: string) => void;
  nameInputRef: React.RefObject<HTMLInputElement>;
}) {
  return (
    <div className="space-y-3">
      <NameField {...props} autoFocus />
      <Field label="描述">
        <Input
          value={props.newItem.description}
          onChange={(e) => props.onDescriptionChange(e.target.value)}
          placeholder="一句话说明（可选）"
        />
      </Field>
    </div>
  );
}

function CollectionFields(props: {
  newItem: ReturnType<typeof useAppStore.getState>['newItem'];
  onNameChange: (n: string) => void;
  onDescriptionChange: (d: string) => void;
  onParentModuleChange: (id: string) => void;
  moduleOptions: { value: string; label: string }[];
  disabledModules: boolean;
  nameInputRef: React.RefObject<HTMLInputElement>;
}) {
  return (
    <div className="space-y-3">
      <NameField {...props} autoFocus />
      <Field label="描述">
        <Input
          value={props.newItem.description}
          onChange={(e) => props.onDescriptionChange(e.target.value)}
          placeholder="一句话说明（可选）"
        />
      </Field>
      <Field label="所属模块" required>
        <Select
          value={props.newItem.parentModuleId}
          onChange={(e) => props.onParentModuleChange(e.target.value)}
          options={props.moduleOptions}
          disabled={props.disabledModules}
        />
      </Field>
    </div>
  );
}

function EndpointFields(props: {
  newItem: ReturnType<typeof useAppStore.getState>['newItem'];
  onNameChange: (n: string) => void;
  onParentModuleChange: (id: string) => void;
  onParentCollectionChange: (id: string) => void;
  moduleOptions: { value: string; label: string }[];
  collectionOptions: { value: string; label: string }[];
  disabledModules: boolean;
  nameInputRef: React.RefObject<HTMLInputElement>;
}) {
  return (
    <div className="space-y-3">
      <NameField {...props} autoFocus />
      <Field label="所属模块" required>
        <Select
          value={props.newItem.parentModuleId}
          onChange={(e) => props.onParentModuleChange(e.target.value)}
          options={props.moduleOptions}
          disabled={props.disabledModules}
        />
      </Field>
      <Field label="所属集合" required>
        <Select
          value={props.newItem.parentCollectionId}
          onChange={(e) => props.onParentCollectionChange(e.target.value)}
          options={props.collectionOptions}
          disabled={props.disabledModules || props.collectionOptions.length === 0}
        />
      </Field>
    </div>
  );
}

// ---------- tiny field wrapper ----------

function Field({ label, required, children }: { label: string; required?: boolean; children: React.ReactNode }) {
  return (
    <label className="block space-y-1">
      <span className="text-xs font-medium text-muted-foreground">
        {label}
        {required && <span className="ml-0.5 text-destructive">*</span>}
      </span>
      {children}
    </label>
  );
}

// ---------- color picker ----------

function ColorPicker({ value, onChange }: { value: string; onChange: (c: string) => void }) {
  return (
    <div className="flex items-center gap-1.5">
      {PROJECT_COLORS.map((c) => (
        <button
          key={c}
          type="button"
          onClick={() => onChange(c)}
          aria-label={`选择颜色 ${c}`}
          className={cn(
            'h-5 w-5 rounded-full transition-all',
            value === c
              ? 'ring-2 ring-ring ring-offset-2 ring-offset-card'
              : 'opacity-70 hover:opacity-100',
          )}
          style={{ backgroundColor: c }}
        />
      ))}
    </div>
  );
}
