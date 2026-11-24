/**
 * 设置页的「日志与诊断」小节。
 *
 * 只导出组件，挂载点由设置页决定：本卡片不读 store、不依赖相邻小节，单独放在任何
 * `SettingsCategory` 列表里都能工作。
 *
 * 导出按宿主分流：桌面形态写进本机日志目录并在文件管理器里选中那个文件，浏览器
 * 形态触发下载。缓冲区是进程级的，故「本次运行」= 这个进程启动至今。
 */
import { useState } from 'react';
import { AlertTriangle, Download, Loader2 } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { SettingsCategory, SettingRow } from '@/features/settings/SettingsCategory';
import { errorText } from '@/lib/error-text';
import { COMMANDS, IS_TAURI_SHELL, invokeCmd } from '@/lib/tauri-commands';
import { saveGeneratedDocument } from '@/lib/transfer-file-save';
import type { LogExport, LogExportFile } from '@/types/logs';

type Scope = 'all' | 'errors';

export function LogExportCard() {
  const [busy, setBusy] = useState<Scope | null>(null);
  const [result, setResult] = useState<string | null>(null);

  async function run(scope: Scope) {
    setBusy(scope);
    setResult(null);
    try {
      if (IS_TAURI_SHELL) {
        // `null` = 没有可导出的内容，且进程没有创建文件（落 0 字节的文件没有意义）。
        const file = await invokeCmd<LogExportFile | null>(COMMANDS.logExportFile, { scope });
        if (file === null) {
          setResult(
            scope === 'errors' ? '本次运行没有错误级日志' : '暂无日志，先复现一次问题再导出',
          );
          return;
        }
        // 只回显路径：`reveal` 失败时命令照常成功，说「已打开文件夹」就成了没发生过的事。
        setResult(`已导出到 ${file.path}`);
        return;
      }
      const doc = await invokeCmd<LogExport>(COMMANDS.logExport, { scope });
      // 空内容不弹保存框：落一个 0 字节的文件对反馈问题毫无用处，用户还会以为导出坏了。
      if (doc.content.trim() === '') {
        setResult(scope === 'errors' ? '本次运行没有错误级日志' : '暂无日志，先复现一次问题再导出');
        return;
      }
      const saved = await saveGeneratedDocument({
        fileName: doc.fileName,
        mediaType: 'text/plain',
        content: doc.content,
        diagnostics: [],
      });
      // 用户取消不是错误：不动上一次的结果文案，只把按钮放开。
      if (saved.status === 'cancelled') return;
      setResult(`已保存 ${saved.fileName ?? doc.fileName}`);
    } catch (e) {
      setResult(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <SettingsCategory
      title="日志与诊断"
      description="导出本次运行期间的应用日志，便于反馈问题"
      icon={<AlertTriangle className="h-3.5 w-3.5" />}
    >
      <SettingRow
        icon={<Download className="h-3.5 w-3.5" />}
        label="导出日志"
        description="含全部级别的后端事件与前端错误，一次一条记录"
        hint={result ?? (IS_TAURI_SHELL ? '导出到日志目录并打开所在文件夹' : '下载到本机')}
        flush
        control={
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void run('all')}
              disabled={busy !== null}
            >
              {busy === 'all' ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Download className="h-3.5 w-3.5" />
              )}
              导出日志
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => void run('errors')}
              disabled={busy !== null}
            >
              {busy === 'errors' ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <AlertTriangle className="h-3.5 w-3.5" />
              )}
              导出错误日志
            </Button>
          </div>
        }
      />
    </SettingsCategory>
  );
}
