import { useEffect, useState } from 'react';
import { Dialog } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { NodeScripts } from '@/types/domain';
import { ScriptPanel } from '@/features/http-tester/ScriptPanel';

/**
 * collection 上的两段脚本。
 *
 * 与端点的脚本共用一个编辑面板（同一个 `ScriptPanel`）：形状相同、跑的沙箱相同，
 * 只是存的地方不同——端点的在 `endpoint_details` 行里，collection 的在自己的表里。
 *
 * **没有草稿态**：这个对话框是「打开—改—保存」的一步式，不进 store。collection 的
 * 脚本不参与端点编辑器的脏标记体系，给它另开一套只会多出一条要同步的路径。
 */
export function CollectionScriptDialog({
  open,
  onOpenChange,
  nodeId,
  nodeName,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  nodeId: string;
  nodeName: string;
}) {
  const [preRequest, setPreRequest] = useState('');
  const [test, setTest] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // 打开时读一次。没配置过脚本不是错误：后端回一份全空的。
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setError(null);
    void (async () => {
      try {
        const scripts = await invokeCmd<NodeScripts>(COMMANDS.nodeScriptsGet, { nodeId });
        if (cancelled) return;
        setPreRequest(scripts.preRequestScript ?? '');
        setTest(scripts.testScript ?? '');
      } catch (err) {
        if (!cancelled) setError((err as Error)?.message ?? String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, nodeId]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await invokeCmd(COMMANDS.nodeScriptsUpdate, {
        nodeId,
        patch: { preRequestScript: preRequest, testScript: test },
      });
      onOpenChange(false);
    } catch (err) {
      setError((err as Error)?.message ?? String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`集合脚本 · ${nodeName}`}
      description="集合里的每个接口都会跑这两段脚本：前置脚本在请求发出之前，测试脚本在响应回来之后。"
      className="max-w-3xl"
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button size="sm" onClick={() => void save()} disabled={saving}>
            {saving ? '保存中…' : '保存'}
          </Button>
        </>
      }
    >
      {error && <div className="alert-error-inline mb-2">{error}</div>}
      <ScriptPanel
        preRequest={preRequest}
        test={test}
        onPreRequestChange={setPreRequest}
        onTestChange={setTest}
      />
    </Dialog>
  );
}
