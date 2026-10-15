//! 仅测试：顶层表面积各测试文件共用的建池 / 建临时目录助手。

use std::path::PathBuf;

use super::*;

/// 建一个以临时目录为根的新连接池，并灌入内嵌 fixture 集。每个测试独享自己的目录，故彼此
/// 独立。返回目录，好让调用方按需 `keep` / `drop`。
/// 下面断言 `p_demo` 之类的测试显式选用 fixture；未播种的形状见 [`fresh_empty_pool`]。
pub(crate) fn fresh_pool() -> (DbPool, PathBuf) {
    let (pool, dir) = fresh_empty_pool();
    test_support::seed(&pool);
    (pool, dir)
}

/// 同 [`fresh_pool`] 但**不插入任何行**——真实首次启动留下的状态。测迁移、或测应用在空库上
/// 行为时用它。
pub(crate) fn fresh_empty_pool() -> (DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-db-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    // 即使上一次运行留下了文件也确保从干净状态开始。
    let _ = std::fs::remove_dir_all(&dir);
    let pool = init(&dir).expect("init");
    (pool, dir)
}

pub(crate) fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-schema-test-{}-{}-{}",
        tag,
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}
