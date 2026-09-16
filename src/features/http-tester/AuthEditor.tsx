import { type AuthConfig, type AuthType } from '@/types/domain';
import { Tabs } from '@/components/ui/tabs';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';

interface Props {
  value: AuthConfig;
  onChange: (auth: AuthConfig) => void;
}

const TYPES: { id: AuthType; label: string }[] = [
  { id: 'none', label: 'No Auth' },
  { id: 'bearer', label: 'Bearer Token' },
  { id: 'basic', label: 'Basic Auth' },
  { id: 'apikey', label: 'API Key' },
];

export function AuthEditor({ value, onChange }: Props) {
  return (
    <div className="px-4 py-3 space-y-3">
      <div>
        <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          类型
        </label>
        <Tabs
          items={TYPES.map((t) => ({ id: t.id, label: t.label }))}
          value={value.type}
          onChange={(id) => onChange({ ...value, type: id as AuthType })}
        />
      </div>

      {value.type === 'none' && (
        <div className="rounded-md border border-dashed border-border p-4 text-center text-sm text-muted-foreground">
          此请求不需要鉴权
        </div>
      )}

      {value.type === 'bearer' && (
        <div>
          <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
            Token
          </label>
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

      {value.type === 'basic' && (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              Username
            </label>
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
            <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              Password
            </label>
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

      {value.type === 'apikey' && (
        <div className="space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <div className="col-span-1">
              <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                Add to
              </label>
              <Select
                value={value.apikey?.in_ ?? 'header'}
                onChange={(e) =>
                  onChange({
                    ...value,
                    apikey: {
                      key: value.apikey?.key ?? '',
                      value: value.apikey?.value ?? '',
                      in_: e.target.value as 'header' | 'query',
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
              <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                Key
              </label>
              <Input
                value={value.apikey?.key ?? ''}
                onChange={(e) =>
                  onChange({
                    ...value,
                    apikey: {
                      key: e.target.value,
                      value: value.apikey?.value ?? '',
                      in_: value.apikey?.in_ ?? 'header',
                    },
                  })
                }
                placeholder="X-API-Key"
                className="font-mono text-sm"
              />
            </div>
          </div>
          <div>
            <label className="mb-1.5 block text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              Value
            </label>
            <Input
              value={value.apikey?.value ?? ''}
              onChange={(e) =>
                onChange({
                  ...value,
                  apikey: {
                    key: value.apikey?.key ?? '',
                    value: e.target.value,
                    in_: value.apikey?.in_ ?? 'header',
                  },
                })
              }
              placeholder="支持 {{var}}"
              className="font-mono text-sm"
            />
          </div>
        </div>
      )}
    </div>
  );
}
