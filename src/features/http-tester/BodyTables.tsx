import { useMemo, useRef, useState } from 'react';
import { FileUp, Plus, Trash2, X } from 'lucide-react';
import type { BodyFile, FormField, MultipartPart } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { formatBytes } from '@/lib/utils';
import {
  parseBodyFile,
  parseFormFields,
  parseMultipartParts,
  serializeFormFields,
} from '@/lib/body';
import { VarHintInput, type VarEntry } from './var-hint';
import { fileContentType, readFileAsBase64 } from './model/fileBytes';

/**
 * 请求体的三个结构化编辑器：`x-www-form-urlencoded`、`form-data` 与 `binary`。
 *
 * **三者都是结构化的**：`content` 里存的不是拼好的正文，而是一份描述结构的 JSON
 * 文本。结构化换来两样东西——表格里的「说明」列有地方放，文件分片有地方放字节。
 *
 * 装配在发送那一步做（`src/lib/body` 与 Rust 的 `core::body` 各一份），故本组件只
 * 负责把用户的输入写成那份 JSON 文本。
 *
 * **行没有稳定 id，用下标当 React key。** 行只能追加与删除，不会重排，故下标在
 * 两次编辑之间是稳定的。给行加 id 意味着它要进 `content` 那份 JSON，而那份 JSON
 * 同时被后端与 MCP 侧解析——多一个纯界面字段会让两侧的形状对不上。
 */

// ============================================================
// x-www-form-urlencoded
// ============================================================

/**
 * 表单字段表：一行一个字段。
 *
 * 写回 `content` 的是 `FormField[]` 的 JSON 文本。后端与 MCP 侧读它时逐字段模板化，
 * 再拼成 urlencoded 正文。
 */
export function FormFieldsEditor({
  content,
  onChange,
  varEntries,
  envName,
}: {
  content: string;
  onChange: (next: string) => void;
  varEntries: VarEntry[];
  envName?: string;
}) {
  const fields = useMemo(() => parseFormFields(content), [content]);

  const write = (next: FormField[]) => onChange(serializeFormFields(next));
  const update = (index: number, patch: Partial<FormField>) =>
    write(fields.map((f, i) => (i === index ? { ...f, ...patch } : f)));
  const remove = (index: number) => write(fields.filter((_, i) => i !== index));
  const add = () => write([...fields, { key: '', value: '', enabled: true }]);

  return (
    <div className="rounded-md border border-border">
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="text-[10px] uppercase tracking-wider text-muted-foreground">
            <th className="w-8 px-1 pb-1.5 text-left font-medium"></th>
            <th className="w-[32%] px-2 pb-1.5 text-left font-medium">参数名</th>
            <th className="w-[38%] px-2 pb-1.5 text-left font-medium">参数值</th>
            <th className="px-2 pb-1.5 text-left font-medium">说明</th>
            <th className="w-8 px-1"></th>
          </tr>
        </thead>
        <tbody>
          {fields.map((field, index) => (
            <tr key={index} className="group">
              <td className="py-1">
                <input
                  type="checkbox"
                  checked={field.enabled}
                  onChange={(e) => update(index, { enabled: e.target.checked })}
                  className="h-3.5 w-3.5 cursor-pointer accent-primary"
                  aria-label="这一行是否参与发送"
                />
              </td>
              <td className="px-1 py-1">
                <Input
                  value={field.key}
                  onChange={(e) => update(index, { key: e.target.value })}
                  placeholder="参数名"
                  autoComplete="off"
                  className="h-7 font-mono text-xs"
                />
              </td>
              <td className="px-1 py-1">
                <VarHintInput
                  value={field.value}
                  onChange={(next) => update(index, { value: next })}
                  entries={varEntries}
                  envName={envName}
                  placeholder="参数值"
                  className="h-7 font-mono text-xs"
                />
              </td>
              <td className="px-1 py-1">
                <Input
                  value={field.description ?? ''}
                  onChange={(e) => update(index, { description: e.target.value })}
                  placeholder="可选"
                  className="h-7 text-xs"
                />
              </td>
              <td className="px-1 py-1 text-right">
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6 opacity-0 transition-opacity group-hover:opacity-100"
                  onClick={() => remove(index)}
                  aria-label="删掉这一行"
                >
                  <Trash2 className="w-3 h-3 text-muted-foreground" />
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="border-t border-border p-2">
        <Button variant="outline" size="sm" onClick={add} className="gap-1.5 text-xs">
          <Plus className="h-3.5 w-3.5" />
          添加字段
        </Button>
      </div>
    </div>
  );
}

// ============================================================
// form-data
// ============================================================

/**
 * form-data 的分片表。
 *
 * 每个分片是「文本」或「文件」：文件分片的内容是字节，存在 `base64` 里，而分片
 * 数组存在 JSON 文本里——故字节只能以 base64 的形态进去。
 */
export function MultipartPartsEditor({
  content,
  onChange,
  varEntries,
  envName,
}: {
  content: string;
  onChange: (next: string) => void;
  varEntries: VarEntry[];
  envName?: string;
}) {
  const parts = useMemo(() => parseMultipartParts(content), [content]);

  const write = (next: MultipartPart[]) => onChange(JSON.stringify(next));
  const update = (index: number, patch: Partial<MultipartPart>) =>
    write(parts.map((p, i) => (i === index ? { ...p, ...patch } : p)));
  const remove = (index: number) => write(parts.filter((_, i) => i !== index));
  const add = () => write([...parts, { name: '', value: '', enabled: true }]);

  return (
    <div className="rounded-md border border-border">
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="text-[10px] uppercase tracking-wider text-muted-foreground">
            <th className="w-8 px-1 pb-1.5 text-left font-medium"></th>
            <th className="w-[24%] px-2 pb-1.5 text-left font-medium">参数名</th>
            <th className="w-[18%] px-2 pb-1.5 text-left font-medium">类型</th>
            <th className="w-[30%] px-2 pb-1.5 text-left font-medium">参数值</th>
            <th className="px-2 pb-1.5 text-left font-medium">说明</th>
            <th className="w-8 px-1"></th>
          </tr>
        </thead>
        <tbody>
          {parts.map((part, index) => (
            <MultipartRow
              key={index}
              part={part}
              index={index}
              varEntries={varEntries}
              envName={envName}
              onUpdate={update}
              onRemove={remove}
            />
          ))}
        </tbody>
      </table>
      <div className="border-t border-border p-2">
        <Button variant="outline" size="sm" onClick={add} className="gap-1.5 text-xs">
          <Plus className="h-3.5 w-3.5" />
          添加分片
        </Button>
      </div>
    </div>
  );
}

/**
 * form-data 的一行。
 *
 * 文件那一格是独立的组件：选文件是**异步**的（要读完字节），状态放在组件里，
 * 不能塞进整表的受控数据里。
 */
function MultipartRow({
  part,
  index,
  varEntries,
  envName,
  onUpdate,
  onRemove,
}: {
  part: MultipartPart;
  index: number;
  varEntries: VarEntry[];
  envName?: string;
  onUpdate: (index: number, patch: Partial<MultipartPart>) => void;
  onRemove: (index: number) => void;
}) {
  const isFile = part.base64 !== undefined || !!part.filename;

  /** 在「文本」与「文件」之间切换时，另一侧的内容要清掉：留着它会让两种形态同时存在。 */
  const switchTo = (file: boolean) => {
    if (file) {
      onUpdate(index, { filename: part.filename ?? '', base64: part.base64 ?? '' });
      return;
    }
    onUpdate(index, { filename: undefined, base64: undefined, contentType: undefined });
  };

  return (
    <tr className="group">
      <td className="py-1">
        <input
          type="checkbox"
          checked={part.enabled !== false}
          onChange={(e) => onUpdate(index, { enabled: e.target.checked })}
          className="h-3.5 w-3.5 cursor-pointer accent-primary"
          aria-label="这一分片是否参与发送"
        />
      </td>
      <td className="px-1 py-1">
        <Input
          value={part.name}
          onChange={(e) => onUpdate(index, { name: e.target.value })}
          placeholder="参数名"
          autoComplete="off"
          className="h-7 font-mono text-xs"
        />
      </td>
      <td className="px-1 py-1">
        <select
          value={isFile ? 'file' : 'text'}
          onChange={(e) => switchTo(e.target.value === 'file')}
          className="h-7 w-full rounded-md border border-border bg-background px-1.5 text-xs"
          aria-label="分片类型"
        >
          <option value="text">文本</option>
          <option value="file">文件</option>
        </select>
      </td>
      <td className="px-1 py-1">
        {isFile ? (
          <FileCell
            filename={part.filename ?? ''}
            onPick={async (file) => {
              onUpdate(index, {
                filename: file.name,
                base64: await readFileAsBase64(file),
                contentType: fileContentType(file),
              });
            }}
            onClear={() => onUpdate(index, { filename: '', base64: '' })}
          />
        ) : (
          <VarHintInput
            value={part.value}
            onChange={(next) => onUpdate(index, { value: next })}
            entries={varEntries}
            envName={envName}
            placeholder="参数值"
            className="h-7 font-mono text-xs"
          />
        )}
      </td>
      <td className="px-1 py-1">
        <Input
          value={part.description ?? ''}
          onChange={(e) => onUpdate(index, { description: e.target.value })}
          placeholder="可选"
          className="h-7 text-xs"
        />
      </td>
      <td className="px-1 py-1 text-right">
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 opacity-0 transition-opacity group-hover:opacity-100"
          onClick={() => onRemove(index)}
          aria-label="删掉这一分片"
        >
          <Trash2 className="w-3 h-3 text-muted-foreground" />
        </Button>
      </td>
    </tr>
  );
}

/** 文件那一格：选文件 / 看清选了什么 / 换掉它。 */
function FileCell({
  filename,
  onPick,
  onClear,
}: {
  filename: string;
  onPick: (file: File) => void | Promise<void>;
  onClear: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);

  return (
    <div className="flex items-center gap-1">
      <input
        ref={inputRef}
        type="file"
        className="hidden"
        onChange={async (e) => {
          const file = e.target.files?.[0];
          // 选同一个文件两次时 `change` 不再触发，故读完就把值清掉。
          e.target.value = '';
          if (!file) return;
          setBusy(true);
          try {
            await onPick(file);
          } finally {
            setBusy(false);
          }
        }}
      />
      {filename ? (
        <>
          <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-foreground/80">
            {filename}
          </span>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={() => inputRef.current?.click()}
            aria-label="换一个文件"
          >
            <FileUp className="h-3 w-3 text-muted-foreground" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={onClear}
            aria-label="清掉这个文件"
          >
            <X className="h-3 w-3 text-muted-foreground" />
          </Button>
        </>
      ) : (
        <Button
          variant="outline"
          size="sm"
          className="h-7 gap-1.5 text-xs"
          disabled={busy}
          onClick={() => inputRef.current?.click()}
        >
          <FileUp className="h-3.5 w-3.5" />
          {busy ? '读取中' : '选择文件'}
        </Button>
      )}
    </div>
  );
}

// ============================================================
// binary
// ============================================================

/**
 * binary 模式：整个请求体就是一个文件。
 *
 * `content` 里存的是文件描述（文件名、体积、MIME 类型与字节的 base64）。字节随请求
 * 走，故它也会进历史记录与请求详情。
 */
export function BinaryBodyEditor({
  content,
  onChange,
}: {
  content: string;
  onChange: (next: string) => void;
}) {
  const file = useMemo(() => parseBodyFile(content), [content]);
  const inputRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);

  const pick = async (picked: File) => {
    setBusy(true);
    try {
      const description: BodyFile = {
        name: picked.name,
        size: picked.size,
        contentType: fileContentType(picked),
        base64: await readFileAsBase64(picked),
      };
      onChange(JSON.stringify(description));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-md border border-dashed border-border p-4">
      <input
        ref={inputRef}
        type="file"
        className="hidden"
        onChange={async (e) => {
          const picked = e.target.files?.[0];
          e.target.value = '';
          if (!picked) return;
          await pick(picked);
        }}
      />
      {file ? (
        <div className="flex items-center gap-2">
          <FileUp className="h-4 w-4 text-muted-foreground" aria-hidden />
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground/85">
            {file.name}
          </span>
          <span className="text-[11px] text-muted-foreground">{formatBytes(file.size)}</span>
          <span className="text-[11px] text-muted-foreground">{file.contentType}</span>
          <Button
            variant="outline"
            size="sm"
            className="h-7"
            onClick={() => inputRef.current?.click()}
          >
            换一个
          </Button>
          <Button variant="ghost" size="sm" className="h-7" onClick={() => onChange('')}>
            清掉
          </Button>
        </div>
      ) : (
        <div className="flex flex-col items-center gap-2 py-4 text-center">
          <FileUp className="h-6 w-6 text-muted-foreground/50" aria-hidden />
          <p className="text-xs text-muted-foreground">选一个文件，整个请求体就是它的字节</p>
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5"
            disabled={busy}
            onClick={() => inputRef.current?.click()}
          >
            <FileUp className="h-3.5 w-3.5" />
            {busy ? '读取中' : '选择文件'}
          </Button>
        </div>
      )}
    </div>
  );
}
