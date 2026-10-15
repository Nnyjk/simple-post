// 触发器。应用层才是主要的检查；这些测试证明一次绕过它的写入仍造不出走不动的树。

use super::nodes_fixtures::*;
use super::*;
use crate::db;

#[test]
fn a_trigger_rejects_an_endpoint_under_an_endpoint() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    // endpoint 是叶子。它被拓宽的父集合是 module / collection，绝不是另一个 endpoint——
    // 而对一个裸写入者来说，守住这条线的是触发器而不是应用层。
    db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e_raw', 'p1', 'm1', 'endpoint', 'raw', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .expect("an endpoint under a module is legal");
    let err = db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('e_child', 'p1', 'e_raw', 'endpoint', 'raw', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("parent type does not accept"),
        "the trigger must be what stops this, got {msg:?}"
    );
}

#[test]
fn a_trigger_rejects_a_module_with_a_node_parent() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    let err = db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('m_raw', 'p1', 'm1', 'module', 'raw', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        err.to_string().contains("a module has no node parent"),
        "got {:?}",
        err.to_string()
    );
}

#[test]
fn a_trigger_rejects_an_illegal_reparent() {
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    let err = db::with_tx(&pool, |c| {
        c.execute("UPDATE nodes SET parent_id = 'e1' WHERE id = 'c1'", [])?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        err.to_string().contains("parent type does not accept"),
        "got {:?}",
        err.to_string()
    );
}

#[test]
fn the_trigger_allows_what_the_matrix_allows() {
    // 一个对一切都开火的触发器比没有更糟：它把调用方推向关掉外键。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    put(&pool, &collection("p1", "c2", "c1", 0), None);

    db::with_tx(&pool, |c| {
        c.execute("UPDATE nodes SET parent_id = 'c1' WHERE id = 'c2'", [])?;
        Ok(())
    })
    .expect("collection under a collection is legal");
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "c2"))
            .unwrap()
            .parent_id
            .as_deref(),
        Some("c1")
    );
}

#[test]
fn the_trigger_allows_an_endpoint_under_a_module() {
    // update 触发器也带着同一份矩阵，且与它一起被拓宽了。留在旧谓词上，它会中止应用层刚刚
    // 认可的那次改父——而 `module_move` / `endpoint_move` 直穿它。
    let (pool, _dir) = fresh_pool();
    put(&pool, &module("p1", "m1", 0), None);
    put(&pool, &collection("p1", "c1", "m1", 0), None);
    let (e, d) = endpoint("p1", "e1", "c1", 0);
    put(&pool, &e, Some(&d));

    // 把它从 collection 里抬出来，走裸 SQL。
    db::with_tx(&pool, |c| {
        c.execute("UPDATE nodes SET parent_id = 'm1' WHERE id = 'e1'", [])?;
        Ok(())
    })
    .expect("reparenting an endpoint to a module is legal");
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .parent_id
            .as_deref(),
        Some("m1")
    );
}

#[test]
fn the_check_constraint_holds_the_type_set_closed() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
             (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
             VALUES ('x', 'p1', NULL, 'folder', 'x', 0, 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("check"),
        "the CHECK constraint should reject an unknown type, got {:?}",
        err.to_string()
    );
}
