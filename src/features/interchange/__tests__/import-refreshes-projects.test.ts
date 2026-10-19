/**
 * 导入接线：提交成功后必须刷新项目清单。
 *
 * 服务定义住在**项目行**上（`projects[].baseUrlDefinitions`），而项目行是
 * bootstrap 第一层加载后就没有人再更新的那份副本。导入在服务端建服务
 * （一次事务完成，回来的只有回报），客户端这边不刷新就看不见新服务——
 * 前置 URL 表里没有它，接口工具栏显示成「项目无默认服务」，直到整页刷新。
 *
 * 为什么是源码形状而不是行为断言：本仓库没有 React hook / 组件测试的设施
 * （`useTransfer` 是个 hook），而这条接线一旦被拆掉，症状是「导入成功但
 * 服务不出现」——很容易被归到别的问题上。接线本身只有一行，用源码形状
 * 守住它是最省事的办法。路径相对**本文件**（`new URL('../…')`）：测试文件
 * 一旦挪目录，这个 URL 会先失效——`tauri-commands-shell.test.ts` 记过这笔账。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

describe('导入接线', () => {
  it('提交成功后刷新项目清单', () => {
    const src = readFileSync(new URL('../useTransfer.ts', import.meta.url), 'utf8');
    const commit = src.slice(src.indexOf('const commit ='), src.indexOf('const save ='));
    assert.ok(commit.length > 0, '没能定位到 commit 回调，源文件形状变了？');
    assert.match(
      commit,
      /refreshProjects\(\)/,
      '导入提交后必须刷新 projects，否则新服务在界面上不存在',
    );
  });
});
