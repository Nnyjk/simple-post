// 数据迁移——两种前任服务表形状。

use rusqlite::Connection;

use super::migration_fixtures::*;
use super::test_helpers::fresh_pool;
use super::*;

// shape A——项目持有的服务表。

#[test]
fn shape_a_urls_are_byte_identical_after_migration() {
    let pool = legacy_store("shape-a-urls");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        let legacy = [
            "a_named_air",
            "a_named_water",
            "a_star_air",
            "a_star_water",
            "a_unset",
        ]
        .iter()
        .map(|id| {
            (
                id.to_string(),
                resolve_legacy_project_owned(c, "p_a", id, "e_a_prod"),
            )
        })
        .collect::<Vec<_>>();
        let before = urls_under_current(c, "p_a", "e_a_prod");
        migrate(c)?;
        let after = urls_under_current(c, "p_a", "e_a_prod");
        assert_eq!(
            before.len(),
            legacy.len(),
            "the same endpoints must exist on both sides of the migration"
        );
        assert_eq!(
            after, legacy,
            "every endpoint must resolve to exactly the URL it did before"
        );
        // 抽查那两个有意思的单元格，而不是相信上面的比较真的碰到了它们。
        assert_eq!(
            after[0].1, "http://127.0.0.1:9201",
            "override wins for m_air"
        );
        assert_eq!(
            after[1].1, "http://127.0.0.1:9200",
            "m_water has no override, so it uses the environment value"
        );
        assert_eq!(
            after[4].1, "",
            "a service with no value resolves to nothing"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_a_puts_every_value_in_the_module_keyed_layer() {
    let pool = legacy_store("shape-a-cells");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        migrate(c)?;
        let (_, cells) = read_env_maps(c, "e_a_prod");
        let per_module = cells.get("d_base").expect("d_base must have cells");
        assert_eq!(per_module.get("m_air").unwrap(), "http://127.0.0.1:9201");
        assert_eq!(per_module.get("m_water").unwrap(), "http://127.0.0.1:9200");
        assert!(
            !cells.contains_key("d_air"),
            "a service with no value anywhere gets no cells at all"
        );
        assert_eq!(
            project_star(c, "p_a").as_deref(),
            Some("d_base"),
            "the project ★ is already the answer shape A was using"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_a_leaves_the_environment_level_urls_in_place() {
    // 契约 §4.6：shape A 的 `base_urls_json` 保持原样。那是这一布局中留它安全的唯一情形，
    // 因为重跑迁移会从它重新推导出完全相同的值。
    let pool = legacy_store("shape-a-keeps-base-urls");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        migrate(c)?;
        let (base_urls, _) = read_env_maps(c, "e_a_prod");
        assert_eq!(base_urls.get("d_base").unwrap(), "http://127.0.0.1:9200");
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_a_star_stays_empty_when_the_project_had_none() {
    // 在这里选出一个 ★ 会给原本解析为 `''` 的 endpoint 一个它从未有过的 host，
    // 故 NULL 默认在迁移后仍是 NULL。
    let pool = legacy_store("shape-a-null-star");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        c.execute(
            "UPDATE projects SET default_base_url_definition_id = NULL WHERE id = 'p_a'",
            [],
        )
        .unwrap();
        migrate(c)?;
        assert_eq!(project_star(c, "p_a"), None);
        Ok(())
    })
    .unwrap();
}

// shape B——模块持有的服务列表。

#[test]
fn shape_b_urls_are_byte_identical_after_migration() {
    let pool = legacy_store("shape-b-urls");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        let legacy = ["b_named_air", "b_named_water", "b_unset"]
            .iter()
            .map(|id| {
                (
                    id.to_string(),
                    resolve_legacy_module_owned(c, id, "e_b_prod"),
                )
            })
            .collect::<Vec<_>>();
        migrate(c)?;
        let after = urls_under_current(c, "p_b", "e_b_prod");
        assert_eq!(
            after, legacy,
            "every endpoint must resolve to exactly the URL it did before"
        );
        assert_eq!(after[0].1, "http://127.0.0.1:9200");
        assert_eq!(
            after[1].1, "http://127.0.0.1:9300",
            "m_water's own clone carried the URL and must not be lost"
        );
        assert_eq!(after[2].1, "", "a clone with no URL resolves to nothing");
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_collapses_each_service_name_to_one_project_level_row() {
    let pool = legacy_store("shape-b-collapse");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        let names: Vec<(String, Option<String>)> = {
            let mut stmt = c
                .prepare(
                    "SELECT name, module_id FROM base_url_definitions \
                     WHERE project_id = 'p_b' ORDER BY sort_order ASC, id ASC",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(
            names.len(),
            3,
            "「新服务」 existed under both modules and must survive once"
        );
        for (name, module) in &names {
            assert!(module.is_none(), "{name} still owns a module: {module:?}");
        }
        // air 是第一个模块，故它的行是存活者。
        assert_eq!(names[1].0, "base");
        let (_, cells) = read_env_maps(c, "e_b_prod");
        assert_eq!(
            cells.get("sp_air_base").unwrap().get("m_air").unwrap(),
            "http://127.0.0.1:9200"
        );
        assert_eq!(
            cells.get("sp_air_new").unwrap().get("m_water").unwrap(),
            "http://127.0.0.1:9300",
            "water's value lands on the survivor's row, under water"
        );
        assert!(
            !cells.contains_key("sp_water_new"),
            "the collapsed clone's id must not survive as a key"
        );
        assert!(
            !cells.get("sp_air_base").unwrap().contains_key("m_water"),
            "an empty cell stays empty: water never had a `base` service"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_conflicting_stars_resolve_to_the_first_module() {
    // air 给「base」打 ★ 而 water 给「新服务」打 ★。模块之间没有继承，故哪个都不是「那个」
    // 答案；决胜规则就是第一个模块，这是明示的而非推导出的。
    let pool = legacy_store("shape-b-star-conflict");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        assert_eq!(
            project_star(c, "p_b").as_deref(),
            Some("sp_air_base"),
            "the first module's ★ wins, by module display order"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_star_falls_back_to_the_first_service_when_the_first_module_has_none() {
    let pool = legacy_store("shape-b-star-fallback");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        c.execute("UPDATE base_url_definitions SET is_default = 0", [])
            .unwrap();
        migrate(c)?;
        assert_eq!(
            project_star(c, "p_b").as_deref(),
            Some("sp_air_air"),
            "with no ★ anywhere, the first service in display order takes it"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_neutralises_the_columns_it_retired() {
    let pool = legacy_store("shape-b-neutralise");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        let (owned, starred): (i64, i64) = c
            .query_row(
                "SELECT COUNT(*) FILTER (WHERE module_id IS NOT NULL), \
                        COUNT(*) FILTER (WHERE is_default != 0) \
                 FROM base_url_definitions WHERE project_id = 'p_b'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(owned, 0, "every row must give up its module");
        assert_eq!(starred, 0, "every row must give up its per-module ★");
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_clears_the_urls_it_moved() {
    // `base_urls_json` 里的每个值现在都在以模块为键的那一层，故留着它会让下次启动把它读成
    // shape A，并把 air 的 host 复制进每个模块的空单元格。
    let pool = legacy_store("shape-b-clears-base-urls");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        let (base_urls, _) = read_env_maps(c, "e_b_prod");
        assert!(
            base_urls.is_empty(),
            "the dead column must not keep answering the next boot: {base_urls:?}"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_remaps_endpoints_onto_the_surviving_rows() {
    let pool = legacy_store("shape-b-remap");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        assert_eq!(
            endpoint_service(c, "b_named_air").as_deref(),
            Some("sp_air_base"),
            "the survivor of air's own row is that row"
        );
        assert_eq!(
            endpoint_service(c, "b_named_water").as_deref(),
            Some("sp_air_new"),
            "water's 「新服务」 collapsed onto air's row of the same name"
        );
        let dangling: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM endpoint_details e \
                 LEFT JOIN base_url_definitions d ON d.id = e.base_url_definition_id \
                 WHERE e.base_url_definition_id IS NOT NULL AND d.id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dangling, 0, "no endpoint may point at a deleted clone");
        Ok(())
    })
    .unwrap();
}

// 幂等性。

#[test]
fn migrating_twice_changes_nothing_in_either_shape() {
    // (临时目录 tag, 项目 id, fixture 构造函数)
    type Shape = (&'static str, &'static str, fn(&mut Connection));
    let shapes: [Shape; 2] = [
        ("twice-a", "p_a", seed_shape_a),
        ("twice-b", "p_b", seed_shape_b),
    ];
    for (tag, project_id, seed) in shapes {
        let pool = legacy_store(tag);
        with_conn(&pool, |c| {
            seed(c);
            migrate(c)?;
            let after_first = snapshot(c, project_id);
            assert!(
                !after_first.is_empty(),
                "{tag}: fixture rows are missing, the comparison below would pass vacuously"
            );
            migrate(c)?;
            assert_eq!(
                snapshot(c, project_id),
                after_first,
                "{tag}: a second migrate must be a no-op"
            );
            // 再来第三次，为的是那个守卫必须注意到库已被修好的形状。
            migrate(c)?;
            assert_eq!(snapshot(c, project_id), after_first, "{tag}: third run");
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn a_current_store_is_left_alone() {
    // seed 出来的 fixture 已经是目标形状，故对它们跑迁移不得碰到任何一行——这正是首轮之后
    // 每次启动都零成本的原因。
    let (pool, _dir) = fresh_pool();
    with_conn(&pool, |c| {
        let before = snapshot(c, "p_demo");
        migrate(c)?;
        assert_eq!(snapshot(c, "p_demo"), before);
        Ok(())
    })
    .unwrap();
}
