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
    ("script/chain.rs", include_str!("chain.rs")),
    ("script/send.rs", include_str!("send.rs")),
    ("script/transform.rs", include_str!("transform.rs")),
];

/// 本模块的测试文件后缀。它们不属于「要守的源码」——扫它们只会扫到本文件自己的说明。
const TEST_SUFFIX: &str = "_test.rs";

/// 目录里真实的源文件名（不含 `*_test.rs`）。
///
/// `include_str!` 是编译期的，拿不到运行时列表，所以清单只能手写；这个函数把「手写」
/// 变成「可校验」：清单少一个文件时，下面那条测试会把缺的名字打出来。
fn module_sources() -> Vec<String> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/script");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("读不到 {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".rs") && !name.ends_with(TEST_SUFFIX))
        .collect();
    names.sort();
    names
}

/// 清单与目录必须一致——双向。
///
/// 单向（清单 ⊆ 目录）挡不住「新加了文件但忘了加进清单」，而漏登记正是这条哨兵唯一的
/// 失效方式：新文件里的 `println!` 不扫就是绿的。
#[test]
fn every_module_source_file_is_guarded() {
    let mut listed: Vec<String> = SOURCES
        .iter()
        .map(|(name, _)| name.rsplit('/').next().unwrap_or(name).to_owned())
        .collect();
    listed.sort();

    let on_disk = module_sources();
    let missing: Vec<&String> = on_disk.iter().filter(|n| !listed.contains(n)).collect();
    assert!(
        missing.is_empty(),
        "script/ 下这些源文件不在 SOURCES 里（加进清单，否则它们的 println! 不会被扫）：{missing:?}"
    );

    let stale: Vec<&String> = listed.iter().filter(|n| !on_disk.contains(n)).collect();
    assert!(stale.is_empty(), "SOURCES 里有已删/改名的文件：{stale:?}");
}

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
