//! 日志导出的行为用例。
//!
//! 缓冲区是进程级全局状态，故每条用例都用自己独有的 `target` 认自己的记录，不断言整份
//! 文本的总长度。

use simple_post_core::logbuf::{self, LogLevel};

use crate::log_api::{append_client_event, export, CLIENT_FIELD_LIMIT_CHARS};

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
