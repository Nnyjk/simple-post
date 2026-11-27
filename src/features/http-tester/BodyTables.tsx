import { useEffect, useMemo, useRef, useState } from 'react';
import { FileUp, Plus, Trash2, X } from 'lucide-react';
import type { BodyFile, FormField, MultipartPart } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { DualProgressBar } from '@/components/ui/progress';
import { uid } from '@/lib/utils';
import {
  parseBodyFile,
  parseFormFields,
  parseMultipartParts,
  serializeFormFields,
} from '@/lib/body';
import { VarHintInput, type VarEntry } from './var-hint';
import { baseName, pickLocalFile } from '@/lib/pick-file';
import type { TabUploadProgress } from '@/stores/slices/ui-response';

/**
 * 请求体的三个结构化编辑器：`x-www-form-urlencoded`、`form-data` 与 `binary`。
 *
 * **三者都是结构化的**：`content` 里存的不是拼好的正文，而是一份描述结构的 JSON 文本。
 * 结构化换来两样东西——表格里的「说明」列有地方放，文件分片有地方放路径。
 *
 * 装配在发送那一步做（`src/lib/body` 与 Rust 的 `core::body` 各一份），故本组件只负责把用户
 * 的输入写成那份 JSON 文本。
 *
 * **行没有稳定 id，用下标当 React key。** 行只能追加与删除，不会重排，故下标在两次编辑之间
 * 是稳定的。给行加 id 意味着它要进 `content` 那份 JSON，而那份 JSON 同时被后端与 MCP 侧
 * 解析——多一个纯界面字段会让两侧的形状对不上。
 */

// ============================================================
// x-www-form-urlencoded 表单
// ============================================================

/**
 * 表单字段表：一行一个字段。写回 `content` 的是 `FormField[]` 的 JSON 文本，后端与 MCP 侧读它
 * 时逐字段模板化，再拼成 urlencoded 正文。
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
    <div>
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="text-nano uppercase tracking-wider text-muted-foreground">
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
                  className="h-6 w-6 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 group-focus-within:opacity-100"
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
      <Button variant="outline" size="sm" onClick={add} className="mt-2 gap-1.5 text-xs">
        <Plus className="h-3.5 w-3.5" />
        添加字段
      </Button>
    </div>
  );
}

// ============================================================
// form-data
// ============================================================

/**
 * 行槽位：与分片**一一对应**的一串稳定 token。
 *
 * 行没有 id（见文件头），而下标在删掉上面的行之后会指向别人，故给每一行配一个只活在
 * 渲染层的 token，删行时把同一个下标的 token 一并移除。槽位只用来把「选文件失败」挂到
 * 它那一行上。
 */
function ensureSlots(prev: readonly string[], length: number): string[] {
  if (prev.length === length) return prev as string[];
  const next = prev.slice(0, length);
  while (next.length < length) next.push(`row-${next.length}`);
  return next;
}

/** 0–1 的进度折算成整数百分比。 */
function percent(ratio: number): number {
  return Math.round(Math.min(1, Math.max(0, ratio)) * 100);
}

/**
 * form-data 的分片表。每个分片是「文本」或「文件」：文件分片存的是**路径**，字节由执行层
 * 在发请求时按路径现读，故渲染进程不碰文件内容。
 *
 * 发送时的进度是**整表**一根：传输层报上来的是整体字节数，分不到具体某个文件上。
 */
export function MultipartPartsEditor({
  content,
  onChange,
  varEntries,
  envName,
  uploadProgress,
}: {
  content: string;
  onChange: (next: string) => void;
  varEntries: VarEntry[];
  envName?: string;
  /** 当前标签这一次发送的传输进度；`null` / 缺席 = 没有在传。 */
  uploadProgress?: TabUploadProgress | null;
}) {
  /**
   * 分片表是**本地状态**，不是每次渲染从 `content` 解出来的：解析一份带文件的 body 要
   * 走完整个 JSON 文本（12 MB 的 body ≈ 14 ms，32 MB ≈ 34 ms），而任何一次键入都会
   * 重渲染这张表。故每次都敲键都解一遍时，代价随文件线性涨。
   *
   * `content` 仍是唯一的事实来源：外部改写（切端点、撤销、导入）时下面那个 effect 会
   * 重新解析并把本地状态换掉。自己写出去的内容不回读。
   */
  const [parts, setParts] = useState<MultipartPart[]>(() => parseMultipartParts(content));

  /** 行槽位。与 `parts` 对齐；内容被外部改写时清空，回落到按位置生成的临时 token。 */
  const [slots, setSlots] = useState<string[]>([]);
  /** 选文件失败的理由，键是槽位 token。 */
  const [errors, setErrors] = useState<Record<string, string>>({});

  // 处理器在异步完成时要用到**最新**的分片与槽位：闭包捕获的是发起那一刻的那一份。
  const partsRef = useRef(parts);
  partsRef.current = parts;
  const slotsRef = useRef(slots);
  /** 自己写出去的那份内容，用来把「外部改写」与「自己的写入」分开。 */
  const selfWrite = useRef<string | null>(null);
  /** 已经同步过的那份内容。挂载与 StrictMode 的第二次执行都靠它跳过。 */
  const syncedContent = useRef(content);

  const commitSlots = (next: string[]) => {
    slotsRef.current = next;
    setSlots(next);
  };

  const write = (next: MultipartPart[]) => {
    const json = JSON.stringify(next);
    selfWrite.current = json;
    partsRef.current = next;
    setParts(next);
    onChange(json);
  };

  /** 把槽位补到当前行数（首次操作时它还是空的），返回补好之后的那一串。 */
  const slotsFor = (): string[] => {
    const next = ensureSlots(slotsRef.current, partsRef.current.length);
    if (next !== slotsRef.current) commitSlots(next);
    return next;
  };

  // 内容被**别人**改写（切端点、撤销、导入）时，本地状态整体换掉：里面那些路径属于上一份
  // 分片表。
  useEffect(() => {
    if (syncedContent.current === content) return;
    syncedContent.current = content;
    if (selfWrite.current === content) {
      selfWrite.current = null;
      return;
    }
    setParts(parseMultipartParts(content));
    slotsRef.current = [];
    setSlots([]);
    setErrors({});
  }, [content]);

  const update = (index: number, patch: Partial<MultipartPart>) =>
    write(partsRef.current.map((p, i) => (i === index ? { ...p, ...patch } : p)));

  const remove = (index: number) => {
    const tokens = slotsFor();
    const token = tokens[index];
    write(partsRef.current.filter((_, i) => i !== index));
    commitSlots(tokens.filter((_, i) => i !== index));
    if (token) {
      setErrors((prev) => {
        if (!(token in prev)) return prev;
        const next = { ...prev };
        delete next[token];
        return next;
      });
    }
  };

  const add = () => {
    const tokens = slotsFor();
    write([...partsRef.current, { name: '', value: '', enabled: true }]);
    commitSlots([...tokens, `row-${uid()}`]);
  };

  /** 文本 ↔ 文件。另一侧的内容清掉；切到文件时已有的路径留着。 */
  const switchType = (index: number, file: boolean) => {
    const token = slotsFor()[index];
    const part = partsRef.current[index];
    if (!part) return;
    if (file) {
      update(index, { filename: part.filename ?? '' });
    } else {
      update(index, { filename: undefined, path: undefined, contentType: undefined });
    }
    setErrors((prev) => {
      if (!(token in prev)) return prev;
      const next = { ...prev };
      delete next[token];
      return next;
    });
  };

  /**
   * 选文件：拿到路径就写回分片；用户取消什么也不做；失败（浏览器形态拿不到路径）把
   * 理由挂在那一行上。
   */
  const pickFile = async (index: number) => {
    const token = slotsFor()[index];
    try {
      const picked = await pickLocalFile();
      if (!picked) return;
      // 选择框是异步的：期间可能删了行，按 token 重新找它现在的位置。
      const at = slotsRef.current.indexOf(token);
      const part = at === -1 ? undefined : partsRef.current[at];
      if (!part) return;
      update(at, { path: picked.path, filename: part.filename || picked.name });
      setErrors((prev) => {
        if (!(token in prev)) return prev;
        const next = { ...prev };
        delete next[token];
        return next;
      });
    } catch (err) {
      setErrors((prev) => ({ ...prev, [token]: (err as Error)?.message ?? String(err) }));
    }
  };

  /** 清掉这个文件：路径与类型都没了，这一行回到文本形态。 */
  const clearFile = (index: number) => {
    update(index, { path: undefined, contentType: undefined, filename: undefined });
  };

  // 传输进度是整表一根：报上来的是整体字节数，分不到具体某个文件上。
  const upload = uploadProgress ?? null;
  const sent = upload && upload.totalBytes > 0 ? upload.sentBytes / upload.totalBytes : 0;
  const received =
    upload &&
    upload.serverReceivedBytes !== null &&
    upload.serverTotalBytes !== null &&
    upload.serverTotalBytes > 0
      ? upload.serverReceivedBytes / upload.serverTotalBytes
      : undefined;

  return (
    <div>
      <table className="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr className="text-nano uppercase tracking-wider text-muted-foreground">
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
              error={errors[slots[index] ?? '']}
              onUpdate={update}
              onRemove={remove}
              onSwitchType={switchType}
              onPick={pickFile}
              onClearFile={clearFile}
            />
          ))}
        </tbody>
      </table>
      <div className="mt-2 flex items-center gap-3">
        <Button variant="outline" size="sm" onClick={add} className="gap-1.5 text-xs">
          <Plus className="h-3.5 w-3.5" />
          添加分片
        </Button>
        {upload && (
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <DualProgressBar
              value={sent}
              overlay={received}
              label="将资源发送到服务器的进度"
              className="flex-1"
            />
            <span className="shrink-0 text-nano text-faint">
              {percent(sent)}%{received !== undefined && ` · 服务端 ${percent(received)}%`}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * form-data 的一行。文件那一格是独立的组件：选文件是**异步**的（弹框、拿路径），状态放在
 * 整表的编辑器里，因为它跨行、按槽位归属。
 */
function MultipartRow({
  part,
  index,
  varEntries,
  envName,
  error,
  onUpdate,
  onRemove,
  onSwitchType,
  onPick,
  onClearFile,
}: {
  part: MultipartPart;
  index: number;
  varEntries: VarEntry[];
  envName?: string;
  /** 选文件失败的理由；`undefined` = 没出错。 */
  error?: string;
  onUpdate: (index: number, patch: Partial<MultipartPart>) => void;
  onRemove: (index: number) => void;
  onSwitchType: (index: number, file: boolean) => void;
  onPick: (index: number) => void;
  onClearFile: (index: number) => void;
}) {
  // 有路径即文件分片（字节只从路径来）；只写了文件名的行也是文件分片，那多半是从文本
  // 切过来的，还等着选文件。
  const isFile = part.path !== undefined || part.filename !== undefined;

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
        <Select
          options={[
            { value: 'text', label: '文本' },
            { value: 'file', label: '文件' },
          ]}
          value={isFile ? 'file' : 'text'}
          onChange={(e) => onSwitchType(index, e.target.value === 'file')}
          className="h-7 w-full px-1.5 text-xs"
          aria-label="分片类型"
        />
      </td>
      <td className="px-1 py-1">
        {isFile ? (
          <FileCell
            path={part.path}
            filename={part.filename ?? ''}
            error={error}
            onPick={() => onPick(index)}
            onClear={() => onClearFile(index)}
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
          className="h-6 w-6 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 group-focus-within:opacity-100"
          onClick={() => onRemove(index)}
          aria-label="删掉这一分片"
        >
          <Trash2 className="w-3 h-3 text-muted-foreground" />
        </Button>
      </td>
    </tr>
  );
}

/**
 * 文件那一格：选文件 / 看清选了什么 / 换掉它。
 *
 * 显示的是**路径**：字节由执行层按路径现读，渲染进程拿不到文件，也就报不出体积。
 */
function FileCell({
  path,
  filename,
  error,
  onPick,
  onClear,
}: {
  /** 文件的绝对路径；`undefined` = 还没选文件。 */
  path?: string;
  /** 分片自己的文件名（线上那份 `filename`）。 */
  filename: string;
  /** 选文件失败的理由。 */
  error?: string;
  onPick: () => void;
  onClear: () => void;
}) {
  const label = filename || (path ? baseName(path) : '');

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div className="flex min-w-0 items-center gap-1">
        {label ? (
          <>
            <span
              className="min-w-0 flex-1 truncate font-mono text-micro text-foreground/80"
              title={path}
            >
              {label}
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              onClick={onPick}
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
          <Button variant="outline" size="sm" className="h-7 gap-1.5 text-xs" onClick={onPick}>
            <FileUp className="h-3.5 w-3.5" />
            选择文件
          </Button>
        )}
      </div>
      {error && (
        <span className="text-nano text-destructive" role="alert">
          {error}
        </span>
      )}
    </div>
  );
}

// ============================================================
// binary
// ============================================================

/**
 * binary 模式：整个请求体就是一个文件。`content` 里存的是文件描述（文件名与路径）；字节由
 * 执行层在发请求时按路径现读，故渲染进程不碰文件内容。
 */
export function BinaryBodyEditor({
  content,
  onChange,
}: {
  content: string;
  onChange: (next: string) => void;
}) {
  const file = useMemo(() => parseBodyFile(content), [content]);
  /** 选文件失败的理由。 */
  const [error, setError] = useState<string | null>(null);

  const pick = async () => {
    try {
      const picked = await pickLocalFile();
      if (!picked) return;
      const description: BodyFile = {
        name: picked.name,
        size: 0,
        contentType: '',
        path: picked.path,
      };
      onChange(JSON.stringify(description));
      setError(null);
    } catch (err) {
      setError((err as Error)?.message ?? String(err));
    }
  };

  return (
    <div className="rounded-md border border-dashed border-border p-4">
      {file ? (
        <div className="flex items-center gap-2">
          <FileUp className="h-4 w-4 text-muted-foreground" aria-hidden />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="min-w-0 truncate font-mono text-xs text-foreground/85">
              {file.name}
            </span>
            <span
              className="min-w-0 truncate font-mono text-micro text-muted-foreground"
              title={file.path}
            >
              {file.path}
            </span>
          </div>
          <Button variant="outline" size="sm" className="h-7" onClick={() => void pick()}>
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
          <Button variant="outline" size="sm" className="gap-1.5" onClick={() => void pick()}>
            <FileUp className="h-3.5 w-3.5" />
            选择文件
          </Button>
        </div>
      )}
      {error && (
        <p role="alert" className="mt-2 text-micro text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
