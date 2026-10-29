import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Archive,
  Hourglass,
  Repeat,
  Database,
  RotateCcw,
  Send,
  ShieldCheck,
  ShieldAlert,
  FileKey,
  KeyRound,
  LockKeyhole,
} from 'lucide-react';
import { useAppStore } from '@/stores/app-store';
import {
  parseHistoryKeepDraft,
  REQUEST_SETTINGS_DEFAULTS,
  RESPONSE_HISTORY_KEEP_DEFAULT,
  RESPONSE_HISTORY_KEEP_MAX,
} from '@/stores/slices/ui-prefs';
import type { RequestSettings } from '@/stores/store-types';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { SettingsCategory, SettingRow } from '../SettingsCategory';
import { GroupHeader } from './rows/GroupHeader';
import { SkipVerifyWarning } from '@/components/tls/skip-verify-warning';
import {
  TLS_PATH_FIELDS,
  TLS_PATH_NOTE,
  TLS_SKIP_VERIFY,
  setTlsPath,
  tlsEquals,
  tlsPathValue,
} from '@/lib/tls';

// 「恢复默认恢复到哪里」的唯一真相。做成常量，好让「恢复默认」处理器与禁用态判定对
// 「什么算被改动过」保持一致。
//
// 取自 store 切片里的那一份：后端 `RequestSettings::default()` 与它是同一个数，
// 界面与 MCP 侧的缺省值因此一致。
const REQUEST_DEFAULTS = REQUEST_SETTINGS_DEFAULTS;

/**
 * 小节里的设置项数：三项出站旋钮 + HTTPS 出站的四项。
 *
 * 不从 `REQUEST_DEFAULTS` 的键数算——`tls` 是四个字段的一组，算成一项会让页面上
 * 少报三项。
 */
const SETTING_COUNT = 7;

/** 三个证书路径输入框的前导图标，按它们在出站请求里各管的事区分。 */
const PATH_ICONS = {
  caCertificatePath: <FileKey className="h-3.5 w-3.5" />,
  clientCertificatePath: <KeyRound className="h-3.5 w-3.5" />,
  clientKeyPath: <LockKeyhole className="h-3.5 w-3.5" />,
} as const;

/**
 * 这一小节是否已偏离缺省值。
 *
 * 提成纯函数是为了能单独测它：「恢复默认」按钮的可用状态就是它的取反，而那一段
 * 判断原先只长在组件里，改了哪一项都要靠人肉读组件才知道有没有被算进去。
 */
export function isRequestSettingsDirty(settings: RequestSettings): boolean {
  return (
    settings.timeoutMs !== REQUEST_DEFAULTS.timeoutMs ||
    settings.followRedirects !== REQUEST_DEFAULTS.followRedirects ||
    settings.maxResponseSizeKb !== REQUEST_DEFAULTS.maxResponseSizeKb ||
    !tlsEquals(settings.tls, REQUEST_DEFAULTS.tls)
  );
}

/**
 * 请求行为 —— 作用于每一次外发 HTTP 调用。
 *
 * 七行，各带标签、一句说明默认值*为何*重要的描述，以及右侧的控件。前三行是出站
 * 旋钮，后四行是 HTTPS 出站设置（一个开关 + 三个证书路径）。「恢复默认」按钮放在
 * 小节表头，且只有当至少一行已偏离 `REQUEST_DEFAULTS` 时才可用，免得这个便利性一直引诱
 * 用户去「重置」本来就已是默认值的东西。
 *
 * 本组件还渲染第二张卡片「历史保留」：发送历史的保留条数也是全局的，但它不是出站请求的
 * 旋钮（改它不影响任何一次发送的形态），故不与上面那几行混在一张卡里。
 */
export function RequestSection() {
  const settings = useAppStore((s) => s.requestSettings);
  const updateRequestSettings = useAppStore((s) => s.updateRequestSettings);
  const historyKeep = useAppStore((s) => s.responseHistoryKeep);
  const updateResponseHistoryKeep = useAppStore((s) => s.updateResponseHistoryKeep);

  const isDirty = useMemo(() => isRequestSettingsDirty(settings), [settings]);

  const resetToDefaults = () => updateRequestSettings({ ...REQUEST_DEFAULTS });

  // 保留条数先落在草稿里，失焦或回车才写回。
  //
  // 每次按键就写回是不行的：写回会立刻按新条数裁库，而裁掉的行不会回来。用户把 100
  // 改成 50 时第一键送出的 5 已经把库裁到 5 条，第二键送出的 50 只是让剩下的更少。
  const [keepDraft, setKeepDraft] = useState(String(historyKeep));

  // 别人改了保留条数时草稿跟着回到当前值，否则输入框里留着旧数字。
  useEffect(() => {
    setKeepDraft(String(historyKeep));
  }, [historyKeep]);

  const commitKeepDraft = useCallback(() => {
    const n = parseHistoryKeepDraft(keepDraft);
    if (n === null) {
      setKeepDraft(String(historyKeep));
      return;
    }
    updateResponseHistoryKeep(n);
  }, [historyKeep, keepDraft, updateResponseHistoryKeep]);

  return (
    <>
      <SettingsCategory
        title="请求行为"
        description="应用到所有 HTTP 发送（v0.2 由 Rust 端消费）"
        icon={<Send className="h-3.5 w-3.5" />}
        meta={`${SETTING_COUNT} 项`}
        actions={
          <Button
            variant="ghost"
            size="xs"
            onClick={resetToDefaults}
            disabled={!isDirty}
            className="gap-1 text-muted-foreground"
          >
            <RotateCcw className="h-3 w-3" />
            恢复默认
          </Button>
        }
        hideDivider
      >
        <SettingRow
          icon={<Hourglass className="h-3.5 w-3.5" />}
          label="请求超时"
          description="单次请求最长等待时间。超过即中断并返回超时错误"
          hint={`默认 ${REQUEST_DEFAULTS.timeoutMs.toLocaleString()} ms`}
          flush
          control={
            <Input
              type="number"
              min={1000}
              step={1000}
              value={settings.timeoutMs}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isFinite(n) && n >= 1000) {
                  updateRequestSettings({ timeoutMs: n });
                }
              }}
              className="h-8 w-32 font-mono text-xs"
            />
          }
        />

        <SettingRow
          icon={<Repeat className="h-3.5 w-3.5" />}
          label="跟随重定向"
          description="自动跟随 3xx，最多 10 次"
          hint={settings.followRedirects ? '开启 · 自动处理 3xx' : '关闭 · 手动处理 3xx'}
          control={
            <Switch
              checked={settings.followRedirects}
              onCheckedChange={(v) => updateRequestSettings({ followRedirects: v })}
            />
          }
        />

        <SettingRow
          icon={<Database className="h-3.5 w-3.5" />}
          label="最大响应体积"
          description="超过此体积的响应会被截断，避免本地内存被异常响应撑爆"
          hint={`默认 ${REQUEST_DEFAULTS.maxResponseSizeKb.toLocaleString()} KB`}
          control={
            <Input
              type="number"
              min={1}
              step={64}
              value={settings.maxResponseSizeKb}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isFinite(n) && n >= 1) {
                  updateRequestSettings({ maxResponseSizeKb: n });
                }
              }}
              className="h-8 w-32 font-mono text-xs"
            />
          }
        />

        {/* HTTPS 出站 —— 一个分组副标题把下面四行与上面三项出站旋钮分开。 */}
        <GroupHeader
          icon={<ShieldCheck className="h-3.5 w-3.5" />}
          title="HTTPS 出站"
          description="出站请求怎么校验服务端证书、出示什么客户端证书"
        />

        <SettingRow
          icon={<ShieldAlert className="h-3.5 w-3.5" />}
          label={TLS_SKIP_VERIFY.label}
          description={TLS_SKIP_VERIFY.description}
          hint={settings.tls.skipVerify ? '已开启 · 不再校验服务端证书' : '关闭 · 校验服务端证书'}
          flush
          control={
            <Switch
              checked={settings.tls.skipVerify}
              onCheckedChange={(v) =>
                updateRequestSettings({ tls: { ...settings.tls, skipVerify: v } })
              }
              aria-label={TLS_SKIP_VERIFY.label}
            />
          }
        />

        {/* 警示只在开关打开时出现：开关关着时面板不该断言一个并不存在的中间人风险。
            端点那侧的「证书」页签用同一个组件（见 `SkipVerifyWarning`）。 */}
        <SkipVerifyWarning skipVerify={settings.tls.skipVerify} className="mx-4" />

        {TLS_PATH_FIELDS.map((field) => (
          <SettingRow
            key={field.key}
            icon={PATH_ICONS[field.key]}
            label={field.label}
            description={field.description}
            control={
              <Input
                value={settings.tls[field.key] ?? ''}
                onChange={(e) =>
                  updateRequestSettings({
                    tls: setTlsPath(settings.tls, field.key, tlsPathValue(e.target.value)),
                  })
                }
                placeholder={field.placeholder}
                spellCheck={false}
                aria-label={field.label}
                className="h-8 w-72 font-mono text-xs"
              />
            }
          />
        ))}

        <p className="px-4 py-2 text-[11px] text-muted-foreground/80">{TLS_PATH_NOTE}</p>

        {/* 同一张卡片内的页脚 —— 一句小提醒，说明这里的取值在当前管线中才真正起作用。
            让用户不至于跑去看文档也知道自己身在何处。 */}
        <div className="flex items-center gap-2 border-t border-border/60 bg-muted/20 px-4 py-2 text-[11px] text-muted-foreground">
          <Badge variant="muted" className="font-mono">
            v0.2
          </Badge>
          <span>
            以下设置（超时 / 跟随重定向 / 最大响应体积 / HTTPS 出站）仅在真实请求模式下生效；HTTPS
            出站那几项同时决定 MCP 侧发请求时怎么校验证书。
          </span>
        </div>
      </SettingsCategory>

      {/* 发送历史的保留条数。与上面那几行分属两张卡片：它改的不是「请求怎么发」，而是
        「发过的记录留多少条」，两者的适用范围不同。 */}
      <SettingsCategory
        title="历史保留"
        description="每个接口保留多少条未收藏的发送历史，跨项目共用同一个条数"
        icon={<Archive className="h-3.5 w-3.5" />}
        meta="1 项"
        hideDivider
      >
        <SettingRow
          icon={<Archive className="h-3.5 w-3.5" />}
          label="历史保留条数"
          description="每个接口保留最近多少条未收藏的发送历史。收藏的行不计入这个条数，永久保存"
          hint={`默认 ${RESPONSE_HISTORY_KEEP_DEFAULT} 条 · 取值范围 1 – ${RESPONSE_HISTORY_KEEP_MAX}`}
          flush
          control={
            <Input
              type="number"
              min={1}
              max={RESPONSE_HISTORY_KEEP_MAX}
              step={1}
              value={keepDraft}
              onChange={(e) => setKeepDraft(e.target.value)}
              onBlur={commitKeepDraft}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  commitKeepDraft();
                }
              }}
              className="h-8 w-32 font-mono text-xs"
            />
          }
        />
      </SettingsCategory>
    </>
  );
}
