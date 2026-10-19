//! 指纹去重：重复落库不产生副本，改名不改指纹，同一份 IR 里的重复只落一次。

use simple_post_core::db;

use super::support::{commit_all, fresh_project, nested_exchange};

#[test]
fn committing_the_same_ir_twice_creates_no_duplicate() {
    let (pool, project_id) = fresh_project();
    let ex = nested_exchange();

    let first = commit_all(&pool, &project_id, &ex).expect("first");
    assert_eq!(first.created_endpoint_ids.len(), 3);

    let second = commit_all(&pool, &project_id, &ex).expect("second");
    assert!(
        second.created_endpoint_ids.is_empty(),
        "second pass created {:?}",
        second.created_endpoint_ids
    );
    assert_eq!(second.skipped_duplicates.len(), 3);
    // 跳过要有出处：一条 Info 说清按指纹跳过，且没有更严重的诊断。
    assert_eq!(second.notable_diagnostics().len(), 0);
    assert!(second
        .diagnostics
        .iter()
        .any(|d| d.severity == crate::Severity::Info));

    let count = db::with_conn(&pool, |c| {
        db::endpoint_details::list_by_root(c, &project_id)
    })
    .expect("list")
    .len();
    assert_eq!(count, 3);
}

#[test]
fn a_renamed_endpoint_is_still_recognised_as_the_same_one() {
    let (pool, project_id) = fresh_project();
    commit_all(&pool, &project_id, &nested_exchange()).expect("first");

    let mut renamed = nested_exchange();
    renamed.modules[0].endpoints[0].name = "改了名字".into();
    renamed.modules[0].endpoints[0].description = Some("补了描述".into());
    let second = commit_all(&pool, &project_id, &renamed).expect("second");

    // 三个接口一个都没变（名字与描述不进指纹），故全部被认出来。
    assert!(
        second.created_endpoint_ids.is_empty(),
        "改名不该造出副本, created = {:?}",
        second.created_endpoint_ids
    );
    assert_eq!(second.skipped_duplicates.len(), 3);
    let count = db::with_conn(&pool, |c| {
        db::endpoint_details::list_by_root(c, &project_id)
    })
    .expect("list")
    .len();
    assert_eq!(count, 3);
}

#[test]
fn a_duplicate_inside_one_ir_lands_once() {
    let (pool, project_id) = fresh_project();
    let mut ex = nested_exchange();
    // 同一个 IR 节点被复制了一份——key 也一样。真实外来文件不会这样（key 是源
    // 格式里的位置指针），故这条测的是「解析器给出重复 key」时失败形态是什么：
    // 落一份，不是落两份。
    let dup = ex.modules[0].endpoints[0].clone();
    ex.modules[0].endpoints.push(dup);

    let report = commit_all(&pool, &project_id, &ex).expect("commit");
    assert_eq!(report.created_endpoint_ids.len(), 3, "重复项第二次被跳过");
    assert_eq!(report.skipped_duplicates, vec!["直挂的接口"]);
}
