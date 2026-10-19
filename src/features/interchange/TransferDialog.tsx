import { useMemo, useRef, useState } from 'react';
import { FileInput, Link2 } from 'lucide-react';

import type { Environment } from '@/types/domain';
import type { ExportPreview, ImportPreview, TransferDiagnostic } from '@/lib/interchange-types';
import { DOC_FORMATS, SEVERITY_LABEL, SEVERITY_VARIANT } from '@/lib/interchange-types';
import { cn, focusRingBare } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Dialog } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { TransferTree } from './TransferTree';
import type { TransferController } from './useTransfer';

interface TransferDialogProps {
  controller: TransferController;
  /** 导出时要选的环境。项目内全部环境，由 store 给出。 */
  environments: Environment[];
  onClose: () => void;
}

/**
 * 导入与导出**共用**这一个弹窗。两个模式只差三处：来源选择、导出那两项设置
 * （环境 / 格式）、以及结尾的落定动作。中间那屏——计数、勾选树、诊断——完全相同，
 * 故它不是「两套界面长得像」，而是同一段代码跑在两种数据上。
 */
export function TransferDialog({ controller, environments, onClose }: TransferDialogProps) {
  const { session, phase, error, preview, result } = controller;
  if (!session) return null;

  const isImport = session.mode === 'import';
  const title = isImport
    ? `导入到模块「${session.moduleName}」`
    : `导出模块「${session.moduleName}」`;
  const busy = phase === 'working';

  return (
    <Dialog
      open
      // 勾选树是阻塞式的：点背景关掉它会让人以为刚才勾的都没了。
      disableBackdropClose
      onOpenChange={(o) => {
        if (!o && !busy) onClose();
      }}
      title={title}
      description={describe(preview, session.mode)}
      className="max-w-2xl"
      footer={<Footer controller={controller} isImport={isImport} />}
    >
      {error && (
        <p className="mb-3 rounded-md border border-destructive/30 bg-destructive/10 px-2.5 py-2 text-[11px] text-destructive">
          {error}
        </p>
      )}

      {phase === 'source' && isImport && <SourceStep controller={controller} />}
      {busy && (
        <p className="py-8 text-center text-xs text-muted-foreground">
          {isImport ? '正在解析…' : '正在生成…'}
        </p>
      )}
      {phase === 'preview' && (
        <PreviewStep controller={controller} environments={environments} isImport={isImport} />
      )}
      {phase === 'done' && result && <DoneStep result={result} />}
    </Dialog>
  );
}

function describe(preview: ImportPreview | ExportPreview | null, mode: 'import' | 'export') {
  if (!preview) return mode === 'import' ? 'Postman 集合 v2.1 或 OpenAPI 3.0 文档' : undefined;
  const bits: string[] = [];
  if ('formatLabel' in preview) bits.push(preview.formatLabel);
  // `title` 只有导入侧有：导出的标题就是模块名，已经在弹窗标题里了。
  if ('title' in preview && preview.title) bits.push(preview.title);
  if ('sourceName' in preview && preview.sourceName) bits.push(preview.sourceName);
  return bits.join(' · ') || undefined;
}

/** 来源：文件或链接。文件走 `<input type=file>`，两种形态下都可用，不需要插件权限。 */
function SourceStep({ controller }: { controller: TransferController }) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [url, setUrl] = useState('');

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <input
          ref={fileRef}
          type="file"
          accept=".json,.yaml,.yml"
          className="hidden"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (!file) return;
            void file.text().then((text) => controller.previewFromContent(text));
            // 选同一个文件两次要能再触发一次 change，故先清空。
            e.target.value = '';
          }}
        />
        <Button variant="outline" size="sm" onClick={() => fileRef.current?.click()}>
          <FileInput className="h-3.5 w-3.5" />
          选择文件…
        </Button>
        <span className="text-[11px] text-muted-foreground">或</span>
        <Input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && url.trim()) controller.previewFromUrl(url.trim());
          }}
          placeholder="文档链接"
          className="h-8 flex-1 text-xs"
        />
        <Button
          variant="ghost"
          size="sm"
          disabled={!url.trim()}
          onClick={() => controller.previewFromUrl(url.trim())}
        >
          <Link2 className="h-3.5 w-3.5" />
          拉取
        </Button>
      </div>
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        支持 Postman 集合 v2.1（.json）与 OpenAPI 3.0 文档（.yaml / .yml / .json）。
        解析只在本地进行，不会修改项目里的任何内容——看完中间态、勾完才落库。
      </p>
    </div>
  );
}

function PreviewStep({
  controller,
  environments,
  isImport,
}: {
  controller: TransferController;
  environments: Environment[];
  isImport: boolean;
}) {
  const {
    preview,
    groups,
    diagnostics,
    total,
    chosen,
    selected,
    environmentId,
    format,
    setEnvironmentId,
    setFormat,
  } = controller;
  const counts = preview?.counts;
  const warned = diagnostics.filter((d) => d.severity !== 'info');
  const [showInfo, setShowInfo] = useState(false);
  const info = diagnostics.filter((d) => d.severity === 'info');

  const missingServers = useMemo(
    () =>
      isImport
        ? []
        : ((preview as ExportPreview | null)?.servers ?? [])
            .filter((m) => m.servers.length === 0)
            .map((m) => m.moduleName),
    [isImport, preview],
  );

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        {counts && (
          <>
            <Badge variant="muted">{counts.groups} 个集合</Badge>
            <Badge variant="muted">{counts.endpoints} 个接口</Badge>
            {counts.duplicates > 0 && <Badge variant="warning">{counts.duplicates} 个已存在</Badge>}
          </>
        )}
        {isImport && (preview as ImportPreview | null)?.environments?.length ? (
          <Badge variant="outline">
            将新建 {(preview as ImportPreview).environments.length} 个环境
          </Badge>
        ) : null}
        <div className="flex-1" />
        <ToolbarButton onClick={controller.selectAll}>全选</ToolbarButton>
        <ToolbarButton onClick={controller.selectNone}>全不选</ToolbarButton>
        <ToolbarButton onClick={controller.invert}>反选</ToolbarButton>
        <span className="ml-1 text-[11px] tabular-nums text-muted-foreground">
          已选 {chosen} / {total}
        </span>
      </div>

      {!isImport && (
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-muted-foreground">环境</span>
          <Select
            value={environmentId}
            onChange={(e) => setEnvironmentId(e.target.value)}
            options={[
              { value: '', label: '不选环境（不输出 servers）' },
              ...environments.map((e) => ({ value: e.id, label: e.name })),
            ]}
            className="h-7 text-[11px]"
          />
          <div className="flex-1" />
          <SegmentedControl value={format} onChange={setFormat} />
        </div>
      )}

      {!isImport && missingServers.length > 0 && (
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          <span className="text-warning-foreground">该环境下没有配置服务 URL</span>：
          {missingServers.join('、')}。这些模块不会输出 <code className="font-mono">servers</code>
          ，也不会借用别的模块的地址或项目的 ★。
        </p>
      )}

      <div className="max-h-[52vh] overflow-y-auto rounded-md border border-border bg-background/40">
        <TransferTree
          groups={groups}
          selected={selected}
          onToggleEndpoint={controller.toggleEndpoint}
          onToggleGroup={controller.toggleGroup}
        />
      </div>

      {(warned.length > 0 || info.length > 0) && (
        <div className="rounded-md border border-border/60">
          <button
            type="button"
            onClick={() => setShowInfo(!showInfo)}
            className={cn(
              'flex w-full items-center gap-2 px-2.5 py-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-overlay-hover',
              focusRingBare,
            )}
          >
            诊断
            <Badge variant={warned.length > 0 ? 'warning' : 'muted'}>
              {warned.length > 0 ? `${warned.length} 条需要注意` : `${info.length} 条提示`}
            </Badge>
            <span className="flex-1" />
            {info.length > 0 && <span>{showInfo ? '只看需要注意的' : '显示全部'}</span>}
          </button>
          <div className="max-h-40 overflow-y-auto border-t border-border/60">
            {(showInfo ? diagnostics : warned).map((d, i) => (
              <DiagnosticRow key={`${d.pointer}-${i}`} d={d} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function DiagnosticRow({ d }: { d: TransferDiagnostic }) {
  return (
    <div className="flex items-start gap-2 border-b border-border/40 px-2.5 py-1.5 last:border-b-0">
      <Badge variant={SEVERITY_VARIANT[d.severity] ?? 'muted'} className="mt-px">
        {SEVERITY_LABEL[d.severity] ?? d.severity}
      </Badge>
      <div className="min-w-0 flex-1">
        <p className="text-[11px] text-foreground">{d.message}</p>
        <p className="mt-0.5 font-mono text-[10px] text-faint">{d.pointer}</p>
        {d.dropped && <p className="mt-0.5 text-[10px] text-faint">已丢弃：{d.dropped}</p>}
      </div>
    </div>
  );
}

function DoneStep({ result }: { result: NonNullable<TransferController['result']> }) {
  return (
    <div className="space-y-2">
      <p className="text-xs text-foreground">
        新增 <span className="font-medium tabular-nums">{result.createdEndpoints}</span> 个接口
        {result.skippedDuplicates > 0 && (
          <>
            ，按指纹跳过{' '}
            <span className="font-medium tabular-nums">{result.skippedDuplicates}</span> 个
          </>
        )}
        。
      </p>
      {result.environmentsCreated.length > 0 && (
        <p className="text-[11px] text-muted-foreground">
          新建环境：{result.environmentsCreated.join('、')}
        </p>
      )}
      {result.diagnostics.filter((d) => d.severity !== 'info').length > 0 && (
        <div className="max-h-32 overflow-y-auto rounded-md border border-border/60">
          {result.diagnostics
            .filter((d) => d.severity !== 'info')
            .map((d, i) => (
              <DiagnosticRow key={`${d.pointer}-${i}`} d={d} />
            ))}
        </div>
      )}
    </div>
  );
}

function Footer({ controller, isImport }: { controller: TransferController; isImport: boolean }) {
  const { phase, chosen } = controller;
  const busy = phase === 'working';
  if (phase === 'done') {
    return (
      <Button size="sm" onClick={controller.close}>
        完成
      </Button>
    );
  }
  if (phase === 'source') {
    return (
      <Button variant="ghost" size="sm" onClick={controller.close}>
        取消
      </Button>
    );
  }
  return (
    <>
      <Button variant="ghost" size="sm" disabled={busy} onClick={controller.close}>
        取消
      </Button>
      {isImport ? (
        <Button size="sm" disabled={busy || chosen === 0} onClick={controller.commit}>
          导入选中的 {chosen} 个
        </Button>
      ) : (
        <>
          <Button
            variant="ghost"
            size="sm"
            disabled={busy || chosen === 0}
            onClick={() => void controller.save(true)}
          >
            复制
          </Button>
          <Button
            size="sm"
            disabled={busy || chosen === 0}
            onClick={() => void controller.save(false)}
          >
            导出选中的 {chosen} 个
          </Button>
        </>
      )}
    </>
  );
}

/** 工具条上的小字按钮。不是 `Button`：这几个不占视觉主位，用 ghost 更合适。 */
function ToolbarButton({ children, onClick }: { children: React.ReactNode; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'rounded-sm px-1.5 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground',
        focusRingBare,
      )}
    >
      {children}
    </button>
  );
}

/** 与 `AgentPermissionsTable` 同一条凹槽的 JSON / YAML 切换。 */
function SegmentedControl({
  value,
  onChange,
}: {
  value: 'json' | 'yaml';
  onChange: (v: 'json' | 'yaml') => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="输出格式"
      className="inline-flex items-center gap-1 rounded-md border border-border/70 bg-muted/40 p-0.5"
    >
      {DOC_FORMATS.map((f) => {
        const active = value === f.value;
        return (
          <button
            key={f.value}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(f.value)}
            className={cn(
              'inline-flex h-7 items-center rounded-sm px-3 text-[11px] font-medium transition-colors',
              focusRingBare,
              active
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:bg-accent hover:text-foreground',
            )}
          >
            {f.label}
          </button>
        );
      })}
    </div>
  );
}
