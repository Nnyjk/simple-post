/**
 * GitHub 国内镜像 —— 把发布页 / 发布说明里的 GitHub 链接改写成走某个镜像的等价地址。
 *
 * 只改写「用户点开去读」的那类链接（网页、raw 文件、gist）：检查更新走的
 * `api.github.com` 不在名单里，故那次请求始终直连，镜像切换不会让更新检查失效。
 *
 * 命名与规则只有这一份：持久化偏好里存的是镜像 id 字符串，非法值一律回落 `direct`，
 * 于是手改 localStorage 也只会退回直连，而不是丢掉链接。
 */

export type GithubMirrorId = 'direct' | 'ghfast' | 'ghproxy' | 'kkgithub' | 'moeyy';

/**
 * 本项目的 GitHub 主页。README（即使用文档）与 issue 都在那里，欢迎页的「查看文档」
 * 与设置页版本号旁的图标都指向它。
 */
export const PROJECT_GITHUB_URL = 'https://github.com/Nnyjk/simple-post';

export const GITHUB_MIRRORS: { id: GithubMirrorId; label: string; hint: string }[] = [
  { id: 'direct', label: '直连 GitHub', hint: '官方地址，国内网络可能较慢或不可达' },
  { id: 'ghfast', label: 'ghfast.top', hint: '前缀镜像 · 只代理发布包与 raw 文件下载' },
  { id: 'ghproxy', label: 'gh-proxy.com', hint: '前缀镜像 · 网页与 raw 文件均可用' },
  { id: 'kkgithub', label: 'kkgithub.com', hint: '换域名的镜像 · 不覆盖 gist' },
  { id: 'moeyy', label: 'github.moeyy.xyz', hint: '前缀镜像 · 国内可用' },
];

export const GITHUB_MIRROR_IDS: readonly GithubMirrorId[] = GITHUB_MIRRORS.map((m) => m.id);

/**
 * 能直接把 GitHub 网页呈现给浏览器的镜像。
 *
 * 前缀镜像里只有 gh-proxy.com 实测能代理 HTML 网页（仓库页 200）；ghfast.top 是下载加速，
 * 对 `https://ghfast.top/https://github.com/<owner>/<repo>` 回 403 `Invalid input.`，故它
 * 与未验证过的 github.moeyy.xyz 都只用于文件，不用于网页。
 */
const PAGE_MIRRORS: Partial<Record<GithubMirrorId, true>> = {
  ghproxy: true,
  kkgithub: true,
};

/**
 * 仓库主页（README 与 issue 所在页）的地址。
 *
 * 只让网页镜像参与改写：`mirrorGithubUrl` 会给前缀镜像加前缀，而网页经前缀镜像未必成立
 * （见 [`PAGE_MIRRORS`]），用户点开就落到错误页。其余镜像一律直连。
 */
export function projectHomeUrl(mirror: GithubMirrorId): string {
  return PAGE_MIRRORS[mirror] === true
    ? mirrorGithubUrl(PROJECT_GITHUB_URL, mirror)
    : PROJECT_GITHUB_URL;
}

/** 镜像能覆盖的 GitHub 主机。`api.github.com` 刻意不在其中（更新检查要直连）。 */
const MIRRORABLE_HOSTS: Record<string, true> = {
  'github.com': true,
  'www.github.com': true,
  'raw.githubusercontent.com': true,
  'gist.github.com': true,
};

/** 前缀型镜像：镜像前缀 + 原 URL。 */
const PREFIX_MIRRORS: Partial<Record<GithubMirrorId, string>> = {
  ghfast: 'https://ghfast.top/',
  ghproxy: 'https://gh-proxy.com/',
  moeyy: 'https://github.moeyy.xyz/',
};

/** 换域名型镜像：主机名替换表。gist.github.com 不在此表，故 gist 链接原样返回。 */
const KK_GITHUB_HOSTS: Record<string, string> = {
  'github.com': 'kkgithub.com',
  'www.github.com': 'kkgithub.com',
  'raw.githubusercontent.com': 'raw.kkgithub.com',
};

/** 把偏好里读到的原始字符串归一成可用 id；非法值回落 `direct`。 */
export function resolveGithubMirror(raw: string | null | undefined): GithubMirrorId {
  return GITHUB_MIRROR_IDS.find((id) => id === raw) ?? 'direct';
}

/**
 * 按镜像改写一个 URL。非 http(s)、非 GitHub 主机、解析失败一律原样返回 —— 镜像只负责
 * 「读 GitHub 上的东西」，不替调用方判断链接该不该打开。
 */
export function mirrorGithubUrl(url: string, mirror: GithubMirrorId): string {
  if (mirror === 'direct') return url;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return url;
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return url;
  const host = parsed.hostname.toLowerCase();
  if (MIRRORABLE_HOSTS[host] !== true) return url;

  const prefix = PREFIX_MIRRORS[mirror];
  if (prefix) return prefix + url;

  const replacement = KK_GITHUB_HOSTS[host];
  if (!replacement) return url;
  parsed.hostname = replacement;
  return parsed.toString();
}
