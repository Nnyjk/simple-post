//! 百分号编码的字符集与 JS 的 `encodeURIComponent` 对齐。

use super::percent_encode;

/// `encodeURIComponent` 不转义的那批字符必须原样保留。
#[test]
fn the_kept_character_set_is_the_one_encodeuricomponent_keeps() {
    // `encodeURIComponent` 不转义 `A-Z a-z 0-9 - _ . ! ~ * ' ( )`。
    for c in "abcXYZ019-_.!~*'()".chars() {
        let s = c.to_string();
        assert_eq!(percent_encode(&s), s, "{c:?} 必须原样保留");
    }
}

/// 其余字符一律 `%XX`，十六进制大写。
#[test]
fn everything_else_is_percent_encoded_with_uppercase_hex() {
    assert_eq!(percent_encode("a b"), "a%20b");
    assert_eq!(percent_encode("a+b"), "a%2Bb");
    assert_eq!(percent_encode("a&b=c"), "a%26b%3Dc");
    assert_eq!(percent_encode("a/b"), "a%2Fb");
    assert_eq!(percent_encode("%"), "%25");
}

/// 非 ASCII 字符按 UTF-8 逐字节编码。
#[test]
fn non_ascii_is_encoded_byte_by_byte_as_utf8() {
    assert_eq!(percent_encode("中"), "%E4%B8%AD");
    assert_eq!(percent_encode("é"), "%C3%A9");
}
