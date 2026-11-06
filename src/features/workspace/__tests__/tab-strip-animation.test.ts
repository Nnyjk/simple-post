/**
 * 标签条动画的结构约定（源码哨兵）。
 *
 * 这一组缺陷的共同点是：**三套动画机制各自为政** —— 位置与宽度由 GSAP Flip 补间、
 * 宽度另有一层 CSS 过渡、横向滚动由原生 `scrollIntoView` 直接改 `scrollLeft`。它们
 * 对「标签最终在哪」给出三个不同的答案，标签多到条放不下时（用户报的症状：长度稍长
 * 以后）就表现为整条标签猛地变宽、猛跳一下，再慢慢缩回并滑回。
 *
 * 这些约定靠代码评审看不出来、坏了也不一定立刻出事，故在这里钉住：
 *
 * 1. Flip 的依赖里必须有 `isCompact`。宽度在 normal（160px）与 compact（92px）之间
 *    变，宽度也是 Flip 补间的一部分；少了它，上一次记下的位置是另一套宽度，标签会
 *    凭空涨回旧宽度。
 * 2. 标签上不能有宽度的 CSS 过渡。Flip 逐帧写下的 inline width 与 CSS 过渡各改一次
 *    宽度，动画结束后还要再慢慢收回去。
 * 3. 不能用原生 `scrollIntoView`。它瞬间改掉 `scrollLeft`，所有标签的视口位置在同一
 *    帧里挪一截，那一下不参与任何动画。
 * 4. 横向滚动由 GSAP 补间完成，目标位置由 `scrollTargetForActiveTab` 算出。
 * 5. 上一次补间要在下一轮开始前落定。Flip 的补间期间把标签移出文档流，不落定的话
 *    下一轮量到的是动画中途的位置。
 *
 * 这些检查只读源码；实际动画效果由界面验证脚本覆盖。
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const FILE = resolve(process.cwd(), 'src/features/workspace/WorkspaceTabs.tsx');

/**
 * 去掉注释再匹配：注释里出现的 `scrollIntoView`（解释为什么不用它）不该被当成调用。
 * 行尾注释一并去掉 —— 这个文件里没有含 `//` 的字符串，故直接按行尾截断是安全的。
 */
function stripComments(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .map((line) => {
      const trimmed = line.trimStart();
      if (trimmed.startsWith('//') || trimmed.startsWith('*')) return '';
      const at = line.indexOf('//');
      return at >= 0 ? line.slice(0, at) : line;
    })
    .join('\n');
}

const code = stripComments(readFileSync(FILE, 'utf8'));

describe('标签条动画的结构约定', () => {
  it('Flip 的依赖里有 compact 模式', () => {
    assert.match(code, /\[\s*openTabs\s*,\s*isCompact\s*\]/);
  });

  it('标签上没有宽度的 CSS 过渡', () => {
    assert.doesNotMatch(code, /transition-\[width/);
  });

  it('不用原生 scrollIntoView 滚动', () => {
    assert.doesNotMatch(code, /scrollIntoView/);
  });

  it('横向滚动由 GSAP 补间完成，目标位置由纯函数算出', () => {
    assert.match(code, /scrollTargetForActiveTab\(/);
    assert.match(code, /gsap\.to\(strip/);
    assert.match(code, /scrollLeft:\s*target/);
    // 直接赋值 scrollLeft 就是瞬间挪一截，不参与任何动画。滚轮那条是另一回事：
    // 它改的是 `el.scrollLeft`（滚轮自己的增量），不在这一行的禁止范围内。
    assert.doesNotMatch(code, /\bstrip\.scrollLeft\s*=[^=]/);
  });

  it('上一次补间在下一轮开始前落定，且落定时要还原标签样式', () => {
    // 落定必须在清理函数里：写在效果体里，落定的就是**这一轮**的补间，而不是上一轮。
    assert.match(code, /return \(\) => \{[^}]*tween\.progress\(1\)/);
    // suppressEvents 会把 GSAP 还原标签样式的回调一起抑制掉，标签就此停在绝对定位上，
    // 条的内容宽度从此算不对。
    assert.doesNotMatch(code, /progress\(1\s*,\s*true\)/);
  });

  it('新出现的标签被并进 Flip 的那份记录', () => {
    // 不并进去的话，Flip 会把它当成进入元素：它留在文档流里，其余标签被 Flip 移出
    // 文档流之后，它会退回条的最左边，与第一个标签重叠。
    assert.match(code, /previous\.add\(\s*Flip\.getState\(\s*entering\s*\)\s*\)/);
  });

  it('滚动结束后重新记录布局', () => {
    // Flip 记的是视口坐标：条滚动过之后那份记录就过期了，下一次增删标签会先横跳一截。
    assert.match(code, /onComplete:\s*\(\)\s*=>\s*captureFlipState\(\)/);
  });

  it('滚轮滚动之后重新记录布局', () => {
    assert.match(code, /scheduleFlipStateRefresh\(\)/);
  });

  it('目标已在可视范围时停掉上一次滚动', () => {
    // 不停掉的话，上一次滚动会继续往旧标签那边滑，把刚点开的标签带出视野。
    assert.match(code, /if \(target === null\) \{[^}]*gsap\.killTweensOf\(strip\)/);
  });

  it('滚轮滚动之前停掉程序滚动', () => {
    // 程序滚动那个补间每帧都在写 scrollLeft，会把滚轮的结果抹掉。
    assert.match(code, /onWheel[\s\S]{0,600}?gsap\.killTweensOf\(el\)/);
  });

  it('Flip 补间还在跑时不立刻滚动', () => {
    // 此刻标签的位置是动画中途的位置，算出来的滚动目标是错的。
    // 这道守卫在「立刻滚动」那个效果里，不是文件里任意一处。
    assert.match(
      code,
      /if \(tabsChanged\) return;[\s\S]{0,300}?if \(flipRunningRef\.current\) return;/,
    );
  });

  it('位置补间不再走 useGSAP 的清理函数', () => {
    // `useGSAP` 的清理函数在依赖变化时执行，而 Flip 的补间要在**新 DOM 提交后**才开始，
    // 两者的先后关系由 useLayoutEffect 自己管更直接。
    assert.doesNotMatch(code, /useGSAP/);
  });
});
