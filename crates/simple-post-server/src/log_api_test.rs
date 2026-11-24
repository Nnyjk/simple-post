//! 日志导出的行为用例。
//!
//! 缓冲区是进程级全局状态，故每条用例都用自己独有的 `target` 认自己的记录，不断言整份
//! 文本的总长度。

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use simple_post_core::logbuf::{self, LogLevel};

use crate::log_api::{
    append_client_event, export, export_to_disk_in, log_dir_at, redact_credentials,
    CLIENT_FIELD_LIMIT_CHARS,
};

/// 文件名形如 `simple-post-logs-20261118-112909.txt`：前缀固定，中间 15 个字符的时间戳，
/// 后缀 `.txt`。
fn assert_file_name(name: &str, prefix: &str) {
    assert!(
        name.starts_with(&format!("{prefix}-")),
        "{name} 不以 {prefix}- 开头"
    );
    assert!(name.ends_with(".txt"), "{name} 不以 .txt 结尾");
    let stamp = name
        .trim_start_matches(&format!("{prefix}-"))
        .trim_end_matches(".txt");
    assert_eq!(stamp.len(), 15, "{stamp} 不是 YYYYMMDD-HHMMSS");
}

#[test]
fn all_and_errors_have_distinct_file_names() {
    let all = export("all").expect("all 是合法范围");
    assert_file_name(&all.file_name, "simple-post-logs");

    let errors = export("errors").expect("errors 是合法范围");
    assert_file_name(&errors.file_name, "simple-post-errors");
}

#[test]
fn an_unknown_scope_is_a_validation_error() {
    let err = export("warnings").expect_err("未知范围必须报错");
    assert_eq!(err.code(), "validation");
}

#[test]
fn the_errors_scope_drops_info_lines() {
    logbuf::record(LogLevel::Info, "test::scope", "info-only-marker");
    logbuf::record(LogLevel::Error, "test::scope", "error-marker");

    let errors = export("errors").expect("errors 是合法范围").content;
    assert!(errors.contains("error-marker"));
    assert!(
        !errors.contains("info-only-marker"),
        "errors 范围不该带上 Info 级记录"
    );

    let all = export("all").expect("all 是合法范围").content;
    assert!(all.contains("info-only-marker"));
    assert!(all.contains("error-marker"));
}

#[test]
fn a_client_event_lands_under_webview_source_and_keeps_the_detail() {
    append_client_event("error", "smoke", "render failed", Some("stack line 1"))
        .expect("客户端上报不因内容失败");

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "webview:smoke")
        .expect("应当写进缓冲区");
    assert_eq!(hit.level, LogLevel::Error);
    assert_eq!(hit.message, "render failed\nstack line 1");
}

#[test]
fn a_blank_detail_adds_no_extra_line() {
    append_client_event("warn", "smoke-blank", "no detail", Some("   "))
        .expect("空 detail 不是错误");

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "webview:smoke-blank")
        .expect("应当写进缓冲区");
    assert_eq!(hit.message, "no detail");
}

#[test]
fn an_unknown_level_falls_back_to_info() {
    append_client_event("verbose", "smoke-level", "typed wrong", None)
        .expect("级别拼错不该让写入失败");

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "webview:smoke-level")
        .expect("应当写进缓冲区");
    assert_eq!(hit.level, LogLevel::Info);
}

#[test]
fn client_fields_are_clamped_by_chars() {
    let huge = "x".repeat(CLIENT_FIELD_LIMIT_CHARS * 2);
    append_client_event("info", "smoke-clamp", &huge, None).expect("超长正文仍应写入");

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "webview:smoke-clamp")
        .expect("应当写进缓冲区");
    assert_eq!(hit.message.chars().count(), CLIENT_FIELD_LIMIT_CHARS);
}

/// 多字节正文再被缓冲区按 8 KiB 字节上限截一次；截点必须落在字符边界上，否则导出里会
/// 出现替换字符。
#[test]
fn a_long_multibyte_message_is_cut_on_a_char_boundary() {
    let huge = "字".repeat(CLIENT_FIELD_LIMIT_CHARS * 2);
    append_client_event("info", "smoke-multibyte", &huge, None).expect("超长正文仍应写入");

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "webview:smoke-multibyte")
        .expect("应当写进缓冲区");
    assert_eq!(
        hit.message.chars().count(),
        logbuf::MESSAGE_LIMIT_BYTES / '字'.len_utf8()
    );
    assert!(!hit.message.contains('\u{FFFD}'));
}

/// `source` 进的是记录的 target —— 不截它，一条上报就能把缓冲区吃满。
#[test]
fn an_overlong_source_is_clamped_by_chars() {
    let huge = "s".repeat(CLIENT_FIELD_LIMIT_CHARS * 2);
    append_client_event("info", &huge, "short body", None).expect("超长 source 仍应写入");

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target.starts_with("webview:ssss"))
        .expect("应当写进缓冲区");
    let source = hit
        .target
        .strip_prefix("webview:")
        .expect("target 形如 webview:<source>");
    assert_eq!(source.chars().count(), CLIENT_FIELD_LIMIT_CHARS);
}

/// 导出的文件是给人看的：能拿来直接调接口的凭据不该原样躺在里面。
#[test]
fn credentials_are_redacted_in_both_forms() {
    let cases = [
        // header 全写，scheme 大小写不敏感。
        (
            "Authorization: Bearer abcd1234",
            "Authorization: Bearer [redacted]",
        ),
        (
            "authorization: basic dXNlcjpwYXNz",
            "authorization: basic [redacted]",
        ),
        // 裸 scheme：正文里粘一段 curl 就是这样。
        ("curl -H 'Bearer sk-live-42'", "curl -H 'Bearer [redacted]'"),
        ("scheme Basic Zm9vYmFy", "scheme Basic [redacted]"),
        // 带引号的 JSON 字段，键名大小写不敏感。
        (r#"{"token": "abc123"}"#, r#"{"token": "[redacted]"}"#),
        (
            r#"{"authToken": "xyz-789", "ok": true}"#,
            r#"{"authToken": "[redacted]", "ok": true}"#,
        ),
        (
            r#"{"AUTHTOKEN":"no-space"}"#,
            r#"{"AUTHTOKEN":"[redacted]"}"#,
        ),
    ];
    for (input, want) in cases {
        assert_eq!(redact_credentials(input), want, "输入：{input}");
    }
}

/// 普通文本必须原样通过：脱敏不该改写日志内容。
#[test]
fn plain_text_is_left_untouched() {
    for text in [
        "render failed: TypeError: x is not a function",
        "GET /api/requests 200 in 12ms",
        r#"{"level":"error","code":"E1001"}"#,
        "",
    ] {
        assert_eq!(redact_credentials(text), text, "输入：{text}");
    }
}

/// 上报路径也要脱敏 —— 只在导出侧抹是把凭据先存进了内存缓冲。
#[test]
fn a_client_event_redacts_message_and_detail() {
    append_client_event(
        "error",
        "smoke-redact",
        "request failed with Authorization: Bearer top-secret",
        Some(r#"body {"authToken": "also-secret"}"#),
    )
    .expect("客户端上报不因内容失败");

    let hit = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "webview:smoke-redact")
        .expect("应当写进缓冲区");
    assert!(!hit.message.contains("top-secret"), "{}", hit.message);
    assert!(!hit.message.contains("also-secret"), "{}", hit.message);
    assert!(hit.message.contains("Authorization: Bearer [redacted]"));
    assert!(hit.message.contains(r#"{"authToken": "[redacted]"}"#));
}

/// 桌面端落盘：文件名与 `export` 同一条规则，内容就是那份文本。
///
/// 「没有日志时返回 `None`」不在这里断言：`logbuf` 是进程级全局缓冲，同进程的其它用例
/// 会往里写 Error 级记录，没有可靠的清空手段，构造空态只能靠时序运气。
#[test]
fn export_to_disk_writes_the_text_under_the_given_dir() {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("simple-post-log-disk-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    logbuf::record(LogLevel::Error, "test::disk", "disk-export-marker");
    let file = export_to_disk_in(&dir, "errors")
        .expect("写盘不该失败")
        .expect("刚写入一条 Error，导出不空");

    assert_file_name(&file.file_name, "simple-post-errors");
    assert_eq!(Path::new(&file.path), dir.join(&file.file_name));
    let written = std::fs::read_to_string(&file.path).expect("文件已落盘");
    assert!(written.contains("disk-export-marker"), "{written}");

    std::fs::remove_dir_all(&dir).ok();
}

/// 目录规则：基准目录下 `simple-post/logs`，与 `mcp.port` 同源。
#[test]
fn log_dir_nests_under_the_app_segment() {
    assert_eq!(
        log_dir_at(Path::new("base")),
        Path::new("base").join("simple-post").join("logs")
    );
}
