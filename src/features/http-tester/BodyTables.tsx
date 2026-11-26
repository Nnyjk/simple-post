import { useEffect, useMemo, useRef, useState } from 'react';
import { FileUp, Plus, Trash2, X } from 'lucide-react';
import type { BodyFile, FormField, MultipartPart } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { DualProgressBar } from '@/components/ui/progress';
import { formatBytes, uid } from '@/lib/utils';
import {
  parseBodyFile,
  parseFormFields,
  parseMultipartParts,
  serializeFormFields,
} from '@/lib/body';
import { VarHintInput, type VarEntry } from './var-hint';
import { decodedBase64ByteLength, fileContentType, readFileAsBase64 } from './model/fileBytes';
import type { TabUploadProgress } from '@/stores/slices/ui-response';

/**
 * 请求体的三个结构化编辑器：`x-www-form-urlencoded`、`form-data` 与 `binary`。
 *
 * **三者都是结构化的**：`content` 里存的不是拼好的正文，而是一份描述结构的 JSON 文本。
 * 结构化换来两样东西——表格里的「说明」列有地方放，文件分片有地方放字节。
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

/** 一行文件读取（「添加到请求」）的状态。 */
interface RowRead {
  status: 'adding' | 'ready' | 'error';
  loaded: number;
  total: number;
  error?: string;
}

/** 一行在「发送到服务器」阶段的进度，0–1。 */
interface FileUploadProgress {
  client: number;
  server?: number;
}

/**
 * 行槽位：与分片**一一对应**的一串稳定 token。
 *
 * 行没有 id（见文件头），而读取是异步的，用下标当身份会在删掉上面的行之后把进度挂到别人
 * 头上。故给每一行配一个只活在渲染层的 token，删行时把同一个下标的 token 一并移除。
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
 * 把整体的上传 / 接收字节数折算到某一个分片行。
 *
 * 只有已经轮到（或已经传完）的分片才返回进度：还没开始的行不画 0%，那只会让整张表看起来
 * 同时有五个进度条。
 */
function fileUploadProgress(
  index: number,
  upload: TabUploadProgress | null | undefined,
): FileUploadProgress | undefined {
  if (!upload) return undefined;
  const range = upload.partRanges.find((r) => r.index === index);
  if (!range || upload.sentBytes <= range.start) return undefined;
  const span = range.end - range.start;
  if (span <= 0) return undefined;
  return {
    client: (upload.sentBytes - range.start) / span,
    server:
      upload.serverReceivedBytes === null
        ? undefined
        : (upload.serverReceivedBytes - range.start) / span,
  };
}

/**
 * form-data 的分片表。每个分片是「文本」或「文件」：文件分片的内容是字节，存在 `base64`
 * 里，而分片数组存在 JSON 文本里 —— 故字节只能以 base64 的形态进去。
 *
 * **两种进度画在同一格**：选文件时是「添加到请求」（分块读字节并编码），发送时是「发送到
 * 服务器」（传输层已写出 / 服务端已接收）。它们发生在两个时刻，但说的是同一个资源，换位置
 * 只会让人以为换了一个东西。
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
  const parts = useMemo(() => parseMultipartParts(content), [content]);

  /** 行槽位。与 `parts` 对齐；内容被外部改写时清空，回落到按位置生成的临时 token。 */
  const [slots, setSlots] = useState<string[]>([]);
  /** 读取状态，键是槽位 token。 */
  const [reads, setReads] = useState<Record<string, RowRead>>({});
  /** 在途读取的 controller，键同上。 */
  const controllers = useRef<Map<string, AbortController>>(new Map());

  // 处理器在异步完成时要用到**最新**的分片与槽位：闭包捕获的是发起那一刻的那一份。
  const partsRef = useRef(parts);
  partsRef.current = parts;
  const slotsRef = useRef(slots);
  /** 自己写出去的那份内容，用来把「外部改写」与「自己的写入」分开。 */
  const selfWrite = useRef<string | null>(null);

  const commitSlots = (next: string[]) => {
    slotsRef.current = next;
    setSlots(next);
  };

  const write = (next: MultipartPart[]) => {
    const json = JSON.stringify(next);
    selfWrite.current = json;
    onChange(json);
  };

  /** 停掉一行的读取并清掉它的状态。 */
  const stopRead = (token: string) => {
    controllers.current.get(token)?.abort();
    controllers.current.delete(token);
    setReads((prev) => {
      if (!(token in prev)) return prev;
      const next = { ...prev };
      delete next[token];
      return next;
    });
  };

  /** 把槽位补到当前行数（首次操作时它还是空的），返回补好之后的那一串。 */
  const slotsFor = (): string[] => {
    const next = ensureSlots(slotsRef.current, partsRef.current.length);
    if (next !== slotsRef.current) commitSlots(next);
    return next;
  };

  // 内容被**别人**改写（切端点、撤销、导入）时，在途读取全部作废：它们读出来的字节属于
  // 上一份分片表，写回去就是错的。
  useEffect(() => {
    if (selfWrite.current === content) {
      selfWrite.current = null;
      return;
    }
    for (const ctrl of controllers.current.values()) ctrl.abort();
    controllers.current.clear();
    slotsRef.current = [];
    setSlots([]);
    setReads({});
  }, [content]);

  // 卸载时中止全部在途读取：组件没了就没人要这些字节了。
  useEffect(() => {
    const inFlight = controllers.current;
    return () => {
      for (const ctrl of inFlight.values()) ctrl.abort();
      inFlight.clear();
    };
  }, []);

  const update = (index: number, patch: Partial<MultipartPart>) =>
    write(partsRef.current.map((p, i) => (i === index ? { ...p, ...patch } : p)));

  const remove = (index: number) => {
    const tokens = slotsFor();
    stopRead(tokens[index]);
    write(partsRef.current.filter((_, i) => i !== index));
    commitSlots(tokens.filter((_, i) => i !== index));
  };

  const add = () => {
    const tokens = slotsFor();
    write([...partsRef.current, { name: '', value: '', enabled: true }]);
    commitSlots([...tokens, `row-${uid()}`]);
  };

  /** 文本 ↔ 文件。另一侧的内容清掉，在途读取也停下。 */
  const switchType = (index: number, file: boolean) => {
    stopRead(slotsFor()[index]);
    const part = partsRef.current[index];
    if (!part) return;
    if (file) {
      update(index, { filename: part.filename ?? '', base64: part.base64 ?? '' });
      return;
    }
    update(index, { filename: undefined, base64: undefined, contentType: undefined });
  };

  /** 选文件：分块读、报进度，读完按 token 重新定位再写回。 */
  const pickFile = async (index: number, file: File) => {
    const token = slotsFor()[index];
    stopRead(token);
    const ctrl = new AbortController();
    controllers.current.set(token, ctrl);
    setReads((prev) => ({
      ...prev,
      [token]: { status: 'adding', loaded: 0, total: file.size },
    }));
    try {
      const base64 = await readFileAsBase64(file, {
        signal: ctrl.signal,
        onProgress: ({ loaded, total }) =>
          setReads((prev) => {
            const current = prev[token];
            if (!current || current.status !== 'adding') return prev;
            return { ...prev, [token]: { ...current, loaded, total } };
          }),
      });
      if (ctrl.signal.aborted) return;
      // 读完时下标可能已经变了（期间删掉了别的行）：按 token 重新找它现在的位置。
      const at = slotsRef.current.indexOf(token);
      if (at === -1 || at >= partsRef.current.length) return;
      write(
        partsRef.current.map((p, i) =>
          i === at ? { ...p, filename: file.name, base64, contentType: fileContentType(file) } : p,
        ),
      );
      setReads((prev) => ({
        ...prev,
        [token]: { status: 'ready', loaded: file.size, total: file.size },
      }));
    } catch (err) {
      // 取消不是错误：删行、切类型、换文件都会走到这里。
      if ((err as { name?: string } | null)?.name === 'AbortError') return;
      setReads((prev) => ({
        ...prev,
        [token]: {
          status: 'error',
          loaded: 0,
          total: file.size,
          error: (err as Error)?.message ?? String(err),
        },
      }));
    } finally {
      if (controllers.current.get(token) === ctrl) controllers.current.delete(token);
    }
  };

  const clearFile = (index: number) => {
    stopRead(slotsFor()[index]);
    update(index, { filename: '', base64: '' });
  };

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
              read={reads[slots[index] ?? '']}
              upload={fileUploadProgress(index, uploadProgress)}
              onUpdate={update}
              onRemove={remove}
              onSwitchType={switchType}
              onPick={pickFile}
              onClearFile={clearFile}
            />
          ))}
        </tbody>
      </table>
      <Button variant="outline" size="sm" onClick={add} className="mt-2 gap-1.5 text-xs">
        <Plus className="h-3.5 w-3.5" />
        添加分片
      </Button>
    </div>
  );
}

/**
 * form-data 的一行。文件那一格是独立的组件：选文件是**异步**的（要读完字节），状态放在
 * 整表的编辑器里，因为读取跨越多行生命周期、且要在删行时按槽位取消。
 */
function MultipartRow({
  part,
  index,
  varEntries,
  envName,
  read,
  upload,
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
  read: RowRead | undefined;
  upload: FileUploadProgress | undefined;
  onUpdate: (index: number, patch: Partial<MultipartPart>) => void;
  onRemove: (index: number) => void;
  onSwitchType: (index: number, file: boolean) => void;
  onPick: (index: number, file: File) => void;
  onClearFile: (index: number) => void;
}) {
  const isFile = part.base64 !== undefined || !!part.filename;

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
            filename={part.filename ?? ''}
            sizeBytes={part.base64 ? decodedBase64ByteLength(part.base64) : 0}
            read={read}
            upload={upload}
            onPick={(file) => onPick(index, file)}
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
 * 文件那一格：选文件 / 看清选了什么 / 换掉它 / 两个阶段的进度。
 *
 * 进度画在文件名下面而不是替换掉文件名：那两件事同时有用，而行的宽度只够竖着叠。
 */
function FileCell({
  filename,
  sizeBytes,
  read,
  upload,
  onPick,
  onClear,
}: {
  filename: string;
  /** 已存字节数（base64 解码后的长度）；0 = 没有文件。 */
  sizeBytes: number;
  read: RowRead | undefined;
  upload: FileUploadProgress | undefined;
  onPick: (file: File) => void;
  onClear: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const adding = read?.status === 'adding';

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div className="flex min-w-0 items-center gap-1">
        <input
          ref={inputRef}
          type="file"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0];
            // 选同一个文件两次时 `change` 不再触发，故读完就把值清掉。
            e.target.value = '';
            if (file) onPick(file);
          }}
        />
        {filename ? (
          <>
            <span className="min-w-0 flex-1 truncate font-mono text-micro text-foreground/80">
              {filename}
            </span>
            <span className="shrink-0 text-nano text-faint">{formatBytes(sizeBytes)}</span>
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
            disabled={adding}
            onClick={() => inputRef.current?.click()}
          >
            <FileUp className="h-3.5 w-3.5" />
            {adding ? '读取中' : '选择文件'}
          </Button>
        )}
      </div>

      {read?.status === 'adding' && (
        <div className="flex items-center gap-2">
          <DualProgressBar
            value={read.total > 0 ? read.loaded / read.total : 1}
            label="将资源添加到请求的进度"
            className="flex-1"
          />
          <span className="shrink-0 text-nano text-faint">
            {percent(read.total > 0 ? read.loaded / read.total : 1)}%
          </span>
        </div>
      )}

      {read?.status === 'error' && (
        <span className="text-nano text-destructive" role="alert">
          {read.error}
        </span>
      )}

      {upload && !adding && (
        <div className="flex items-center gap-2">
          <DualProgressBar
            value={upload.client}
            overlay={upload.server}
            label="将资源发送到服务器的进度"
            className="flex-1"
          />
          <span className="shrink-0 text-nano text-faint">
            {percent(upload.client)}%
            {upload.server !== undefined && ` · 服务端 ${percent(upload.server)}%`}
          </span>
        </div>
      )}
    </div>
  );
}

// ============================================================
// binary
// ============================================================

/**
 * binary 模式：整个请求体就是一个文件。`content` 里存的是文件描述（文件名、体积、MIME 类型
 * 与字节的 base64）。字节随请求走，故它也会进历史记录与请求详情。
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
          <span className="text-micro text-muted-foreground">{formatBytes(file.size)}</span>
          <span className="text-micro text-muted-foreground">{file.contentType}</span>
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
