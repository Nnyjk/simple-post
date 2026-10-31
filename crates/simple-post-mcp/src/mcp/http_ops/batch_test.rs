//! `api_run_collection` 的报告算术 —— 三类互斥且穷尽，故总数不会算错。
//!
//! 这组断言盯的是报告的恒等式：总数 = 状态码分布之和 + 没拿到状态码 + 压根没发出去
//! + 未执行。分类逻辑住在 `batch.rs` 的 `Counts::add`，本文件只喂结论、验算术。

use super::batch::Counts;

/// 一条结论的三要素：成功与否、状态码、失败分类。
///
/// `failure` 的取值与 `batch.rs` 里一致：`blocked`（压根没发出去）、`http`
/// （拿到了响应但不是 2xx）、`transport`（发出去了但没拿到响应）。
fn counts_of(outcomes: &[(bool, u16, Option<&str>)]) -> Counts {
    let mut counts = Counts::default();
    for (success, status, failure) in outcomes {
        counts.add(*success, *status, *failure);
    }
    counts
}

#[test]
fn three_classes_are_mutually_exclusive_and_exhaustive() {
    let outcomes = [
        (true, 200, None),
        (true, 201, None),
        (false, 404, Some("http")),
        (false, 500, Some("http")),
        (false, 0, Some("transport")),
        (false, 0, Some("blocked")),
    ];
    let counts = counts_of(&outcomes);

    // 三类之和恒等于结论条数 —— 少算一条就会与总数对不上。
    assert_eq!(counts.classified(), outcomes.len());
    assert_eq!(counts.success, 2);
    assert_eq!(counts.failed, 4);
    assert_eq!(counts.blocked, 1);
    assert_eq!(counts.no_response, 1);
    assert_eq!(counts.status_counts.get(&200), Some(&1));
    assert_eq!(counts.status_counts.get(&201), Some(&1));
    assert_eq!(counts.status_counts.get(&404), Some(&1));
    assert_eq!(counts.status_counts.get(&500), Some(&1));
}

#[test]
fn the_report_identity_holds() {
    let outcomes = [
        (true, 200, None),
        (false, 500, Some("http")),
        (false, 0, Some("transport")),
        (false, 0, Some("blocked")),
    ];
    let counts = counts_of(&outcomes);
    let total = outcomes.len();

    // MCP 的调用是同步的，没有中止协议，故每一条都轮到过，`not_run` 恒为 0。
    let not_run = 0;
    assert_eq!(
        total,
        counts.status_counts.values().sum::<usize>()
            + counts.no_response
            + counts.blocked
            + not_run
    );
}

#[test]
fn blocked_is_not_counted_as_no_response() {
    // 「压根没发出去」与「没拿到响应」是两回事：前者是这条没准备好，后者是发出去了
    // 但对方没应答。两者都没有状态码，故必须靠判定的先后把它们分开。
    let counts = counts_of(&[(false, 0, Some("blocked"))]);
    assert_eq!(counts.blocked, 1);
    assert_eq!(counts.no_response, 0);
    assert!(counts.status_counts.is_empty());
}

#[test]
fn a_status_code_never_lands_in_no_response() {
    let counts = counts_of(&[(false, 404, Some("http"))]);
    assert_eq!(counts.no_response, 0);
    assert_eq!(counts.status_counts.get(&404), Some(&1));
}

#[test]
fn an_empty_batch_counts_nothing() {
    let counts = counts_of(&[]);
    assert_eq!(counts.classified(), 0);
    assert_eq!(counts.success, 0);
    assert_eq!(counts.failed, 0);
}
