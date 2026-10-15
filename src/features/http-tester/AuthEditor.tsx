import { useRef, useState } from 'react';
import { KeyRound } from 'lucide-react';
import {
  type AuthConfig,
  type AuthType,
  type OAuth2Auth,
  type OAuth2ClientAuth,
  type OAuth2GrantType,
  type OAuth2TokenResult,
  type OAuth2TokenLocation,
} from '@/types/domain';
import { Tabs } from '@/components/ui/tabs';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import { KeyValueEditor } from './KeyValueEditor';
import { DEFAULT_OAUTH2, fetchOAuth2Token } from '@/lib/oauth2';

interface Props {
  value: AuthConfig;
  onChange: (auth: AuthConfig) => void;
}

const TYPES: { id: AuthType; label: string }[] = [
  { id: 'none', label: 'No Auth' },
  { id: 'bearer', label: 'Bearer Token' },
  { id: 'basic', label: 'Basic Auth' },
  { id: 'apikey', label: 'API Key' },
  { id: 'oauth2', label: 'OAuth 2.0' },
];

const GRANT_OPTIONS: { value: OAuth2GrantType; label: string }[] = [
  { value: 'authorization_code', label: '授权码（authorization_code）' },
  { value: 'client_credentials', label: '客户端凭证（client_credentials）' },
  { value: 'password', label: '密码（password）' },
  { value: 'implicit', label: '隐式（implicit）' },
];

export function AuthEditor({ value, onChange }: Props) {
  return (
    <div className="px-4 py-3 space-y-3">
      <div>
        <label className="mb-1.5 block label-caps">类型</label>
        <Tabs
          items={TYPES.map((t) => ({ id: t.id, label: t.label }))}
          value={value.kind}
          onChange={(id) => onChange({ ...value, kind: id as AuthType })}
        />
      </div>

      {value.kind === 'none' && (
        <div className="rounded-md border border-dashed border-border p-4 text-center text-sm text-muted-foreground">
          此请求不需要鉴权
        </div>
      )}

      {value.kind === 'bearer' && (
        <div>
          <label className="mb-1.5 block label-caps">Token</label>
          <Input
            value={value.bearer ?? ''}
            onChange={(e) => onChange({ ...value, bearer: e.target.value })}
            placeholder="eyJhbGciOi... 支持 {{var}}"
            className="font-mono text-sm"
          />
          <p className="mt-1.5 text-[11px] text-muted-foreground">
            将以 <code className="rounded bg-muted px-1">Authorization: Bearer ...</code> 发送
          </p>
        </div>
      )}

      {value.kind === 'basic' && (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-1.5 block label-caps">Username</label>
            <Input
              value={value.basic?.username ?? ''}
              onChange={(e) =>
                onChange({
                  ...value,
                  basic: { username: e.target.value, password: value.basic?.password ?? '' },
                })
              }
              className="font-mono text-sm"
            />
          </div>
          <div>
            <label className="mb-1.5 block label-caps">Password</label>
            <Input
              type="password"
              value={value.basic?.password ?? ''}
              onChange={(e) =>
                onChange({
                  ...value,
                  basic: { username: value.basic?.username ?? '', password: e.target.value },
                })
              }
              className="font-mono text-sm"
            />
          </div>
        </div>
      )}

      {value.kind === 'apikey' && (
        <div className="space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <div className="col-span-1">
              <label className="mb-1.5 block label-caps">Add to</label>
              <Select
                value={value.apikey?.inLocation ?? 'header'}
                onChange={(e) =>
                  onChange({
                    ...value,
                    apikey: {
                      key: value.apikey?.key ?? '',
                      value: value.apikey?.value ?? '',
                      inLocation: e.target.value as 'header' | 'query',
                    },
                  })
                }
                options={[
                  { value: 'header', label: 'Header' },
                  { value: 'query', label: 'Query Params' },
                ]}
              />
            </div>
            <div className="col-span-2">
              <label className="mb-1.5 block label-caps">Key</label>
              <Input
                value={value.apikey?.key ?? ''}
                onChange={(e) =>
                  onChange({
                    ...value,
                    apikey: {
                      key: e.target.value,
                      value: value.apikey?.value ?? '',
                      inLocation: value.apikey?.inLocation ?? 'header',
                    },
                  })
                }
                placeholder="X-API-Key"
                className="font-mono text-sm"
              />
            </div>
          </div>
          <div>
            <label className="mb-1.5 block label-caps">Value</label>
            <Input
              value={value.apikey?.value ?? ''}
              onChange={(e) =>
                onChange({
                  ...value,
                  apikey: {
                    key: value.apikey?.key ?? '',
                    value: e.target.value,
                    inLocation: value.apikey?.inLocation ?? 'header',
                  },
                })
              }
              placeholder="支持 {{var}}"
              className="font-mono text-sm"
            />
          </div>
        </div>
      )}

      {value.kind === 'oauth2' && (
        <OAuth2Fields value={value} cfg={value.oauth2 ?? DEFAULT_OAUTH2} onChange={onChange} />
      )}
    </div>
  );
}

/**
 * OAuth 2.0 表单。
 *
 * `cfg` 恒为一份完整配置：`AuthConfig.oauth2` 可以缺省（老数据、MCP 只写了
 * `kind`），此时渲染回落到 `DEFAULT_OAUTH2` 而**不**往 store 补写——那会把用户
 * 没配过的字段记成配过的。
 */
function OAuth2Fields({
  value,
  cfg,
  onChange,
}: {
  value: AuthConfig;
  cfg: OAuth2Auth;
  onChange: (auth: AuthConfig) => void;
}) {
  const [pending, setPending] = useState(false);
  const [token, setToken] = useState<OAuth2TokenResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  /**
   * 在途取 token 的序号。改配置与发起取 token 都让它自增，结果回来时只有序号仍是
   * 当前值才写 state。
   *
   * 少了它就会出现这一串：点「获取 Token」→ 在返回前改掉 `tokenUrl` → 晚到的响应
   * 按旧配置 setToken → 界面对**新**配置显示「刚获取」。`pending` 挡不住这个，它只
   * 管按钮点两下。
   */
  const reqSeq = useRef(0);

  // 改配置即作废上一次的结果：那个 token 是**改之前**的配置拿到的，继续显示会让人
  // 以为新配置已经生效。同时作废在途的那一次，否则它的晚到结果会绕过上面那句。
  const patch = (next: Partial<OAuth2Auth>) => {
    reqSeq.current += 1;
    setToken(null);
    setError(null);
    onChange({ ...value, oauth2: { ...cfg, ...next } });
  };

  const grant = cfg.grantType;
  const isImplicit = grant === 'implicit';
  const needsAuthorizationUrl = grant === 'authorization_code' || isImplicit;

  const acquire = async () => {
    if (pending) return;
    const seq = ++reqSeq.current;
    setPending(true);
    setError(null);
    try {
      // 传 `cfg` 而不是 `value.oauth2`：载荷缺省时后者是 `undefined`，后端会按
      // 「缺少 oauth2 载荷」拒绝，而这个表单明明渲染得出完整配置。
      const auth: AuthConfig = { kind: 'oauth2', oauth2: cfg };
      // 隐式流程没有 token 端点可调，token 由用户从浏览器粘回来；其余流程走
      // `force` 让后端忽略缓存重取一次。
      const t = await fetchOAuth2Token(
        auth,
        isImplicit ? { manual: cfg.authCode } : { force: true },
      );
      if (seq === reqSeq.current) setToken(t);
    } catch (err) {
      if (seq !== reqSeq.current) return;
      setToken(null);
      setError((err as Error)?.message ?? String(err));
    } finally {
      if (seq === reqSeq.current) setPending(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="mb-1.5 block label-caps">流程</label>
          <Select
            value={grant}
            onChange={(e) => patch({ grantType: e.target.value as OAuth2GrantType })}
            options={GRANT_OPTIONS}
          />
        </div>
        <div>
          <label className="mb-1.5 block label-caps">客户端凭据发送方式</label>
          <Select
            value={cfg.clientAuth}
            onChange={(e) => patch({ clientAuth: e.target.value as OAuth2ClientAuth })}
            options={[
              { value: 'basic', label: 'Basic 请求头' },
              { value: 'body', label: '表单字段' },
            ]}
          />
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="mb-1.5 block label-caps">Client ID</label>
          <Input
            value={cfg.clientId}
            onChange={(e) => patch({ clientId: e.target.value })}
            className="font-mono text-sm"
          />
        </div>
        <div>
          <label className="mb-1.5 block label-caps">Client Secret</label>
          <Input
            type="password"
            value={cfg.clientSecret}
            onChange={(e) => patch({ clientSecret: e.target.value })}
            className="font-mono text-sm"
          />
        </div>
      </div>

      <div>
        <label className="mb-1.5 block label-caps">Scope</label>
        <Input
          value={cfg.scope}
          onChange={(e) => patch({ scope: e.target.value })}
          placeholder="空格分隔，留空用服务端默认值"
          className="font-mono text-sm"
        />
      </div>

      {!isImplicit && (
        <div>
          <label className="mb-1.5 block label-caps">Token URL</label>
          <Input
            value={cfg.tokenUrl}
            onChange={(e) => patch({ tokenUrl: e.target.value })}
            placeholder="https://idp.example.com/oauth2/token"
            className="font-mono text-sm"
          />
        </div>
      )}

      {needsAuthorizationUrl && (
        <div>
          <label className="mb-1.5 block label-caps">Authorization URL</label>
          <Input
            value={cfg.authorizationUrl}
            onChange={(e) => patch({ authorizationUrl: e.target.value })}
            placeholder="https://idp.example.com/oauth2/authorize"
            className="font-mono text-sm"
          />
          <p className="mt-1.5 text-[11px] text-muted-foreground">
            本应用不打开浏览器，也不监听回环端口 —— 复制这个地址去登录，再把回调内容粘到下面
          </p>
        </div>
      )}

      {grant === 'authorization_code' && (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-1.5 block label-caps">Redirect URI</label>
            <Input
              value={cfg.redirectUri}
              onChange={(e) => patch({ redirectUri: e.target.value })}
              placeholder="须与 IdP 侧登记的一致"
              className="font-mono text-sm"
            />
          </div>
          <div>
            <label className="mb-1.5 block label-caps">Code Verifier</label>
            <Input
              value={cfg.codeVerifier}
              onChange={(e) => patch({ codeVerifier: e.target.value })}
              placeholder="PKCE，需与 IdP 侧登记的 challenge 一致，本应用不自动生成"
              className="font-mono text-sm"
            />
          </div>
        </div>
      )}

      {needsAuthorizationUrl && (
        <div>
          <label className="mb-1.5 block label-caps">
            {isImplicit ? '浏览器回带的值' : '授权码'}
          </label>
          <Input
            value={cfg.authCode}
            onChange={(e) => patch({ authCode: e.target.value })}
            placeholder={
              isImplicit
                ? '整条回调 URL 或裸 token'
                : '在浏览器登录后，把回调 URL 或其中的 code 粘到这里'
            }
            className="font-mono text-sm"
          />
        </div>
      )}

      {grant === 'password' && (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-1.5 block label-caps">Username</label>
            <Input
              value={cfg.username}
              onChange={(e) => patch({ username: e.target.value })}
              className="font-mono text-sm"
            />
          </div>
          <div>
            <label className="mb-1.5 block label-caps">Password</label>
            <Input
              type="password"
              value={cfg.password}
              onChange={(e) => patch({ password: e.target.value })}
              className="font-mono text-sm"
            />
          </div>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="mb-1.5 block label-caps">Token 放在</label>
          <Select
            value={cfg.tokenLocation}
            onChange={(e) => patch({ tokenLocation: e.target.value as OAuth2TokenLocation })}
            options={[
              { value: 'header', label: '请求头 Authorization' },
              { value: 'query', label: '查询参数' },
            ]}
          />
        </div>
        {cfg.tokenLocation === 'query' && (
          <div>
            <label className="mb-1.5 block label-caps">参数名</label>
            <Input
              value={cfg.tokenParam}
              onChange={(e) => patch({ tokenParam: e.target.value })}
              placeholder="access_token"
              className="font-mono text-sm"
            />
          </div>
        )}
      </div>

      <div>
        <label className="mb-1.5 block label-caps">额外参数</label>
        {/* KeyValueEditor 自带 `px-4 py-3`，负外边距把它对齐到其余字段的左边界。 */}
        <div className="-mx-4">
          <KeyValueEditor
            items={cfg.extraParams}
            onChange={(extraParams) => patch({ extraParams })}
            keyPlaceholder="audience"
            valuePlaceholder="https://api.example.com"
          />
        </div>
      </div>

      <div className="space-y-1.5">
        <Button
          variant="outline"
          size="sm"
          onClick={acquire}
          disabled={pending}
          className="gap-1.5"
        >
          <KeyRound className="h-3.5 w-3.5" />
          {pending ? '处理中…' : isImplicit ? '保存 Token' : '获取 Token'}
        </Button>
        {token && (
          <p className="text-[11px] text-muted-foreground">
            {token.expiresAt === null
              ? '过期时间未知'
              : `有效期至 ${new Date(token.expiresAt * 1000).toLocaleTimeString()}`}
            {' · '}
            {token.refreshed ? '刚获取' : '命中缓存'}
          </p>
        )}
        {error && <p className="text-[11px] text-destructive">{error}</p>}
      </div>
    </div>
  );
}
