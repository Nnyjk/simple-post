//! 两侧共用的词汇表。顺序敏感的那条在这里钉住——它是契约，不是实现细节。

use super::{escape_pointer_token, PATH_METHODS};

#[test]
fn a_slash_becomes_tilde_one() {
    assert_eq!(escape_pointer_token("/users/{id}"), "~1users~1{id}");
}

#[test]
fn a_tilde_is_escaped_before_the_slashes_are() {
    // 反过来做的话，`~1` 里那个 `~` 会被第二步再转一次，变成 `~01`。
    // `/files/~user` 是合法路径，所以这个顺序真的会遇到。
    assert_eq!(escape_pointer_token("/files/~user"), "~1files~1~0user");
    assert_eq!(
        escape_pointer_token("a~b/c"),
        "a~0b~1c",
        "两步各改一处时，两个字符都要按原样转义"
    );
}

#[test]
fn the_fixed_verb_set_is_the_one_the_spec_lists() {
    assert_eq!(PATH_METHODS.len(), 8);
    for verb in [
        "get", "put", "post", "delete", "options", "head", "patch", "trace",
    ] {
        assert!(PATH_METHODS.contains(&verb), "{verb} 必须能被导出");
    }
}
