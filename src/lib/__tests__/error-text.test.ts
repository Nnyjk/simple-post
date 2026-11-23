import { test } from 'node:test';
import assert from 'node:assert/strict';
import { errorText } from '@/lib/error-text';

test('errorText: 一条 Error 取它的 message', () => {
  assert.equal(errorText(new Error('炸了')), '炸了');
});

test('errorText: 不是 Error 的取值走 String()', () => {
  assert.equal(errorText('字符串'), '字符串');
  assert.equal(errorText(42), '42');
  assert.equal(errorText(null), 'null');
});

test('errorText: 转不成字符串的取值给出回退文案，不抛异常', () => {
  // `Object.create(null)` 没有 `toString`，`String(e)` 会抛 `TypeError`。
  // 批量发送时这个异常发生在逐条的 catch 里，一次就会让整批停下。
  const bare = Object.create(null);
  assert.equal(errorText(bare), '未知错误');
});

test('errorText: 后端的 { code, message } 对象取它的 message', () => {
  // 桌面端 invoke 失败时 Tauri 2 把 `AppError` 当作普通对象 reject：它不是 `Error`，
  // 若直接转字符串会显示 `[object Object]`，把唯一有信息量的那一句丢掉。
  const appError = { code: 'validation', message: 'validation: 变量名非法' };
  assert.equal(errorText(appError), 'validation: 变量名非法');
});

test('errorText: 没有 message 字段的对象仍走 String()', () => {
  assert.equal(errorText({ code: 'x' }), '[object Object]');
  assert.equal(errorText({ message: '' }), '[object Object]');
});
