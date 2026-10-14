/**
 * 三个新建对话框共用的外壳 —— `Dialog` 框、提交守卫，以及各类型都有的两个字段。
 *
 * 各类型是独立组件、住在自己的入口旁（`CreateProjectDialog` / `CreateModuleDialog` /
 * `CreateCollectionDialog`），本文件只留它们的公共部分。接口没有对话框：
 * `createEndpointAt` 直接写那一行。
 */

import { useEffect, type FormEvent, type ReactNode, type RefObject } from 'react';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { PROJECT_COLORS } from '@/lib/palette';

interface CreateDialogProps {
  open: boolean;
  onClose: () => void;
  /** 对话框标题，如 「新建模块」。 */
  title: string;
  /** 标题下的一行说明，可选。 */
  description?: string;
  /** 必填项还没填齐时为 false。 */
  canSubmit: boolean;
  onSubmit: () => void;
  /** 打开即聚焦 —— 名称是唯一每次都要填的字段。 */
  nameInputRef: RefObject<HTMLInputElement>;
  children: ReactNode;
}

/** 三个对话框共用的框架：回车提交、打开时聚焦名称，主按钮直接反映守卫而非点了没反应。 */
export function CreateDialog({
  open,
  onClose,
  title,
  description,
  canSubmit,
  onSubmit,
  nameInputRef,
  children,
}: CreateDialogProps) {
  // 延后一拍：输入框要等对话框渲染出来才存在，而 `Dialog` 的 portal 在 open 过渡时才挂。
  useEffect(() => {
    if (!open) return;
    const t = window.setTimeout(() => nameInputRef.current?.focus(), 0);
    return () => window.clearTimeout(t);
  }, [open, nameInputRef]);

  if (!open) return null;

  const handleSubmit = (e?: FormEvent) => {
    e?.preventDefault();
    if (!canSubmit) return;
    onSubmit();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      title={title}
      description={description}
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={onClose}>
            取消
          </Button>
          <Button size="sm" onClick={() => handleSubmit()} disabled={!canSubmit}>
            创建
          </Button>
        </>
      }
    >
      <form onSubmit={handleSubmit} className="space-y-3">
        {children}
      </form>
    </Dialog>
  );
}

export function Field({
  label,
  required,
  children,
}: {
  label: string;
  required?: boolean;
  children: ReactNode;
}) {
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

export function NameField({
  value,
  onChange,
  inputRef,
  placeholder = '输入名称',
}: {
  value: string;
  onChange: (next: string) => void;
  inputRef: RefObject<HTMLInputElement>;
  placeholder?: string;
}) {
  return (
    <Field label="名称" required>
      <Input
        ref={inputRef}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoFocus
        data-testid="new-item-name"
      />
    </Field>
  );
}

export function DescriptionField({
  value,
  onChange,
  placeholder = '一句话说明（可选）',
}: {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
}) {
  return (
    <Field label="描述">
      <Input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} />
    </Field>
  );
}

/** 项目颜色色板。只有项目对话框有：颜色正是欢迎页列表和树头部分辨两个项目的依据，模块与集合都没处显示它。 */
export function ColorPicker({ value, onChange }: { value: string; onChange: (c: string) => void }) {
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
