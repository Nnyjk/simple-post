import assert from 'node:assert/strict';
import { test } from 'node:test';
import { HUE, HUE_ANGLES, HUE_MIN_SEPARATION, HUE_SEPARATION_PAIRS } from '../hue';
import type { HueConcept } from '../hue';

/**
 * 语义色相系统的回归网。
 *
 * 这里要拦的失败模式是静默的：两个概念漂到同一个颜色上，不抛异常、测试照绿，直到
 * 有人发现页签条里的模块图标和树里那个不一致。只有测试不靠人看也能发现。
 *
 * 间隔检查是承重的那条。另外两条防的是机械失误：改了某个概念的三个类里的一个，
 * 忘了另外两个。
 */

const CONCEPTS = Object.keys(HUE_ANGLES) as HueConcept[];

/** 色轮上两个色相的最短距离。 */
function hueDistance(a: number, b: number): number {
  const raw = Math.abs(a - b) % 360;
  return raw > 180 ? 360 - raw : raw;
}

test('every concept has a class trio and a hue angle', () => {
  assert.equal(CONCEPTS.length, 8);

  for (const concept of CONCEPTS) {
    const entry = HUE[concept];
    assert.ok(entry, `${concept} missing from HUE`);
    // 三个类必须指向**同一个** `--hue-*` 变量。有人改了 `text` 却没改 `bg`，tile
    // 就会是另一个概念的淡色底配这个概念的字色，配对静默地错掉——正是这张表要防的漂移
    for (const [slot, cls] of Object.entries(entry)) {
      assert.match(
        cls,
        new RegExp(`^\\w+-hue-${concept}(/\\d+)?$`),
        `${concept}.${slot} should target --hue-${concept}, got "${cls}"`,
      );
    }
  }
});

test('HUE and HUE_ANGLES cover exactly the same concepts', () => {
  const inHue = Object.keys(HUE).sort();
  const inAngles = Object.keys(HUE_ANGLES).sort();
  assert.deepEqual(inHue, inAngles, 'a concept is missing from one of the two maps');
});

test('the four previously-colliding pairs stay visually distinct', () => {
  // 这几对必须彼此可辨。若有人「简化」调色板、复用同一个色相，拦下它的就是这里。
  assert.equal(HUE_SEPARATION_PAIRS.length, 4);

  for (const [a, b] of HUE_SEPARATION_PAIRS) {
    const distance = hueDistance(HUE_ANGLES[a], HUE_ANGLES[b]);
    assert.ok(
      distance >= HUE_MIN_SEPARATION,
      `${a} (${HUE_ANGLES[a]}°) and ${b} (${HUE_ANGLES[b]}°) are only ${distance}° apart; ` +
        `they need >= ${HUE_MIN_SEPARATION}° to be tellable apart at 12-14px`,
    );
  }
});

test('no two concepts share a hue angle at all', () => {
  // 比配对检查更严：系统里任何地方都不得复用别人的角度，因为面板内容一变，
  // 任意两个色相都可能挨到一起
  for (let i = 0; i < CONCEPTS.length; i++) {
    for (let j = i + 1; j < CONCEPTS.length; j++) {
      const a = CONCEPTS[i];
      const b = CONCEPTS[j];
      assert.notEqual(HUE_ANGLES[a], HUE_ANGLES[b], `${a} and ${b} share hue ${HUE_ANGLES[a]}°`);
    }
  }
});

test('hueDistance is a real circular distance', () => {
  assert.equal(hueDistance(10, 350), 20);
  assert.equal(hueDistance(350, 10), 20);
  assert.equal(hueDistance(0, 180), 180);
  assert.equal(hueDistance(90, 90), 0);
});
