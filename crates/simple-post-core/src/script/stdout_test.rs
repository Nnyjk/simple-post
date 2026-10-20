//! stdout 纪律的回归网。
//!
//! `simple-post-mcp` 的 stdout 只允许 JSON-RPC 帧，而沙箱住在 **core** —— 被 MCP 守护
//! 进程链接。于是脚本模块里任何 `println!` 都会污染 AI 客户端的流，而这类问题在功能
//! 测试里看不出来（它只是把客户端的协议流搞坏）。
//!
//! 故这条断言是**源码级**的：扫本模块的源文件文本。注释行跳过——否则这段说明自己的话
//! 就会把测试弄红。

/// 本模块的源文件。新增文件时把它加进来。
const SOURCES: &[(&str, &str)] = &[
    ("script/mod.rs", include_str!("mod.rs")),
    ("script/types.rs", include_str!("types.rs")),
    ("script/limits.rs", include_str!("limits.rs")),
    ("script/prelude.rs", include_str!("prelude.rs")),
    ("script/engine.rs", include_str!("engine.rs")),
    ("script/run.rs", include_str!("run.rs")),
];

/// 这一行是不是注释。
fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*')
}

#[test]
fn the_script_module_never_writes_to_stdout() {
    let mut offenders: Vec<String> = Vec::new();
    for (name, source) in SOURCES {
        for (index, line) in source.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            // `eprintln!` 走 stderr，允许；它含有 "println!" 字样，故先排掉。
            let code = line.replace("eprintln!", "");
            if code.contains("println!(") || code.contains("print!(") || code.contains("dbg!(") {
                offenders.push(format!("{name}:{}", index + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "脚本模块不得写 stdout（MCP 的 stdout 只允许 JSON-RPC 帧）：{offenders:?}"
    );
}
