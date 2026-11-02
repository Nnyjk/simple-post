/**
 * collection 脚本的 patch 形状。
 *
 * 单独成文件是为了让它可测：对话框本身是 React 组件，而这个仓库没有组件测试的
 * 工具链（见 AGENTS.md）。规则本身必须成立，故不该只活在 JSX 里。
 */

/** 两段脚本的 patch。值是 `null` 表示「清除这一段」。 */
export interface ScriptsPatch {
  preRequestScript: string | null;
  testScript: string | null;
}

/**
 * 编辑器里的两段文本 → patch。
 *
 * **空脚本按「清除」发送**，而不是存一个空串：空串会让那一行一直留在库里，而
 * 「没有脚本」与「有一个空的脚本」在链装配时是同一件事。故空的一律发 `null`，
 * 由后端把那一行删掉。
 *
 * 纯空白（只有空格与换行）也算空——那通常是用户敲完又删了。
 */
export function buildScriptsPatch(preRequest: string, test: string): ScriptsPatch {
  return {
    preRequestScript: preRequest.trim() ? preRequest : null,
    testScript: test.trim() ? test : null,
  };
}
