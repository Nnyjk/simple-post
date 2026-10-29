import { useMemo } from 'react';
import { Download, FileText, Image as ImageIcon } from 'lucide-react';
import type { ApiResponse } from '@/types/domain';
import { Button } from '@/components/ui/button';
import { formatBytes } from '@/lib/utils';
import { downloadResponse, responseDataUrl, responseFileName } from './responseKind';

/**
 * 二进制响应的视图 —— 图片给预览，其余给下载。
 *
 * **不把二进制渲染成文本。** 一张 PNG 的字节按 UTF-8 解出来是一串「��」，用户看到的是
 * 一屏乱码，而那不是响应——那是显示不出来。故这里要么给出图片预览，要么给出下载。
 */
export function BinaryView({ response }: { response: ApiResponse }) {
  const dataUrl = useMemo(() => responseDataUrl(response), [response]);
  const fileName = useMemo(() => responseFileName(response), [response]);

  if (dataUrl) {
    return (
      <div className="flex h-full flex-col items-center gap-3 overflow-auto p-4">
        <img
          src={dataUrl}
          alt={fileName}
          className="max-h-[70vh] max-w-full rounded-md border border-border object-contain"
          data-testid="response-image-preview"
        />
        <DownloadRow response={response} fileName={fileName} />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-4 text-center">
      <div className="flex h-12 w-12 items-center justify-center rounded-full bg-muted/40">
        <FileText className="h-6 w-6 text-muted-foreground/50" />
      </div>
      <p className="text-sm text-muted-foreground">这是一个二进制响应，不能按文本显示</p>
      <DownloadRow response={response} fileName={fileName} />
    </div>
  );
}

/** 下载那一行：文件名 + 体积 + 按钮。 */
function DownloadRow({ response, fileName }: { response: ApiResponse; fileName: string }) {
  return (
    <div className="flex items-center gap-2 text-xs text-muted-foreground">
      <ImageIcon className="h-3.5 w-3.5" aria-hidden />
      <span className="font-mono text-foreground/80">{fileName}</span>
      <span>·</span>
      <span>{formatBytes(response.sizeBytes)}</span>
      <Button
        variant="outline"
        size="sm"
        className="h-7 gap-1.5"
        onClick={() => downloadResponse(response)}
      >
        <Download className="h-3.5 w-3.5" />
        下载
      </Button>
    </div>
  );
}
