//! 导入与导出两侧都必须认同的词汇表。
//!
//! 这几项两侧若各写一份，它们可以各自正确却互相不一致，而**没有哪一个测试会红**：
//! 每侧只对着自己的期望值自洽。共用一份是这个模块存在的全部理由。

/// OpenAPI 3.x `paths` 的固定动词键集。
///
/// - 导入侧：`x-` 扩展键不在这个集合里，被自然跳过。
/// - 导出侧：集合外的动词塞进去会让整份文档不合法，故必须挡住它，而不是「尽量映射」。
pub(crate) const PATH_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// 一个 multipart 分片。
///
/// **直接用 core 的那一份**，不在这里再抄一份：`BodyMode::Multipart` 的
/// `RequestBody::content` 装的是这个数组的 JSON 文本，导入侧写出的形状必须与发送侧
/// 读到的形状逐字段相同。两份定义漂移的后果是「导入看着成功，发送时零分片」，而
/// 没有任何一侧的测试会红。
///
/// 依赖方向是 `interchange → core`，故这里引用得到。
pub(crate) use simple_post_core::domain::MultipartPart;

/// JSON Pointer 转义：`/` 与 `~` 不能原样出现在指针里。
///
/// 替换顺序是契约的一部分——`~` 必须先换成 `~0`，否则后一步产生的 `~1` 会被再转一次。
///
/// 一份转义器担着两件事：诊断的 `pointer`，以及导入时 [`crate::ir::IrNode`] 的 `key`
/// （那是节点在源格式里的位置指针，与诊断指针同一套寻址纪律）。
pub(crate) fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
#[path = "spec_test.rs"]
mod tests;
