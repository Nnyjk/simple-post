import { useState } from 'react';
import { Tabs } from '@/components/ui/tabs';
import { ScriptEditor } from './ScriptEditor-lazy';

/** 两段脚本里当前编辑哪一段。 */
type ScriptTab = 'pre' | 'test';

const SCRIPT_TABS: { id: ScriptTab; label: string }[] = [
  { id: 'pre', label: '前置脚本' },
  { id: 'test', label: '测试脚本' },
];

/**
 * 两段脚本的编辑面。
 *
 * 端点的脚本与 collection 的脚本共用这一个面板：两者形状相同、跑的沙箱相同，
 * 只是存的地方不同（端点在 `endpoint_details` 行里，collection 在自己的表里）。
 */
export function ScriptPanel({
  preRequest,
  test,
  onPreRequestChange,
  onTestChange,
}: {
  preRequest: string;
  test: string;
  onPreRequestChange: (value: string) => void;
  onTestChange: (value: string) => void;
}) {
  const [tab, setTab] = useState<ScriptTab>('pre');

  return (
    <div className="px-4 py-3">
      <div className="mb-2 flex items-center gap-2">
        <Tabs
          items={SCRIPT_TABS.map((t) => ({ id: t.id, label: t.label }))}
          value={tab}
          onChange={(id) => setTab(id as ScriptTab)}
        />
        <span className="text-[10px] text-muted-foreground">
          {tab === 'pre' ? '变量替换之后、请求发出之前' : '响应回来之后'}
        </span>
      </div>

      {tab === 'pre' ? (
        <ScriptEditor
          value={preRequest}
          onChange={onPreRequestChange}
          placeholder="pm.environment.set('token', '…');"
        />
      ) : (
        <ScriptEditor
          value={test}
          onChange={onTestChange}
          placeholder="pm.test('状态码是 200', function () { pm.response.to.have.status(200); });"
        />
      )}

      <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
        脚本在受限沙箱里跑：可用 pm.request / pm.environment / pm.response / pm.test / pm.expect /
        console.log；够不到文件、网络与进程，超时会被打断。脚本失败不影响这次请求的成败。
      </p>
    </div>
  );
}
