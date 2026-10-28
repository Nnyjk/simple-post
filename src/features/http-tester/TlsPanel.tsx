import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { SkipVerifyWarning } from '@/components/tls/skip-verify-warning';
import type { TlsSettings } from '@/types/domain';
import {
  TLS_PATH_FIELDS,
  TLS_PATH_NOTE,
  TLS_SKIP_VERIFY,
  setTlsPath,
  tlsPathValue,
  type TlsPathKey,
} from '@/lib/tls';

/**
 * 端点自己的 HTTPS 出站设置。
 *
 * **合成语义是整体取代，不是逐字段合并**：端点给了这一份就整体顶掉全局默认那份。
 * 故本面板只有一个「继承全局设置」开关加四个字段，没有「逐项覆盖」的中间态。
 *
 * 开关打开时四个字段禁用，且端点不再带自己那份设置（落库时写 `null`）；关闭时四个
 * 字段可编辑，初值取当前全局设置的值，用户在此基础上改。
 *
 * 面板**不持有本地副本**：端点那份设置的每个改动都立刻写进草稿（`onChange`），
 * 于是显示的值始终是草稿里的那一份。端点换了标签时不会带着上一个接口的残留值。
 */
export function TlsPanel({
  value,
  globalSettings,
  onChange,
}: {
  /** 端点自己那份设置。`null` = 继承全局默认。 */
  value: TlsSettings | null;
  /** 全局默认那份（设置面板里改的那份）。 */
  globalSettings: TlsSettings;
  /** 端点这份设置的下一个取值。`null` = 回到继承全局。 */
  onChange: (next: TlsSettings | null) => void;
}) {
  const inherit = value === null;

  // 继承全局时显示全局那份的值（四个字段禁用），否则显示端点自己那份的。
  const shown = inherit ? globalSettings : value;

  /**
   * 改「继承全局设置」这个开关。
   *
   * 关掉继承时端点立刻拿到一份自己的设置，初值取当前全局默认的值；打开继承时端点
   * 不再带自己那份设置（写 `null`），全局默认那份重新生效。
   */
  const handleInheritChange = (next: boolean) => {
    onChange(next ? null : { ...globalSettings });
  };

  /** 改端点自己那份里的一个字段。 */
  const patchOwn = (patch: Partial<TlsSettings>) => {
    if (!value) return;
    onChange({ ...value, ...patch });
  };

  /** 改一个证书路径。空串落库时写成 `null`。 */
  const patchPath = (key: TlsPathKey, raw: string) => {
    if (!value) return;
    onChange(setTlsPath(value, key, tlsPathValue(raw)));
  };

  return (
    <div className="space-y-4 px-4 py-3">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium text-foreground">继承全局设置</div>
          <div className="mt-0.5 text-[11px] text-muted-foreground">
            打开时这个接口用设置面板里的全局默认；关掉后这个接口改用自己的那一份，且整体顶掉全局默认
          </div>
        </div>
        <Switch checked={inherit} onCheckedChange={handleInheritChange} aria-label="继承全局设置" />
      </div>

      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium text-foreground">{TLS_SKIP_VERIFY.label}</div>
          <div className="mt-0.5 text-[11px] text-muted-foreground">
            {TLS_SKIP_VERIFY.description}
          </div>
        </div>
        <Switch
          checked={shown.skipVerify}
          onCheckedChange={(v) => patchOwn({ skipVerify: v })}
          disabled={inherit}
          aria-label={TLS_SKIP_VERIFY.label}
        />
      </div>

      <SkipVerifyWarning skipVerify={shown.skipVerify} />

      {TLS_PATH_FIELDS.map((field) => (
        <div key={field.key}>
          <label className="mb-1.5 block label-caps">{field.label}</label>
          <Input
            value={shown[field.key] ?? ''}
            onChange={(e) => patchPath(field.key, e.target.value)}
            placeholder={field.placeholder}
            disabled={inherit}
            spellCheck={false}
            aria-label={field.label}
            className="font-mono text-sm"
          />
          <p className="mt-1 text-[11px] text-muted-foreground">{field.description}</p>
        </div>
      ))}

      <p className="text-[11px] text-muted-foreground/80">{TLS_PATH_NOTE}</p>
    </div>
  );
}
