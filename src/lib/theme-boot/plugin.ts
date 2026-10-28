import vm from 'node:vm';

import { transformWithOxc, type Plugin } from 'vite';

/**
 * `index.html` 里放这段脚本的位置标记。
 *
 * 插件把标记替换成编译好的内联脚本。标记不在 HTML 里就是构建配置与源码脱节了，
 * 插件会在构建期报错，而不是让页面悄悄用回默认主题。
 */
export const THEME_BOOT_MARKER = '<!--theme-boot-->';

/** 内联脚本把共享模块挂到哪个全局名上。 */
const GLOBAL_NAME = '__simplePostTheme';

/** 一行里以 `import` 开头的语句。内联脚本不能有它们。 */
const IMPORT_LINE = /^[ \t]*import\b.*$/gm;

/** 一行里以 `export` 开头的语句。运行时只允许一个（默认导出）。 */
const EXPORT_LINE = /^[ \t]*export\b.*$/gm;

/** 动态 import。经典脚本里同样没有它。 */
const DYNAMIC_IMPORT = /\bimport\s*\(/;

/**
 * 独占一行的块注释。
 *
 * 注释是给人看的，注入进 `index.html` 的那段脚本是给浏览器看的——首屏预算里它每一个字节
 * 都要算。故把块注释剥掉，只留代码。
 *
 * 单行与多行分开剥：多行那条要求注释从行首开始、到行首的结束标记为止，单行那条自己就能
 * 收尾。分开剥才不会出现「单行注释的结束标记不在行首，于是匹配跨到下一个注释，把中间的
 * 代码一起吞掉」这种事。
 */
const SINGLE_LINE_BLOCK_COMMENT = /^[ \t]*\/\*\*[^\n]*\*\/[ \t]*\n?/gm;
const MULTI_LINE_BLOCK_COMMENT = /^[ \t]*\/\*\*[\s\S]*?^[ \t]*\*\/\n?/gm;

/**
 * 代码里声明了哪些名字（函数与常量）。
 *
 * 剥注释前后的两份代码必须声明同样的名字：剥注释一旦吞掉了代码，名字就少了，而那份代码
 * 往往仍是合法 JavaScript，光看语法发现不了。
 */
function declarationNames(code: string): string {
  const names: string[] = [];
  for (const m of code.matchAll(/\bfunction\s+([A-Za-z_$][\w$]*)/g)) names.push(m[1]);
  for (const m of code.matchAll(/\bconst\s+([A-Za-z_$][\w$]*)\s*=/g)) names.push(m[1]);
  return names.sort().join(',');
}

/**
 * 剥掉块注释。
 *
 * 剥完的代码若与原来声明的名字对不上，或不再是合法 JavaScript，就原样返回带注释的版本——
 * 省下几个字节不值得换一个坏掉的首帧。
 */
function stripBlockComments(code: string): string {
  const stripped = code
    .replace(SINGLE_LINE_BLOCK_COMMENT, '')
    .replace(MULTI_LINE_BLOCK_COMMENT, '');
  if (declarationNames(stripped) !== declarationNames(code)) return code;
  try {
    // 只编译，不执行：这里要确认的是语法没被剥坏。
    new vm.Script(stripped);
    return stripped;
  } catch {
    return code;
  }
}

/**
 * 把共享模块的源码编译成一段可注入 `index.html` 的内联脚本。
 *
 * 做法是三步：
 *
 * 1. `transformWithOxc` 把 TypeScript 转成 JavaScript（类型消失，语法保留）。
 * 2. 把唯一那条默认导出改写成给全局对象赋值 —— 内联脚本是 classic script，拿不到模块系统
 *    的导出，只能靠全局名把 API 递出去。
 * 3. 包进 IIFE 并在末尾调用 `applyInitialTheme`，让脚本一被解析就把主题写到 `<html>` 上。
 *
 * 任何一步的前提不成立就抛错：编译期失败比运行时少一段功能好得多。
 */
export async function buildThemeBootScript(source: string): Promise<string> {
  const { code } = await transformWithOxc(source, 'theme-boot.ts', { lang: 'ts' });

  const importLines = code.match(IMPORT_LINE);
  if (importLines || DYNAMIC_IMPORT.test(code)) {
    throw new Error(
      'theme-boot：共享模块里有 import 语句。内联脚本是 classic script，浏览器不允许它 import。' +
        '请把共享模块改成运行时零依赖。',
    );
  }

  const exportLines = code.match(EXPORT_LINE) ?? [];
  if (exportLines.length !== 1 || !/^[ \t]*export\s+default\b/.test(exportLines[0])) {
    throw new Error(
      `theme-boot：共享模块的运行时导出必须恰好一个默认导出，实际有 ${exportLines.length} 条导出语句。` +
        '多一个导出，注入的脚本就少一段 API，而页面不会报错。',
    );
  }

  const body = code.replace(/^[ \t]*export\s+default\s+/m, `globalThis.${GLOBAL_NAME} = `);

  // `</script` 出现在脚本文本里会提前结束 HTML 里的 script 标签，故转义掉。
  const safe = stripBlockComments(body).replace(/<\/script/gi, '<\\/script');

  return [
    '/* 由 vite.config.ts 的 themeBootPlugin 从 src/lib/theme-boot/index.ts 生成。',
    '   请改那个源文件，不要改这里。 */',
    '(function () {',
    safe,
    `;globalThis.${GLOBAL_NAME}.applyInitialTheme();`,
    '})();',
  ].join('\n');
}

/**
 * 把编译好的内联脚本注入 `index.html`。
 *
 * 插件自己不读文件、不碰路径：`vite.config.ts` 把源文件内容读出来传进来，于是本模块既能在
 * 构建期用，也能在测试里直接 import 断言它的行为。
 */
export function themeBootPlugin(source: string): Plugin {
  return {
    name: 'simple-post:theme-boot',
    transformIndexHtml: {
      order: 'pre',
      async handler(html: string) {
        if (!html.includes(THEME_BOOT_MARKER)) {
          throw new Error(
            `theme-boot：index.html 里没有 ${THEME_BOOT_MARKER} 标记，注入不了首帧主题脚本。`,
          );
        }
        const script = await buildThemeBootScript(source);
        // 替换串用函数形式：字符串形式会把替换文本里的 `$&` 之类当成替换记号展开，
        // 脚本文本里出现它们时，注入的脚本就被改坏了。
        return html.replace(THEME_BOOT_MARKER, () => `<script>${script}</script>`);
      },
    },
  };
}
