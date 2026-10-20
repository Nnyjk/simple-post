/**
 * 导入接线：提交成功后必须刷新项目清单。
 *
 * 服务定义住在项目行上，而项目行只在 bootstrap 第一层加载过一次——导入在服务端建服务
 * 后不刷新它，界面上就看不见那个服务。
 *
 * 断言源码形状而非行为：本仓库没有 React hook / 组件测试的设施，而这条接线一旦被拆掉，
 * 症状是「导入成功但服务不出现」，很容易被归到别的问题上。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

describe('导入接线', () => {
  it('提交成功后刷新项目清单', () => {
    const src = readFileSync(new URL('../useTransfer.ts', import.meta.url), 'utf8');
    const commit = src.slice(src.indexOf('const commit ='), src.indexOf('const save ='));
    assert.ok(commit.length > 0, '没能定位到 commit 回调，源文件形状变了？');
    // 位置也要钉住：刷新得在导入提交**之后**、且在 try 路径之内。挪到提交之前，
    // 或者挪进 catch，测试都还会绿，但服务照样看不见。
    const importCommit = commit.indexOf('const r = await invokeCmd<ImportCommitResult>');
    const refresh = commit.indexOf('await useAppStore.getState().refreshProjects();');
    const catchBlock = commit.indexOf('} catch (e) {');
    assert.ok(importCommit >= 0, '没能定位到导入提交调用');
    assert.ok(refresh >= 0, '导入提交后必须刷新 projects，否则新服务在界面上不存在');
    assert.ok(catchBlock >= 0, '没能定位到 catch 块');
    assert.ok(
      importCommit < refresh && refresh < catchBlock,
      '刷新必须落在 try 路径里、且在导入提交之后',
    );
  });
});
