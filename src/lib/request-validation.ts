/**
 * 请求格式校验 —— 纯函数、无框架依赖，且**显式调用**。
 *
 * 为什么不做成派生值
 * -----------------
 * 显而易见的写法是对编辑器内容做 `useMemo`：解析 JSON，失败就显示徽标。那样错在
 * 两点：
 *
 *   1. 编辑到一半的 JSON 本来就该是坏的。`{"a": 1,` 是合法文档打了一半的样子，
 *      实时检查几乎每次击键都触发，徽标变成用户学会忽略的背景噪声。
 *   2. 没有任何机制告诉用户*上次*检查是什么时候。400 ms 前算出的结论，下一个键
 *      落下的瞬间就过期了。
 *
 * 所以校验是一个动作而非状态：三条做出承诺的路径——**发送**、**保存**、**格式化**
 * ——去跑它，结果被钉住，直到其中之一再次跑过并成功。这与编译器同构：你在构建
 * 时发现问题，而不是每敲一个字符就发现一次。
 *
 * 范围
 * ----
 * 只管格式。唯一的规则是「json 模式的 body 必须能解析」——**模板里的 `{{name}}`
 * 占位符除外**，那不是「坏了」，只是还没替换（见下面的 `substitutesForValidation`）。
 * 空内容、参数/header 的 key 是否合理、URL 形状都刻意不在这里查——那些该由后端回答，
 * 擅自发明用户没要求的阻塞规则，正是校验器开始和用它的人对着干的方式。
 */
import type { RequestBody } from '@/types/domain';

/**
 * 什么失败了，以及用户得去哪儿才能修好。
 *
 * `tab` 让调用方能把问题*显示*出来，而不是默默什么都不做：用户坐在 Params 页时把
 * 错误钉在 body 编辑器里，正是整个特性要消除的那种不可见。
 */
export interface ValidationIssue {
  /** 占有问题内容的那个 HTTP 页签。 */
  tab: 'body';
  /** 一行，已经按展示措辞写好（不带字段名前缀）。 */
  message: string;
}

/** 校验需要看到的东西 —— *生效后*的请求，含草稿。 */
export interface RequestValidationInput {
  body: RequestBody;
}

/**
 * `{{name}}` 占位符。与替换原语（`src/lib/url.ts` 的 `resolveVarsWithMissing`、
 * Rust 的 `core::template::is_word`）认的是同一套名字：`\w+`。
 */
const PLACEHOLDER = /\{\{\w+\}\}/g;

/**
 * 把占位符换成一段无害的文本，让**模板**能被当成 JSON 看。
 *
 * 动态变量的用法就是往 json body 里写 `{"sns": {{sns}}}`——占位符在**值的位置**且
 * 不带引号。那不是合法 JSON（模板阶段本来就不是），但它是合法的请求模板：发送时
 * 占位符会被替换成取回的值。
 *
 * 两趟替换，因为占位符可能出现在两个位置，而两处的合法形态不同：
 *   - 值的位置（`{"sns": {{sns}}}`）→ 换成裸数字 `0`；
 *   - 键的位置（`{{key}}: 1`）→ 换成带引号的字符串 `"0"`。
 * 两趟都能过不了的，才是真坏了的 JSON。
 */
function substitutesForValidation(content: string): string[] {
  return [content.replace(PLACEHOLDER, '0'), content.replace(PLACEHOLDER, '"0"')];
}

/**
 * 校验生效后的请求。没问题时返回 `null`。
 *
 * 调用方把 `null` 当作「清掉钉住的错误」，所以一次通过与一次失败同样有意义——
 * 它才是让徽标消失的那一次。
 */
export function validateRequest(input: RequestValidationInput): ValidationIssue | null {
  const { body } = input;
  if (body.mode !== 'json') return null;
  const content = body.content.trim();
  // 空的 json body 是「没东西可发」，不是 JSON 坏了。在这里拦它，徽标就成了对
  // 一个用户还没填的字段的抱怨
  if (!content) return null;
  try {
    JSON.parse(content);
    return null;
  } catch (e) {
    const message = (e as Error).message;
    // 带占位符的模板不是「JSON 坏了」——它只是还没替换。两趟都替不出合法 JSON 时，
    // 才是真的坏了，此时报解析器原话。
    for (const candidate of substitutesForValidation(content)) {
      try {
        JSON.parse(candidate);
        return null;
      } catch {
        // 换下一趟
      }
    }
    return { tab: 'body', message };
  }
}
