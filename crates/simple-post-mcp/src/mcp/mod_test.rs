use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录
/// （cargo 默认并行跑测试）。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 测试用的廉价假连接池。`r2d2::Pool` 对 manager 泛型，没有真实
/// `SqliteConnectionManager` 就构造不出来 —— 需要真库的单测改用 `db::init` 开临时
/// 文件；只跑 dispatch 与参数解析的快速验证用这个空壳足够。
///
/// **临时目录必须逐次调用唯一。** 用固定的 `simple-post-mcp-test` 会读起来像不相干的
/// 偶发失败：`db::init` → `ensure_current_schema` 会先删掉它即将替换的 `store.db`，
/// 而本文件的测试跑在并行线程上，两个调用方抢同一个文件，输的那个在 Windows 上死于
/// `os error 32`（文件被占用）。
fn test_state() -> Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-test-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// `tools/list` 覆盖每个已注册工具，含全部类别。主要回归网 —— 加了工具却忘了登记
/// dispatch 表时它会抓到。
#[test]
fn tools_list_returns_all_registered_tools() {
    let names = registered_tool_names();
    assert!(!names.is_empty(), "no tools registered");

    // 抽查：每个类别至少一个工具。
    let has = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
    assert!(has("env_"), "missing env_* tools");
    assert!(has("project_"), "missing project_* tools");
    assert!(has("module_"), "missing module_* tools");
    assert!(has("collection_"), "missing collection_* tools");
    assert!(has("endpoint_"), "missing endpoint_* tools");
    assert!(has("base_url_def_"), "missing base_url_def_* tools");
    assert!(has("api_run_request"), "missing api_run_request");
    assert!(has("api_call_endpoint"), "missing api_call_endpoint");
    assert!(has("api_resolve_"), "missing api_resolve_* tools");
    assert!(has("api_validate_"), "missing api_validate_* tools");
    assert!(has("api_parse_response"), "missing api_parse_response");
    assert!(has("api_diff_responses"), "missing api_diff_responses");
    assert!(has("api_extract_jsonpath"), "missing api_extract_jsonpath");
    assert!(has("dynamic_var_"), "missing dynamic_var_* tools");
    assert!(has("spec_"), "missing spec_* tools");

    // 工具总数抽查 —— 断言刻意放宽，好让后来加工具不必改测试，但紧到能抓到整个
    // 类别被弄丢。
    assert!(
        names.len() >= 30,
        "tool count too low: {} — did a category get dropped?",
        names.len()
    );
}

/// `REQUIRED_CAPABILITY` 对注册表必须**穷尽**。
///
/// 回归网：两道能力门都把缺行当作「未分类」，而未分类一律 fail closed（拒绝）。
/// 有了本测试，下一个工具就不会在注册表里悄悄绕过用户选的策略。
#[test]
fn every_registered_tool_has_a_capability() {
    let missing: Vec<&str> = tool_table()
        .iter()
        .filter(|e| required_capability(e.name).is_none())
        .map(|e| e.name)
        .collect();
    assert!(
        missing.is_empty(),
        "tools missing a REQUIRED_CAPABILITY row (they bypass the global \
             policy gate): {missing:?}"
    );
}

/// 精确的工具总数，断言在一个文档能指着的地方。
///
/// `AGENTS.md` 与 `README.md` 都写着「52 tools」。上面的双射测试在「工具连同它的
/// capability 行一起增删」时仍然绿，光靠它守不住这两个数字。改注册表的同一个 commit
/// 里要改这个常量，同一个 commit 也应更新那两份文档。
const EXPECTED_TOOL_COUNT: usize = 52;

#[test]
fn registered_tool_count_matches_the_documented_figure() {
    assert_eq!(
        tool_table().len(),
        EXPECTED_TOOL_COUNT,
        "tool count changed; update EXPECTED_TOOL_COUNT and the '52 tools' \
             claims in AGENTS.md and README.md"
    );
}

/// 反方向：指向一个已不存在的工具的 capability 行，是会掩盖注册表改名的死配置。
#[test]
fn every_capability_row_names_a_registered_tool() {
    let registered = registered_tool_names();
    let stale: Vec<&str> = REQUIRED_CAPABILITY
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !registered.iter().any(|r| r == n))
        .collect();
    assert!(
        stale.is_empty(),
        "REQUIRED_CAPABILITY rows with no matching tool: {stale:?}"
    );
}

/// `tools/list` 的体积有一个上限，理由是**它每一次会话都要付钱**。
///
/// 客户端在动手之前要先把整份清单读进上下文，于是 52 条描述符的每一个字节都乘以
/// 「会话数」。详细的解释属于 `simple-post://spec-authoring` 那份按需读取的指南，
/// 不属于这里：工具描述只说「这个参数是什么」，长篇的规则与例子放指南里。
///
/// **上限怎么算出来的。** 早先这里用「字符数 ÷ 3」估 token 数，于是 39 642 字符
/// 被说成 13 214 tokens —— 那个数高估了六成：JSON 的标点碎，一个 token 覆盖四个
/// 多字符。实测（`tiktoken` 的 `cl100k_base`，对**紧凑** JSON，即真正上线的形状）
/// 是 40 702 字符 ≈ **8 652 tokens**，比值约 **4.70 字符 / token**。
/// 故这个上限按字符写：**41 400 字符 ≈ 8 800 tokens**，留一点余量给下一个工具。
///
/// 上限第一次重标定，是因为路径参数（`path_params`）进了三个工具的契约：
/// `endpoint_create`、`endpoint_update` 与 `spec_upsert` 各自多出一段说明，而
/// `api_call_endpoint` 的 `overrides` 也要写清那几个字段。契约变大，体积就变大 ——
/// 这时候该做的是把新增的那几段写短，而不是砍掉契约本身。
const TOOLS_LIST_BUDGET_CHARS: usize = 41_400;

#[test]
fn the_tools_list_payload_stays_under_its_budget() {
    let descriptors: Vec<ToolDescriptor> = tool_table()
        .iter()
        .map(|e| ToolDescriptor {
            name: e.name.to_string(),
            description: e.description.to_string(),
            input_schema: e.schema.clone(),
        })
        .collect();
    // 紧凑序列化：真正上线的形状。`serde_json::to_string` 不加空格，与
    // `tools/list` 响应里 axum 的 `Json` 响应体一致。
    let payload = serde_json::to_string(&descriptors).expect("描述符序列化成 JSON");
    assert!(
        payload.len() <= TOOLS_LIST_BUDGET_CHARS,
        "tools/list 是 {} 字符（约 {} tokens），超过上限 {} 字符。\
         详细的规则与例子放 `simple-post://spec-authoring` 那份指南，别往工具描述里塞。",
        payload.len(),
        payload.len() * 100 / 473,
        TOOLS_LIST_BUDGET_CHARS
    );
}

/// 打印 `tools/list` 的体积与逐条分布，供调整描述时对照。
///
/// 这个测试不做断言，只报告数字，并把整份负载写到 `target/tools-list.json`，
/// 好让外面的分词器量真实 token 数。用 `cargo test -p simple-post-mcp -- --nocapture` 跑。
#[test]
fn report_the_tools_list_size() {
    let mut rows: Vec<(String, usize, usize)> = tool_table()
        .iter()
        .map(|e| {
            let schema = serde_json::to_string(&e.schema).unwrap_or_default().len();
            let description = e.description.len();
            (e.name.to_string(), schema, description)
        })
        .collect();
    rows.sort_by_key(|(_, schema, _)| std::cmp::Reverse(*schema));
    let total_schema: usize = rows.iter().map(|(_, s, _)| *s).sum();
    let total_description: usize = rows.iter().map(|(_, _, d)| *d).sum();
    for (name, schema, description) in &rows {
        println!("{name:28} schema {schema:6}  description {description:5}");
    }
    println!(
        "TOTAL schema {total_schema} + description {total_description} = {}",
        total_schema + total_description
    );

    let descriptors: Vec<ToolDescriptor> = tool_table()
        .iter()
        .map(|e| ToolDescriptor {
            name: e.name.to_string(),
            description: e.description.to_string(),
            input_schema: e.schema.clone(),
        })
        .collect();
    let payload = serde_json::to_string_pretty(&descriptors).expect("描述符序列化成 JSON");
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tools-list.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, payload).expect("写下 tools-list.json");
    println!("wrote {}", path.display());
}

/// Gate B 回归网 —— 权限检查结果绝不丢弃。
///
/// 对 `check_permission` 的结果调用 `.ok()` 会静默吞掉 `PermissionDenied`，于是这道
/// 按项目的门对那个工具形同虚设 —— 包括唯一会发外网请求的 `api_call_endpoint`。要用
/// `?` 传播。
#[test]
fn no_permission_check_result_is_discarded() {
    let offenders = source_files()
        .into_iter()
        .filter_map(|(path, text)| {
            let swallowed = text.lines().any(|l| l.trim_start().starts_with(".ok();"));
            let gates = text.contains("check_permission(");
            (swallowed && gates).then_some(path)
        })
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "these files call check_permission and also end a statement with `.ok();` \
         — confirm each one is a deliberate, non-permission discard: {offenders:?}"
    );
}

/// Gate B 回归网 —— 绝不用字面量 "global" 过门。
///
/// `agent_permissions.project_id` 是指向 `projects(id)` 的外键，而项目 id 是
/// `proj_<millis>` 铸出来的，所以 "global" 永远不可能有对应行。查不到就退回
/// `Permission::default()` = `ReadCall`，而任何写工具要求 `Full` —— `ReadCall < Full`
/// 于是拒绝，工具看着注册得好好的，却被永久拒绝。
///
/// 按 `collection_id` / `module_id` 收窄时更严重：那不是工具失效，而是根本没有归属
/// 项目可查，真实拥有者的权限一次也没被读过。
///
/// 现在 handler 一律从调用方给的东西解析真实归属（模块 / 集合走 `nodes::root_id_of`，
/// 项目就是 id 本身），并且**拒绝无范围的调用**而不是编一个 id —— 跨项目的读没有单一
/// 项目可授权。于是不存在「正确使用这个字面量」的场景，允许清单为空，本测试无条件执行。
#[test]
fn no_gate_is_passed_the_literal_global() {
    let offenders = source_files()
        .into_iter()
        .filter_map(|(path, text)| {
            // 匹配的是*参数*，而不是某一种调用写法，所以无论直接传字面量，还是
            // `unwrap_or` 出字面量，都抓得到。扫描到 `;` 为止，因为参数位于
            // `&state.pool` 这类嵌套括号之后，天真的「扫到第一个 `)`」会提前停下。
            let offenders = text
                .match_indices("check_permission(")
                .filter_map(|(i, _)| {
                    let tail = &text[i..];
                    let end = tail.find(';')?;
                    let call = &tail[..end];
                    if !call.contains("\"global\"") {
                        return None;
                    }
                    Some((path.clone(), call.trim().replace('\n', " ")))
                })
                .collect::<Vec<_>>();
            (!offenders.is_empty()).then_some(offenders)
        })
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "these files gate against the literal \"global\", which is not a project id \
         and makes every call consult no real permission row: {offenders:?}"
    );
}

/// 本 crate 的每个 `.rs` 文件及其内容。上面两个测试刻意做成源码级而非行为级：它们守
/// 的缺陷对一套全绿的测试是不可见的 —— 被永久拒绝的工具照样返回一个格式完好的
/// `PermissionDenied`。
fn source_files() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                // 跳过测试模块：本文件在自己的文档注释里就引用了它要找的那种调用
                // 写法，扫到自己会永远把自己报成违规者。
                let is_test = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("_test.rs") || n == "mod_test.rs");
                if is_test {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((path.display().to_string(), text));
                }
            }
        }
    }
    assert!(!out.is_empty(), "source_files() found no .rs files");
    out
}

/// `dispatch` 遇到未知名字返回 `NotFound` —— JSON-RPC 处理器会把它折成
/// `is_error: true`。
#[tokio::test]
async fn dispatch_unknown_tool_returns_not_found() {
    let state = test_state();
    let res = dispatch(&state, "does_not_exist", json!({})).await;
    assert!(matches!(
        res,
        Err(simple_post_core::error::AppError::NotFound(_))
    ));
}

/// 参数辅助函数在边界情形下行为正确：缺键、类型不对、null。
#[test]
fn arg_helpers_handle_missing_or_wrong_type() {
    let v = json!({ "name": "x", "count": 7, "flag": true });
    assert_eq!(arg_str(&v, "name").unwrap(), "x");
    assert!(arg_str(&v, "missing").is_err());
    assert!(arg_str(&v, "count").is_err());
    assert_eq!(arg_u64(&v, "count").unwrap(), 7);
    assert!(arg_bool(&v, "flag").unwrap());

    let v2 = json!({});
    assert_eq!(arg_str_opt(&v2, "x"), None);
    assert_eq!(arg_u64_opt(&v2, "x"), None);
    assert!(!arg_bool_opt(&v2, "x"));
}

/// `JsonRpcResponse::ok` 产出合法的响应信封。
#[test]
fn json_rpc_response_ok_shape() {
    let r = JsonRpcResponse::ok(Some(json!(1)), json!({"x": 1}));
    assert_eq!(r.jsonrpc, "2.0");
    assert_eq!(r.id, json!(1));
    assert!(r.result.is_some());
    assert!(r.error.is_none());
}

/// `JsonRpcResponse::err` 产出错误信封。
#[test]
fn json_rpc_response_err_shape() {
    let r = JsonRpcResponse::err(Some(json!(2)), -32601, "method not found");
    assert_eq!(r.jsonrpc, "2.0");
    assert_eq!(r.id, json!(2));
    assert!(r.result.is_none());
    let e = r.error.unwrap();
    assert_eq!(e.code, -32601);
    assert_eq!(e.message, "method not found");
}

// ===========================================================================
// MCP 线格式合规
//
// 以下都是源码级断言。描述符序列化成 `input_schema` 而不是 `inputSchema` 时，
// dispatch 一切正常：每个 handler 都拿到正确的 `arguments`，每个单测都通过，唯一症状
// 只是合规客户端以为该工具没有参数。功能测试抓不到，只有断言序列化结果才抓得到。
// ===========================================================================

/// spec 里这个字段叫 `inputSchema`。序列化成 `input_schema` 时，每个合规客户端都会看到
/// 一批没有参数的工具，LLM 只能靠描述文字去猜参数列表。
#[test]
fn descriptor_emits_spec_camel_case_field_names() {
    let d = ToolDescriptor {
        name: "t".into(),
        description: "d".into(),
        input_schema: json!({"type": "object"}),
    };
    let v = serde_json::to_value(&d).unwrap();
    assert!(
        v.get("inputSchema").is_some(),
        "spec requires `inputSchema`; got {v}"
    );
    assert!(v.get("input_schema").is_none(), "snake_case leak: {v}");
}

/// 逐个工具检查，不只是上面那个手工构造的描述符。新工具若绕过 `ToolDescriptor` 直接
/// 拼结构体字面量，只有这个测试能抓到。
#[test]
fn every_registered_descriptor_uses_camel_case_schema_key() {
    let state = test_state();
    for entry in tool_table() {
        let v = serde_json::to_value(ToolDescriptor {
            name: entry.name.to_string(),
            description: entry.description.to_string(),
            input_schema: entry.schema.clone(),
        })
        .unwrap();
        assert!(
            v.get("inputSchema").is_some(),
            "tool {} serialises without inputSchema: {v}",
            entry.name
        );
    }
    drop(state);
}

/// spec 把失败标志写作 `isError`。写成 `is_error` 会被客户端静默丢弃，于是被拒绝或
/// 出错的工具调用读起来像成功。
#[test]
fn error_call_tool_result_emits_spec_is_error_key() {
    let v = serde_json::to_value(CallToolResult::err("boom")).unwrap();
    assert_eq!(
        v.get("isError").and_then(|x| x.as_bool()),
        Some(true),
        "{v}"
    );
    assert!(v.get("is_error").is_none(), "snake_case leak: {v}");
}

/// 成功时必须**省略**这个标志，而不是发 `isError: false` —— 有的客户端把显式 false
/// 当成失败信号，而 spec 也把它列为可选。
#[test]
fn success_call_tool_result_omits_error_flag_entirely() {
    let v = serde_json::to_value(CallToolResult::json(json!({"ok": true}))).unwrap();
    assert!(v.get("isError").is_none(), "flag should be omitted: {v}");
    assert!(v.get("is_error").is_none(), "{v}");
    assert!(v.get("content").is_some());
}

/// `handle_tools_call` 的失败分支手写 JSON、不经 `CallToolResult`，所以结构体上那个
/// 属性管不到它 —— spec 的 `isError` 拼写必须在这里也手打一遍。之所以源码级断言，是
/// 因为症状只在 HTTP 上才观察得到。
#[test]
fn daemon_http_error_arm_types_the_spec_spelling() {
    let src = include_str!("../daemon_http.rs");
    // 刻意跳过注释行：该文件正在记录这个缺陷本身，裸 `src.contains` 会命中它自己的
    // 说明文字，从而因错误的原因失败。
    let code: Vec<&str> = src
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .collect();
    let code = code.join("\n");

    assert!(
        !code.contains("\"is_error\": true"),
        "daemon_http hand-writes the error envelope and must use the spec's \
         `isError`; a bare `is_error` there is invisible to clients"
    );
    assert!(
        code.contains("\"isError\": true"),
        "expected the hand-written error envelope to spell isError"
    );
}
