//! Postman 的 `event[]`：两段脚本的导入，以及没有归宿的那些怎么出声。
//!
//! 断言的是**线形状**（key 指针、severity、dropped 文本、脚本源码），因为这几样是
//! 外部真的会读的东西：落库侧拿 key 当选择集、UI 拿 severity 分级、用户拿 dropped
//! 回到源文件。

use serde_json::json;

use crate::diagnostics::Severity;

use super::support::parse_ok;

/// 一条请求上的两段脚本要跟着它走。
///
/// Postman 把脚本挂在**条目**的 `event[]` 上（不在 `request` 里），`listen` 决定相位：
/// `prerequest` 是请求发出前跑的那段，`test` 是响应回来后跑的那段。
#[test]
fn an_item_level_script_lands_on_the_endpoint() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "带脚本的请求",
            "event": [
                { "listen": "prerequest", "script": { "exec": ["pm.environment.set('a', '1')"] } },
                { "listen": "test", "script": { "exec": ["pm.test('ok', function() {})"] } }
            ],
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    assert!(
        ex.diagnostics.is_empty(),
        "不该有诊断：{:?}",
        ex.diagnostics
    );
    let ep = &ex.modules[0].endpoints[0];
    assert_eq!(
        ep.scripts.pre_request_script.as_deref(),
        Some("pm.environment.set('a', '1')")
    );
    assert_eq!(
        ep.scripts.test_script.as_deref(),
        Some("pm.test('ok', function() {})")
    );
}

/// 嵌套文件夹上的脚本落在**那个集合**上。
///
/// 集合能存脚本（`node_scripts` 表），故嵌套 folder 的脚本有归宿。
#[test]
fn a_nested_folder_script_lands_on_the_collection() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "顶层",
            "item": [{
                "name": "中层",
                "event": [
                    { "listen": "prerequest", "script": { "exec": ["console.log('中层')"] } }
                ],
                "item": [{
                    "name": "里面的请求",
                    "request": { "method": "GET", "url": "https://api.example.com/b" }
                }]
            }]
        }]
    }));

    let ex = &parsed.exchange;
    assert!(
        ex.diagnostics.is_empty(),
        "不该有诊断：{:?}",
        ex.diagnostics
    );
    let collection = &ex.modules[0].collections[0];
    assert_eq!(collection.name, "中层");
    assert_eq!(
        collection.scripts.pre_request_script.as_deref(),
        Some("console.log('中层')")
    );
    assert!(collection.scripts.test_script.is_none());
}

/// 同一相位的多项脚本**分块**拼成一段，不是只留第一项。
///
/// Postman 里它们依次执行，只留第一项等于悄悄改掉了用户的集合行为。分块是为了
/// 各自有独立的作用域：同名 `const` 在同一层会撞成 `SyntaxError`，两段都不跑。
#[test]
fn several_scripts_of_the_same_phase_are_joined_as_independent_blocks() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "两段前置脚本",
            "event": [
                { "listen": "prerequest", "script": { "exec": ["a();", "b();"] } },
                { "listen": "prerequest", "script": { "exec": "c();" } }
            ],
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    assert_eq!(
        ex.modules[0].endpoints[0]
            .scripts
            .pre_request_script
            .as_deref(),
        Some("{\na();\nb();\n}\n{\nc();\n}")
    );
}

/// 只有一段时不包块：多余的括号会让人在预览里读不懂脚本。
#[test]
fn a_single_script_is_kept_verbatim() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "一段前置脚本",
            "event": [
                { "listen": "prerequest", "script": { "exec": ["a();"] } }
            ],
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    assert_eq!(
        ex.modules[0].endpoints[0]
            .scripts
            .pre_request_script
            .as_deref(),
        Some("a();")
    );
}

/// 被 Postman 禁用的脚本不导入。
///
/// `disabled: true` 的 event 在 Postman 里不跑；导入后它若变成会跑的，就能改写请求、
/// 能写环境变量——那是把作者特意关掉的东西打开了。
#[test]
fn a_disabled_event_is_not_imported() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "禁用的脚本",
            "event": [
                { "listen": "prerequest", "script": { "exec": ["a();"] } },
                { "listen": "test", "script": { "exec": ["b();"] }, "disabled": true }
            ],
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    let ep = &ex.modules[0].endpoints[0];
    assert_eq!(ep.scripts.pre_request_script.as_deref(), Some("a();"));
    assert!(
        ep.scripts.test_script.is_none(),
        "被禁用的脚本不能变成会跑的：{:?}",
        ep.scripts
    );

    let infos: Vec<_> = ex
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(infos.len(), 1, "{:?}", ex.diagnostics);
    assert_eq!(infos[0].pointer, "/item/0/event/1");
    assert_eq!(infos[0].dropped.as_deref(), Some("禁用的脚本"));
}

/// 整份集合根上的脚本**没有归宿**，逐条出诊断。
///
/// 本仓库的脚本住在接口与集合上，而「整份集合」不是一个节点：顶层条目各自成一个
/// 模块，挂进已有模块时那些模块降级成集合，根上的脚本挂不到任何一个上。
///
/// 不静默丢——用户导入一份带集合级脚本的集合时，该知道那批脚本没进来。
#[test]
fn a_collection_level_script_is_reported_because_it_has_no_home() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "event": [
            { "listen": "prerequest", "script": { "exec": ["console.log('整份集合')"] } }
        ],
        "item": [{
            "name": "请求",
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    let warns: Vec<_> = ex
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect();
    assert_eq!(warns.len(), 1, "{:?}", ex.diagnostics);
    assert_eq!(warns[0].pointer, "/event/0");
    assert_eq!(warns[0].dropped.as_deref(), Some("集合级脚本"));
    // 诊断里不能带脚本源码：用户的脚本里常有 token 与环境变量名，而诊断是要显示
    // 给用户看的。
    assert!(
        !warns[0].message.contains("console.log"),
        "源码不该进诊断：{}",
        warns[0].message
    );
}

/// 本仓库只有两个相位；别的 `listen` 没有对应构造，出诊断说明。
#[test]
fn an_unknown_event_kind_is_reported() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "监听器",
            "event": [
                { "listen": "prerequest", "script": { "exec": ["a();"] } },
                { "listen": "unknown-phase", "script": { "exec": ["b();"] } }
            ],
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    let warns: Vec<_> = ex
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect();
    assert_eq!(warns.len(), 1, "{:?}", ex.diagnostics);
    assert_eq!(warns[0].pointer, "/item/0/event/1");
    assert_eq!(warns[0].dropped.as_deref(), Some("event unknown-phase"));
    // 认得的那一项照样导入。
    assert_eq!(
        ex.modules[0].endpoints[0]
            .scripts
            .pre_request_script
            .as_deref(),
        Some("a();")
    );
}

/// 读不出源码的 event 要出声，且指针指到它自己那一条。
#[test]
fn an_unreadable_script_is_reported_with_its_own_pointer() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "坏脚本",
            "event": [
                { "listen": "prerequest", "script": { "exec": ["a();"] } },
                { "listen": "test" },
                { "listen": "test", "script": { "exec": [1, 2] } }
            ],
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    let warns: Vec<_> = ex
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect();
    assert_eq!(warns.len(), 2, "{:?}", ex.diagnostics);
    assert_eq!(warns[0].pointer, "/item/0/event/1");
    assert_eq!(warns[1].pointer, "/item/0/event/2");
    assert_eq!(
        ex.modules[0].endpoints[0]
            .scripts
            .pre_request_script
            .as_deref(),
        Some("a();")
    );
    assert!(ex.modules[0].endpoints[0].scripts.test_script.is_none());
}

/// 同一段脚本重复出现只报一次。
///
/// 同一段源码出现十次是一个问题，不是十个——与 `Ctx::seen_bad_tokens` 那套去重纪律一样。
#[test]
fn the_same_script_reported_twice_is_reported_once() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "event": [
            { "listen": "prerequest", "script": { "exec": ["console.log('同一段')"] } },
            { "listen": "prerequest", "script": { "exec": ["console.log('同一段')"] } },
            { "listen": "prerequest", "script": { "exec": ["console.log('另一段')"] } }
        ],
        "item": [{
            "name": "请求",
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    let warns: Vec<_> = ex
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect();
    assert_eq!(warns.len(), 2, "同一段只报一次：{:?}", ex.diagnostics);
    assert_eq!(warns[0].pointer, "/event/0");
    assert_eq!(warns[1].pointer, "/event/2");
}

/// 没有 `event[]` 的集合不受影响。
#[test]
fn a_collection_without_events_is_untouched() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "普通请求",
            "request": { "method": "GET", "url": "https://api.example.com/a" }
        }]
    }));

    let ex = &parsed.exchange;
    assert!(ex.diagnostics.is_empty(), "{:?}", ex.diagnostics);
    assert!(ex.modules[0].endpoints[0].scripts.is_empty());
}
