/**
 * 浏览器选中的文件字节：`<input type=file>` 只给 `File`，给不出任何路径。
 *
 * 桌面形态的文件分片只有路径，执行层按路径现读；浏览器形态把这些字节放进
 * `multipart/form-data` 随发送请求一起送上去（服务端 `/api/http/send-upload` 一族边收边
 * 转发，不落盘）。这里是那个「暂存」：选文件时登记 `File`，发送时按请求体里文件分片的顺序
 * 取出来。
 *
 * 不进 store、不进 `localStorage`：`File` 是活句柄，序列化没有意义。刷新页面后重新选一次。
 */
import { isFilePart, parseBodyFile, parseMultipartParts } from './body';
import type { BodyMode } from '@/types/domain';

/** 已选文件：键是「文件名 + 字节数」。同名同大小视为同一份内容，后选的覆盖先选的。 */
const stash = new Map<string, File>();

const keyOf = (name: string, size: number): string => `${name}\u0000${size}`;

/** 登记一个选中的文件。 */
export function rememberUpload(file: File): void {
  stash.set(keyOf(file.name, file.size), file);
}

function fileFor(name: string, size: number): File {
  const file = stash.get(keyOf(name, size));
  if (!file) {
    throw new Error(`浏览器没有「${name}」的字节：它不在这次会话里选过的文件中，请重新选择文件`);
  }
  return file;
}

/**
 * 这个请求体里要随请求上传的文件，按请求体里出现的顺序。
 *
 * 有非空 `path` 的分片不算：那种字节由执行层按路径读盘（桌面形态，或路径在服务端机器上真
 * 能读到）。判据与 Rust `multipart::is_file_part` 同一条。
 */
export function uploadFiles(mode: BodyMode, body: string | null): File[] {
  const text = body ?? '';
  if (mode === 'multipart') {
    return parseMultipartParts(text)
      .filter((part) => part.enabled !== false && isFilePart(part) && (part.path ?? '') === '')
      .map((part) => {
        const size = part.size;
        if (size === undefined) {
          throw new Error(
            `文件分片「${part.name}」没有声明字节数：服务端要靠它写 Content-Length，请重新选择文件`,
          );
        }
        return fileFor(part.filename ?? '', size);
      });
  }
  if (mode === 'binary') {
    const file = parseBodyFile(text);
    // 浏览器选中的零字节文件也是上传：`size === 0` 说明它没有内容，不是「没有文件」。
    // 没有文件描述、或带路径（执行层读盘）时才不算。
    if (!file || file.path !== '' || file.name === '') return [];
    return [fileFor(file.name, file.size)];
  }
  return [];
}

/**
 * 发送信封 + 文件字节的请求体。
 *
 * 第一个分片恒为 `request`（JSON 信封），其余按顺序是各文件内容。服务端只认顺序，不认名字；
 * 名字仍写成文件名，抓包时对得上前端。
 */
export function uploadForm(envelope: unknown, files: File[]): FormData {
  const form = new FormData();
  form.append('request', new Blob([JSON.stringify(envelope)], { type: 'application/json' }));
  for (const file of files) form.append(file.name, file);
  return form;
}
