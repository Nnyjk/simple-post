//! 两侧薄壳必须调共享层 —— 这一层存在的全部意义就在这里。
//!
//! 有人把编排抄回某一侧时，两条 transport 就有了两个答案，而没有一处会提醒它们分叉了。
//! 故这里直接看源码：六个薄壳入口都得调 `project_api`，也都不得再自己拼那一段落库。
//!
//! 源码级而非行为级，是刻意的：这一条断言的正是「两边调的是同一个函数」这件事本身，
//! 它在行为上等价（同一个函数自然有同一个行为），只有源码才说得清「谁调了谁」。
//! 同 `mcp/mod_test.rs` 里那几条源码级断言是同一个道理。

/// 六个薄壳入口：源码路径、源码本身、它必须调的片段、它不得再自己拼的那段编排。
///
/// 「必须调的」是共享层本身。REST 侧那两个收 `Json<Value>` 的薄壳（`projects` 与
/// `nodes`）还必须调 `ensure_server_id(`——body 里的 `id` 缺失时由它补上，缺了它，
/// 收不到 id 的请求会变成 400。`base_url_defs` 收的是结构化 DTO，不走那一步。
///
/// 「不得再自己拼」的那些片段是**落库那一步**：以前每个薄壳都自己调 `db::…::create`
/// 那一族，于是同一个问题有两个答案。片段都带左括号 —— 只看函数名会把文档里提到
/// `db::nodes::create` 的那句话也算成一处调用。
const SHELLS: &[(&str, &str, &[&str], &[&str])] = &[
    (
        "src-tauri/src/commands/projects.rs",
        include_str!("../../../src-tauri/src/commands/projects.rs"),
        &["project_api::"],
        &["projects::create_with_defaults("],
    ),
    (
        "src-tauri/src/commands/base_url_defs.rs",
        include_str!("../../../src-tauri/src/commands/base_url_defs.rs"),
        &["project_api::"],
        &["base_url_defs::create(", "base_url_defs::set_default("],
    ),
    (
        "src-tauri/src/commands/nodes.rs",
        include_str!("../../../src-tauri/src/commands/nodes.rs"),
        &["project_api::"],
        &["nodes::create(", "nodes::root_id_of("],
    ),
    (
        "web_api/routes/projects.rs",
        include_str!("web_api/routes/projects.rs"),
        &["project_api::", "ensure_server_id("],
        &["projects::create_with_defaults("],
    ),
    (
        "web_api/routes/base_url_defs.rs",
        include_str!("web_api/routes/base_url_defs.rs"),
        &["project_api::"],
        &["base_url_defs::create(", "base_url_defs::set_default("],
    ),
    (
        "web_api/routes/nodes.rs",
        include_str!("web_api/routes/nodes.rs"),
        &["project_api::", "ensure_server_id("],
        &["nodes::create(", "nodes::root_id_of("],
    ),
];

#[test]
fn both_transports_call_the_shared_layer() {
    for (path, text, required, forbidden) in SHELLS {
        for snippet in *required {
            assert!(
                text.contains(snippet),
                "{path} 这个薄壳没有调 `{snippet}`，少做的那一步没人替它做"
            );
        }
        for snippet in *forbidden {
            assert!(
                !text.contains(snippet),
                "{path} 又自己拼了 `{snippet}`，编排必须住在 project_api 里"
            );
        }
    }
}
