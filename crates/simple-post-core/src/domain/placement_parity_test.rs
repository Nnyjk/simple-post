//! 父子类型矩阵的一致性样本：Rust 的 `core::domain::ResourceNodeType::may_under`。
//!
//! 同一份样本也被 TS 侧的 `src/lib/__tests__/placement-parity.test.ts` 读。两份实现各写
//! 一遍矩阵（后端在每一次写入时执行它，前端在拖放判定与乐观更新时读它），只有一份共享
//! 样本才能钉住它们没分叉。

use serde::Deserialize;

use crate::domain::ResourceNodeType;

/// 共享样本。放在仓库根的 `fixtures/` 下，TS 侧读同一份文件。
const FIXTURE: &str = include_str!("../../../../fixtures/placement-parity.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    matrix: Vec<Cell>,
    project_top_level: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cell {
    child: String,
    parent: String,
    allowed: bool,
}

/// 样本里出现的类型名。类型名写错时 serde 会把整条 `Cell` 判不出来而报错，不会静默跳过。
fn parse(kind: &str) -> ResourceNodeType {
    ResourceNodeType::parse(kind).unwrap_or_else(|| panic!("样本里的类型名不合法：{kind}"))
}

#[test]
fn every_placement_cell_matches_the_rust_matrix() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("placement parity fixture");
    assert!(
        !fixture.matrix.is_empty(),
        "样本里必须有格子，否则这条测试是空的"
    );

    for cell in &fixture.matrix {
        let child = parse(&cell.child);
        let parent = parse(&cell.parent);
        assert_eq!(
            child.may_under(parent),
            cell.allowed,
            "{} 能否挂在 {} 下",
            cell.child,
            cell.parent
        );
    }
}

#[test]
fn the_project_top_level_takes_modules_only() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("placement parity fixture");
    for kind in ["module", "collection", "endpoint"] {
        let expected = fixture.project_top_level.iter().any(|k| k == kind);
        assert_eq!(
            parse(kind).may_sit_at_project_top(),
            expected,
            "{kind} 能否落在项目顶层"
        );
    }
}

/// 样本必须恰好覆盖 3 种子类型 × 3 种父类型这九格。
///
/// 少一格时上面那条测试照样绿 —— 它只检查样本里写了的格子。故这里数一遍：某一格出现
/// 零次或两次，都说明样本本身没把矩阵写全。
#[test]
fn the_fixture_covers_every_child_parent_pair_exactly_once() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("placement parity fixture");
    for child in ["module", "collection", "endpoint"] {
        for parent in ["module", "collection", "endpoint"] {
            let count = fixture
                .matrix
                .iter()
                .filter(|c| c.child == child && c.parent == parent)
                .count();
            assert_eq!(
                count, 1,
                "样本必须恰好覆盖一次「{child} 挂在 {parent} 下」这一格"
            );
        }
    }
    assert_eq!(
        fixture.matrix.len(),
        9,
        "矩阵是 3 种子类型 × 3 种父类型 = 9 格"
    );
}
