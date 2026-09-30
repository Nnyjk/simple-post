import assert from 'node:assert/strict';
import { test } from 'node:test';
import { HUE, HUE_ANGLES, HUE_MIN_SEPARATION, HUE_SEPARATION_PAIRS } from '../hue';
import type { HueConcept } from '../hue';

/**
 * Guards the semantic hue system.
 *
 * These assertions exist because the failure mode this refactor set out
 * to kill is silent: two concepts drift onto the same colour and nothing
 * throws, the tests stay green, and the inconsistency only shows up
 * when someone notices the module icon in the tab strip no longer
 * matches the one in the tree. A test is the only thing here that can
 * notice without a human looking.
 *
 * The separation check is the load-bearing one. The other two guard
 * against the mechanical mistake of editing one of a concept's three
 * classes and forgetting the other two.
 */

const CONCEPTS = Object.keys(HUE_ANGLES) as HueConcept[];

/** Shortest distance between two hues on the colour wheel. */
function hueDistance(a: number, b: number): number {
  const raw = Math.abs(a - b) % 360;
  return raw > 180 ? 360 - raw : raw;
}

test('every concept has a class trio and a hue angle', () => {
  assert.equal(CONCEPTS.length, 8);

  for (const concept of CONCEPTS) {
    const entry = HUE[concept];
    assert.ok(entry, `${concept} missing from HUE`);
    // The three classes must name the SAME --hue-* variable. If someone
    // edits `text` and not `bg`, the tile gets a tinted background with
    // a foreground from a different concept and the pairing is silently
    // wrong — this is the drift the map is supposed to prevent.
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
  // These pairs used to share a colour outright. If someone "simplifies"
  // the palette by reusing a hue, this is what catches it.
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
  // Stricter than the pair check: nothing anywhere in the system may
  // reuse another's angle, because a hue can end up adjacent to any
  // other once a panel's contents change.
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
