/**
 * HTTPS 出站设置在界面上的共享措辞与纯计算。
 *
 * 设置面板（全局默认）与端点编辑器（端点级覆盖）渲染的是**同一组**设置，故标签、
 * 说明与警示只在这里写一次：两处各写一份时，某天改了一处就会让用户对着两个说法
 * 发愣，而这两处说的本来就是同一件事。
 *
 * **缺省值不在这里。** 全局默认那份的缺省值住在
 * `stores/slices/ui-prefs.ts::REQUEST_SETTINGS_DEFAULTS`——那是后端
 * `RequestSettings::default()` 的镜像，别处不再声明第二份。
 */
import type { TlsSettings } from '@/types/domain';

/** 三个证书路径输入框里的那一个。 */
export type TlsPathKey = 'caCertificatePath' | 'clientCertificatePath' | 'clientKeyPath';

export interface TlsPathField {
  key: TlsPathKey;
  /** 输入框上方的标签。 */
  label: string;
  /** 输入框下方的一句说明：这一项在出站请求里起什么作用。 */
  description: string;
  /** 空输入框里的提示文字。 */
  placeholder: string;
}

/**
 * 三个证书路径输入框。顺序即界面上的上下顺序。
 *
 * 三个字段在一次出站请求里各管一件事：CA 证书决定「额外信谁签发的服务端证书」，
 * 客户端证书与私钥决定「向服务端出示什么身份」。
 *
 * **CA 那一条说的是「额外信任」，不是「只信任」。** 实现把自定义 CA **加进**内置的
 * 公共根证书集（`add_root_certificate`），内置的那一套照样有效 —— 界面若写成
 * 「只信任这张 CA」，用户会以为公共根证书已被排除，于是某个走公共 CA 的接口
 * 握手失败时找不到原因。
 */
export const TLS_PATH_FIELDS: readonly TlsPathField[] = [
  {
    key: 'caCertificatePath',
    label: '自定义 CA 证书',
    description:
      '在内置的公共根证书集之外，额外信任这张 CA 签发的服务端证书。留空则只用内置的那一套',
    placeholder: 'C:\\certs\\company-ca.pem',
  },
  {
    key: 'clientCertificatePath',
    label: '客户端证书',
    description: '以这个身份向服务端出示证书（mTLS）。留空则不挂客户端证书',
    placeholder: 'C:\\certs\\client.pem',
  },
  {
    key: 'clientKeyPath',
    label: '客户端私钥',
    description:
      '与客户端证书配对的私钥，必须同时填客户端证书路径。留空则从客户端证书指向的那个文件里一起读',
    placeholder: 'C:\\certs\\client.key',
  },
];

/** 「跳过证书校验」这一项的措辞。 */
export const TLS_SKIP_VERIFY = {
  label: '跳过证书校验',
  description: '开启后不再检查服务端证书的过期时间、签发者与主机名',
  /** 开关旁边的警示：说清打开之后会发生什么。 */
  warning: '跳过校验之后，任何中间人都能截获并改写这次请求，证书过期与伪造都不会被发现。',
} as const;

/** 三个路径输入框共用的说明：证书只存路径，不存内容。 */
export const TLS_PATH_NOTE =
  '证书只存文件路径，不存内容。路径由运行请求的那台机器解析：桌面端是本机路径，serve-web / Docker 端是服务端容器里的路径。';

/**
 * 两份 TLS 设置是否逐字段相等。
 *
 * 设置面板的「恢复默认」用它判断 TLS 那一组是否已偏离缺省值：比较对象是四个字段
 * 而不是对象引用，故重新读回后端那一行也不会误报成「已改动」。
 */
export function tlsEquals(a: TlsSettings, b: TlsSettings): boolean {
  return (
    a.skipVerify === b.skipVerify &&
    a.caCertificatePath === b.caCertificatePath &&
    a.clientCertificatePath === b.clientCertificatePath &&
    a.clientKeyPath === b.clientKeyPath
  );
}

/**
 * 输入框里的文本转成落库的路径值。
 *
 * 空串算作「没填」，落库时写成 `null` 而不是 `''`——后端据此判断要不要挂证书，
 * 两种写法它都认，但存成 `null` 才与缺省值的形状一致。
 */
export function tlsPathValue(raw: string): string | null {
  return raw.length === 0 ? null : raw;
}

/**
 * 改一份 TLS 设置里的某个证书路径。
 *
 * 逐个字段写出来而不是用计算属性名：计算属性名得到的是 `{[x: string]: …}`，编译器
 * 无法确认它落在 `TlsSettings` 的哪三个字段上，于是要写一句强制转换。
 */
export function setTlsPath(
  settings: TlsSettings,
  key: TlsPathKey,
  value: string | null,
): TlsSettings {
  switch (key) {
    case 'caCertificatePath':
      return { ...settings, caCertificatePath: value };
    case 'clientCertificatePath':
      return { ...settings, clientCertificatePath: value };
    case 'clientKeyPath':
      return { ...settings, clientKeyPath: value };
  }
}
