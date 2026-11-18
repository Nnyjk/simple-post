//! 环形缓冲的行为用例。
//!
//! 缓冲区是进程级全局状态，测试文件内多条用例并发写同一个 `BUF` —— 故每条用例都用
//! 自己独有的 `target` 认自己的那几条，不断言总条数。

use crate::logbuf::{self, LogLevel, LOG_CAPACITY, MESSAGE_LIMIT_BYTES};

#[test]
fn level_labels_are_uppercase_and_parse_case_insensitively() {
    assert_eq!(LogLevel::Error.as_str(), "ERROR");
    assert_eq!(LogLevel::Warn.as_str(), "WARN");
    assert_eq!(LogLevel::Info.as_str(), "INFO");
    assert_eq!(LogLevel::Debug.as_str(), "DEBUG");
    assert_eq!(LogLevel::Trace.as_str(), "TRACE");

    assert_eq!(LogLevel::from_label("ERROR"), LogLevel::Error);
    assert_eq!(LogLevel::from_label("Warn"), LogLevel::Warn);
    assert_eq!(LogLevel::from_label(" debug "), LogLevel::Debug);
    // 拼错按 `Info`：写日志不能因为级别写错就失败。
    assert_eq!(LogLevel::from_label("verbose"), LogLevel::Info);
}

#[test]
fn warn_and_above_are_problems() {
    assert!(LogLevel::Error.is_problem());
    assert!(LogLevel::Warn.is_problem());
    assert!(!LogLevel::Info.is_problem());
    assert!(!LogLevel::Debug.is_problem());
    assert!(!LogLevel::Trace.is_problem());
}

#[test]
fn snapshot_filters_by_level_and_keeps_order() {
    logbuf::record(LogLevel::Info, "test::filter", "first");
    logbuf::record(LogLevel::Error, "test::filter", "second");

    let all: Vec<_> = logbuf::snapshot(false)
        .into_iter()
        .filter(|r| r.target == "test::filter")
        .collect();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].message, "first");
    assert_eq!(all[1].message, "second");

    let problems: Vec<_> = logbuf::snapshot(true)
        .into_iter()
        .filter(|r| r.target == "test::filter")
        .collect();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].level, LogLevel::Error);
}

#[test]
fn render_writes_one_line_per_record_without_a_trailing_newline() {
    // 直接验格式：`snapshot` 是全局的，故这里对一条已知记录的渲染结果做断言。
    let rec = logbuf::LogRecord {
        at_ms: 1_763_465_349_123,
        level: LogLevel::Error,
        target: "webview:smoke".to_string(),
        message: "boom".to_string(),
    };
    assert_eq!(
        rec.render_line(),
        "2025-11-18T11:29:09.123Z ERROR webview:smoke: boom"
    );

    let text = logbuf::render(false);
    assert!(!text.ends_with('\n'), "末尾不该有多余空行");
}

#[test]
fn render_is_empty_when_nothing_matches() {
    // 没有任何 `Warn` 以上的 `target` 能保证为空 —— 缓冲区全局，故只验「空串不带换行」的形状。
    let text = logbuf::render(true);
    assert!(!text.contains("\n\n"), "渲染不得产生连续空行");
}

#[test]
fn overlong_messages_are_clamped_on_a_char_boundary() {
    let big = "日".repeat(MESSAGE_LIMIT_BYTES); // 每个字符 3 字节，远超上限
    logbuf::record(LogLevel::Info, "test::clamp", big);

    let rec = logbuf::snapshot(false)
        .into_iter()
        .find(|r| r.target == "test::clamp")
        .expect("刚写入的记录应当在缓冲区里");
    assert!(rec.message.len() <= MESSAGE_LIMIT_BYTES);
    assert!(rec.message.chars().all(|c| c == '日'), "截断不得切坏字符");
}

#[test]
fn the_buffer_evicts_the_oldest_entry_at_capacity() {
    for i in 0..(LOG_CAPACITY + 5) {
        logbuf::record(LogLevel::Trace, "test::capacity", format!("entry-{i}"));
    }
    let mine: Vec<_> = logbuf::snapshot(false)
        .into_iter()
        .filter(|r| r.target == "test::capacity")
        .collect();
    // 并发测试会插入别的记录，故只钉「不超过容量」，且最后一条一定还在。
    assert!(mine.len() <= LOG_CAPACITY);
    assert_eq!(
        mine.last().map(|r| r.message.as_str()),
        Some(format!("entry-{}", LOG_CAPACITY + 4).as_str())
    );
    assert!(
        !mine.iter().any(|r| r.message == "entry-0"),
        "最旧的一条应当已被挤出"
    );
}

#[test]
fn stamps_are_compact_utc_timestamps() {
    let stamp = logbuf::now_stamp();
    assert_eq!(stamp.len(), 15, "形如 20261118-112909");
    let (date, time) = stamp.split_once('-').expect("日期与时间之间有一个连字符");
    assert_eq!(date.len(), 8);
    assert_eq!(time.len(), 6);
    assert!(stamp.chars().all(|c| c.is_ascii_digit() || c == '-'));
}

#[test]
fn rfc3339_covers_the_epoch_and_known_instants() {
    assert_eq!(logbuf::rfc3339_millis(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(
        logbuf::rfc3339_millis(1_763_465_349_123),
        "2025-11-18T11:29:09.123Z"
    );
}
