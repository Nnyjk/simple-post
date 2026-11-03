/**
 * 语义色相强调 —— 概念→颜色的唯一映射。
 *
 * 所有「模块是什么颜色」的判断都走 `HUE`；具体色值由 `globals.css` 以 CSS 变量持有，
 * 随 `.dark` 类切换明暗。
 *
 * - **A 档** —— project / module / collection / endpoint。彩度更高：这是树的结构词汇，
 *   图标形状本身已不同（package / folder / open-folder / code），颜色只加强而不再承载含义。
 * - **B 档** —— baseurl / variable / connection / request。刻意更安静，好让设置与配置区
 *   永远不与树抢注意力；低彩度也让 `connection` 的玫红不会被误认成同面板里紧挨着的
 *   饱和 `--destructive` 红。
 * - 另外两个概念刻意不取色相：environment → `--primary`（应用的主要上下文，复用 primary
 *   在语义上诚实，也不会与其它概念用同一个颜色）；appearance → `--muted-foreground`
 *   （纯外观偏好面板，给它色相只会无信息收益地喊叫）。
 * - **色相角度的约束**：这些强调色以 12-14px 渲染在 15% alpha 的底色上，明度差几乎看不见，
 *   色相距离是观者唯一还能分辨的线索；下面的角度因此按「任何两个*同时出现*的概念不相隔
 *   30° 以内」排布——见 HUE_ANGLES 与 `src/lib/__tests__/hue.test.ts` 里的间隔断言。
 *   调饱和度或明度是安全的；旋转某个色相不是，那会让两个概念的色相距离重新小于 30°。
 */

export type HueConcept =
  | 'project'
  | 'module'
  | 'collection'
  | 'endpoint'
  | 'baseurl'
  | 'variable'
  | 'connection'
  | 'request';

/**
 * 每个概念的色相角度（度）。「间隔」性质的唯一真相来源；具体颜色三元组在
 * `src/styles/globals.css` 的 `--hue-*` 下。
 *
 * 必须分开的四对及其间隔：
 *   baseurl ↔ collection   40°
 *   variable ↔ module      50°
 *   connection ↔ project   90°
 *   request ↔ endpoint    160°
 */
export const HUE_ANGLES: Record<HueConcept, number> = {
  project: 265,
  module: 45,
  collection: 185,
  endpoint: 150,
  baseurl: 225,
  variable: 95,
  connection: 355,
  request: 310,
};

/** 会出现在同一列表/面板中、必须彼此分开的概念对。 */
export const HUE_SEPARATION_PAIRS: readonly (readonly [HueConcept, HueConcept])[] = [
  ['baseurl', 'collection'],
  ['variable', 'module'],
  ['connection', 'project'],
  ['request', 'endpoint'],
];

/** 两个同时出现的概念必须保持的最小色相角度（度）。 */
export const HUE_MIN_SEPARATION = 30;

interface HueClasses {
  /** 15% alpha 的底色，作 tile 背景。 */
  bg: string;
  /** 满彩度前景 —— 图标字形与文字。 */
  text: string;
  /** 30% alpha 的边缘，给要带边框的 tile。 */
  border: string;
}

/**
 * 概念 → 工具类。每个概念的三个条目刻意相邻：只改其中一个会让三个条目指向不同的
 * `--hue-*` 变量，测试断言它们始终指向同一个。
 */
export const HUE: Record<HueConcept, HueClasses> = {
  project: {
    bg: 'bg-hue-project/15',
    text: 'text-hue-project',
    border: 'border-hue-project/30',
  },
  module: {
    bg: 'bg-hue-module/15',
    text: 'text-hue-module',
    border: 'border-hue-module/30',
  },
  collection: {
    bg: 'bg-hue-collection/15',
    text: 'text-hue-collection',
    border: 'border-hue-collection/30',
  },
  endpoint: {
    bg: 'bg-hue-endpoint/15',
    text: 'text-hue-endpoint',
    border: 'border-hue-endpoint/30',
  },
  baseurl: {
    bg: 'bg-hue-baseurl/15',
    text: 'text-hue-baseurl',
    border: 'border-hue-baseurl/30',
  },
  variable: {
    bg: 'bg-hue-variable/15',
    text: 'text-hue-variable',
    border: 'border-hue-variable/30',
  },
  connection: {
    bg: 'bg-hue-connection/15',
    text: 'text-hue-connection',
    border: 'border-hue-connection/30',
  },
  request: {
    bg: 'bg-hue-request/15',
    text: 'text-hue-request',
    border: 'border-hue-request/30',
  },
};
