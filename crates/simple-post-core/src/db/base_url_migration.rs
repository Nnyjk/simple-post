// 数据迁移：服务表回到项目级，URL 落到 (环境 × 模块 × 服务)。
//
// 这是数据修复而非版本变更：`SCHEMA_VERSION` 刻意不动（bump 会让
// `ensure_current_schema` 删库），列由 `ADDED_COLUMNS` 补齐，本 pass 修复旧布局写的行。
//
// **两种旧布局都在野外的库里存在**，按证据逐项目判定，绝不假定：
// - shape B：每个 (模块 × name) 一行，值在 `base_urls_json[clone_id]`，★ 按模块记在
//   `is_default`。
// - shape A：项目级服务表，`module_id` 为 NULL，值在 `base_urls_json[def_id]`，
//   外加 `module_base_urls_json[def_id][module_id]` 的按模块覆盖。
// `module_id` 非 NULL 只可能是 B；无此行但 `base_urls_json` 有值则是 A。
// 本构建写的库两者皆无，pass 即空操作。

use rusqlite::{params, Connection};

use crate::error::AppError;

use super::json_col_err;

/// 旧布局写下的一条服务行。`module_id` 是布局判别式：`Some` = 模块持有，`None` = 项目持有。
struct ServiceRow {
    id: String,
    name: String,
    module_id: Option<String>,
    sort_order: i64,
    /// 模块持有模型按行记的 ★。shape A 无此标记，故此处恒为 false。
    is_default: bool,
}

/// 项目的一个模块，即「第一个模块」的含义：显示序 → 创建时间 → id，
/// 让选择跨次运行稳定，而不是取决于扫描顺序。
struct ModuleRow {
    id: String,
    sort_order: i64,
    created_at: i64,
}

impl ModuleRow {
    /// 显示序，写作 `(sort_order, created_at, id)`。
    fn rank(&self) -> (i64, i64, &str) {
        (self.sort_order, self.created_at, self.id.as_str())
    }
}

/// 两层 URL 都可读的环境行。本 crate 仅剩这一个 `base_urls_json` 的读者，且必须同时读
/// `module_base_urls_json`——shape A 的覆盖层是答案的一部分。
struct EnvRow {
    id: String,
    /// `def_id -> url`。shape A 的值缺失时从这一列回退读取，shape B 的值由这一列迁移而来；
    /// 它只在 shape B 走完后被清空一次（见 [`migrate_project`]）。
    base_urls: std::collections::BTreeMap<String, String>,
    /// `def_id -> module_id -> url`，现役值层。
    module_base_urls:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

/// 本 pass 仍需处理的项目 id。两类独立证据取并集：仍有服务指名模块 ⇒ shape B；
/// 仍有服务级 URL 的环境 ⇒ shape A。两者皆无即已是现役形状，严格不碰——
/// 这也是首轮之后每次启动都零成本的原因。
fn pending_projects(conn: &mut Connection) -> Result<Vec<String>, AppError> {
    let mut pending: std::collections::BTreeSet<String> = Default::default();

    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT project_id FROM base_url_definitions \
             WHERE module_id IS NOT NULL",
        )?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<String>, _>>()?;
        pending.extend(ids);
    }

    for env in read_envs_needing_repair(conn)? {
        pending.insert(env);
    }

    Ok(pending.into_iter().collect())
}

/// `base_urls_json` 仍有内容的环境所属项目 id。
/// 解析而非与 `'{}'` 比较：仅空白或键序不同的 blob 也算证据。
fn read_envs_needing_repair(conn: &mut Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT project_id, base_urls_json FROM environments \
         WHERE base_urls_json IS NOT NULL AND base_urls_json <> '{}'",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (project_id, raw) = r?;
        match serde_json::from_str::<std::collections::BTreeMap<String, String>>(&raw) {
            Ok(map) if !map.is_empty() => out.push(project_id),
            Ok(_) => {}
            // 解析不了就没东西可搬：否则会为一个本构建已当死列的字段拒绝启动。
            Err(e) => tracing::warn!(
                "environments.base_urls_json is not readable ({e}); \
                 migrating this environment without its service-level URLs"
            ),
        }
    }
    Ok(out)
}

/// 把每个项目的服务搬回项目级，URL 搬进 `module_base_urls_json[def_id][module_id]`。
/// 所有项目共用一个事务：中途崩溃则整遍回滚，下次启动从同一初始状态重来，
/// 不存在半迁移形状。
pub(super) fn migrate_base_urls_into_value_layer(conn: &mut Connection) -> Result<(), AppError> {
    let pending = pending_projects(conn)?;
    if pending.is_empty() {
        return Ok(());
    }

    // 手写 `BEGIN IMMEDIATE` / `COMMIT` 而非 `conn.transaction()`，理由同
    // `run_immediate_tx`：`rusqlite::Transaction` 只把不可变的连接借用交回来，
    // 而下面每个 helper 都要本模块通用的 `&mut Connection`。用 IMMEDIATE 而非默认的
    // deferred，是让并发启动在 BEGIN 处就失败，而不是改写改到一半才失败。
    conn.execute_batch("BEGIN IMMEDIATE")?;
    for project_id in &pending {
        if let Err(e) = migrate_project(conn, project_id) {
            // 尽力回滚——原始错误才有诊断价值。
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }
    conn.execute_batch("COMMIT")?;
    tracing::info!(
        "schema: moved {} project service list(s) back onto the project \
         and their URLs into module_base_urls",
        pending.len()
    );
    Ok(())
}

/// [`migrate_base_urls_into_value_layer`] 的逐项目一半。
fn migrate_project(conn: &mut Connection, project_id: &str) -> Result<(), AppError> {
    let modules = read_modules(conn, project_id)?;
    let services = read_services(conn, project_id)?;
    if services.is_empty() {
        return Ok(());
    }
    let envs = read_envs(conn, project_id)?;

    // shape B = 「还有行持有模块」。逐项目判定，才让从未被模块持有版碰过的库走保 id 的
    // 廉价路径；且这是唯一可用证据——两种布局的 `base_urls_json` 都是有值的。
    let module_owned = services.iter().any(|s| s.module_id.is_some());

    // 在任何改写之前读出项目自己的 ★：shape A 保留它（endpoint 本就按它解析），
    // shape B 已无处可留（那个模型把 ★ 移到了模块上）。
    let star_before: Option<String> = conn.query_row(
        "SELECT default_base_url_definition_id FROM projects WHERE id = ?1",
        params![project_id],
        |r| r.get(0),
    )?;

    let canonical = pick_canonical(&services, &modules, module_owned);
    // clone id -> 存活的 canonical id，仅 shape B：指名 clone 的 endpoint 要改指到留下的那一行。
    let survivor_of: std::collections::HashMap<&str, &str> = canonical
        .iter()
        .filter(|c| !c.replaced.is_empty())
        .flat_map(|c| {
            c.replaced
                .iter()
                .map(move |old| (old.as_str(), c.id.as_str()))
        })
        .collect();

    write_value_layer(conn, &envs, &modules, &canonical, &services, module_owned)?;

    if module_owned {
        remap_endpoints_onto_canonical(conn, project_id, &survivor_of)?;
    }

    // ★：shape A 保留原指向（NULL 就保持 NULL——凭空造一个默认，等于给原本解析为 `''`
    // 的 endpoint 一个它从未有过的 host）；shape B 必须选，因为按模块的 ★ 即将消失，
    // 见 [`choose_star_for_module_owned`]。
    let star = if module_owned {
        choose_star_for_module_owned(&services, &modules, &canonical)
    } else {
        star_before.filter(|d| canonical.iter().any(|c| c.id == *d))
    };
    conn.execute(
        "UPDATE projects SET default_base_url_definition_id = ?1 WHERE id = ?2",
        params![star, project_id],
    )?;

    // 先写值层再退场 clone：读的时候那些 id 还得可解析。
    for c in &canonical {
        for old in &c.replaced {
            conn.execute(
                "DELETE FROM base_url_definitions WHERE id = ?1",
                params![old],
            )?;
        }
    }
    conn.execute(
        "UPDATE base_url_definitions SET module_id = NULL, is_default = 0 \
         WHERE project_id = ?1",
        params![project_id],
    )?;

    // 仅 shape B：`base_urls_json` 就是值层，其中每个值都已进 `module_base_urls_json`
    // （各 clone 的 URL 归到它自己的模块，剩余键指向已不存在的行）。留着不只是脏：
    // 下次启动看到没有 `module_id` 的库会判定为 shape A，于是把 `base_urls[canonical]`
    // 展开进每个模块的空单元格——模块 B 被填上模块 A 的 host，且每次启动都重来。
    // 清空它才使这一形状幂等。shape A 不动：它的键就是自己的 def id，之后每遍都重新
    // 推导出逐字节相同的值，是唯一残留 blob 真正无害的布局。
    if module_owned {
        conn.execute(
            "UPDATE environments SET base_urls_json = '{}' WHERE project_id = ?1",
            params![project_id],
        )?;
    }
    Ok(())
}

/// 迁移后存活的服务行，加上它顶替的那些 id。
struct CanonicalService {
    id: String,
    name: String,
    sort_order: i64,
    /// 被本行顶替的 per-module clone id（仅 shape B）。为空表示没有折叠，
    /// shape A 每行皆如此。
    replaced: Vec<String>,
}

/// 决定每个服务名由哪一行代表。
///
/// - shape A：每行已是项目级，全部存活、无需合并，故重命名不是合并——同 `name` 两行
///   是应用层自己造不出的状态。
/// - shape B：每 (模块 × name) 一行，须收敛为每 name 一行。存活者取**第一个**模块下的
///   那行（显示序 → 创建时间 → id）：那是项目配置最多的模块，换一个就得有一条
///   「哪个模块赢」的规则，而数据里没有依据。同模块内以该行自身显示序决胜。
/// - 无模块的行排在所有真模块之后，只在该 name 仅剩它一行时胜出——混合库里模块持有版
///   在旧行折叠前新建服务会留下这种状态。
fn pick_canonical(
    services: &[ServiceRow],
    modules: &[ModuleRow],
    module_owned: bool,
) -> Vec<CanonicalService> {
    if !module_owned {
        return services
            .iter()
            .map(|s| CanonicalService {
                id: s.id.clone(),
                name: s.name.clone(),
                sort_order: s.sort_order,
                replaced: Vec::new(),
            })
            .collect();
    }

    let rank_of = |module_id: Option<&str>| -> (u8, (i64, i64, &str)) {
        match module_id.and_then(|m| modules.iter().find(|x| x.id == m)) {
            // 有模块的行排在无主残留之前。
            Some(m) => (0, m.rank()),
            None => (1, (0, 0, "")),
        }
    };

    let mut by_name: std::collections::BTreeMap<&str, &ServiceRow> = Default::default();
    for s in services {
        let entry = by_name.entry(s.name.as_str()).or_insert(s);
        let better = rank_of(s.module_id.as_deref()) < rank_of(entry.module_id.as_deref())
            // 同模块（或都无主）时退回该行自身的显示序，使选择不依赖扫描顺序。
            || (rank_of(s.module_id.as_deref()) == rank_of(entry.module_id.as_deref())
                && (s.sort_order, s.id.as_str()) < (entry.sort_order, entry.id.as_str()));
        if better {
            *entry = s;
        }
    }

    by_name
        .values()
        .map(|keep| CanonicalService {
            id: keep.id.clone(),
            name: keep.name.clone(),
            sort_order: keep.sort_order,
            replaced: services
                .iter()
                .filter(|s| s.name == keep.name && s.id != keep.id)
                .map(|s| s.id.clone())
                .collect(),
        })
        .collect()
}

/// 模块持有模型下，谁成为项目的 ★。
///
/// 取第一个模块的 ★——最可能就是用户本意，它的请求正是从这里发出的。**两模块冲突时没有
/// 正确答案**：该模型给每个模块独立默认值，从未把两者关联，既无继承可循，也无多数可问；
/// 「第一个模块胜出」是明示的决胜规则，不是还原本意。另一个选择（不设 ★）会静默剥掉每个
/// 未指名服务的 endpoint 的 base URL。首个模块自己无 ★ 时，退回 `sort_order` 最小的服务，
/// 即选择器里排在最前的那个。
fn choose_star_for_module_owned(
    services: &[ServiceRow],
    modules: &[ModuleRow],
    canonical: &[CanonicalService],
) -> Option<String> {
    let first_module = modules.iter().min_by(|a, b| a.rank().cmp(&b.rank()));
    if let Some(first) = first_module {
        let stars: Vec<&ServiceRow> = services
            .iter()
            .filter(|s| s.is_default && s.module_id.as_deref() == Some(first.id.as_str()))
            .collect();
        if let Some(pick) = stars
            .iter()
            .min_by(|a, b| (a.sort_order, &a.id).cmp(&(b.sort_order, &b.id)))
        {
            // ★ 自身可能就是 clone，此时项目默认须指名存活的行，而不是即将被删的那一行。
            if let Some(keep) = canonical
                .iter()
                .find(|c| c.name == pick.name)
                .map(|c| c.id.clone())
            {
                return Some(keep);
            }
        }
    }
    canonical
        .iter()
        .min_by_key(|c| c.sort_order)
        .map(|c| c.id.clone())
}

/// 把每个现役 URL 复制进 `module_base_urls_json[def_id][module_id]`。
///
/// 单元格的值就是**旧**模型为该 (服务 × 模块) 对解析出的值，不多不少：
/// - shape A：有按模块覆盖就用覆盖，否则用环境级 URL——正是旧解析的顺序。
/// - shape B：同名 clone 在该模块下的 URL；该模块没有此服务则缺席，这正是要点：
///   空单元格表示「此模块对此服务无地址」，不得从兄弟模块回填。
///
/// 已有值的单元格保持不变，故本 pass 只增不改、重跑无事。两边都无值时**不写**该单元格：
/// 「未设置」是 UI 会显示、解析器会当作无 base URL 的状态，物化成 `""` 会让两者在存储的
/// 数据里无法区分。
fn write_value_layer(
    conn: &mut Connection,
    envs: &[EnvRow],
    modules: &[ModuleRow],
    canonical: &[CanonicalService],
    services: &[ServiceRow],
    module_owned: bool,
) -> Result<(), AppError> {
    for env in envs {
        let mut cells = env.module_base_urls.clone();
        let mut changed = false;

        for c in canonical {
            for m in modules {
                if cells.get(&c.id).is_some_and(|per| per.contains_key(&m.id)) {
                    continue;
                }
                let url = if module_owned {
                    services
                        .iter()
                        .find(|s| s.name == c.name && s.module_id.as_deref() == Some(m.id.as_str()))
                        .and_then(|s| env.base_urls.get(&s.id))
                } else {
                    env.module_base_urls
                        .get(&c.id)
                        .and_then(|per| per.get(&m.id))
                        .or_else(|| env.base_urls.get(&c.id))
                };
                if let Some(url) = url {
                    cells
                        .entry(c.id.clone())
                        .or_default()
                        .insert(m.id.clone(), url.clone());
                    changed = true;
                }
            }
        }

        if changed {
            conn.execute(
                "UPDATE environments SET module_base_urls_json = ?1 WHERE id = ?2",
                params![serde_json::to_string(&cells)?, env.id],
            )?;
        }
    }
    Ok(())
}

/// 把项目内每个 endpoint 改指到它原名所指名服务的那一行。仅 shape B——shape A 的 id 从不变。
/// 未指名服务的保持未指名：它走项目 ★，而 ★ 在同一 pass 里已改指。
/// 指名了本项目不持有的 id 的一律不动：那是跨项目引用，写路径本就拒绝，
/// 不该由数据 pass 静默改写。
fn remap_endpoints_onto_canonical(
    conn: &mut Connection,
    project_id: &str,
    survivor_of: &std::collections::HashMap<&str, &str>,
) -> Result<(), AppError> {
    if survivor_of.is_empty() {
        return Ok(());
    }
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT e.node_id, e.base_url_definition_id \
             FROM endpoint_details e \
             JOIN nodes n ON n.id = e.node_id \
             WHERE n.root_id = ?1 AND e.base_url_definition_id IS NOT NULL \
             ORDER BY e.node_id ASC",
        )?;
        let out = stmt
            .query_map(params![project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<(String, String)>, _>>()?;
        out
    };
    for (endpoint_id, named) in rows {
        let Some(keep) = survivor_of.get(named.as_str()) else {
            continue;
        };
        conn.execute(
            "UPDATE endpoint_details SET base_url_definition_id = ?1 WHERE node_id = ?2",
            params![keep, endpoint_id],
        )?;
    }
    Ok(())
}

fn read_modules(conn: &mut Connection, project_id: &str) -> Result<Vec<ModuleRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, sort_order, created_at FROM nodes \
         WHERE root_id = ?1 AND type = 'module' \
         ORDER BY sort_order ASC, created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(ModuleRow {
            id: row.get(0)?,
            sort_order: row.get(1)?,
            created_at: row.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn read_services(conn: &mut Connection, project_id: &str) -> Result<Vec<ServiceRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, module_id, sort_order, is_default \
         FROM base_url_definitions \
         WHERE project_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(ServiceRow {
            id: row.get(0)?,
            name: row.get(1)?,
            module_id: row.get(2)?,
            sort_order: row.get(3)?,
            is_default: row.get::<_, i64>(4)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn read_envs(conn: &mut Connection, project_id: &str) -> Result<Vec<EnvRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, base_urls_json, module_base_urls_json FROM environments \
         WHERE project_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        let base_urls: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&row.get::<_, String>(1)?)
                .map_err(|e| json_col_err("base_urls_json", e))?;
        let module_base_urls: std::collections::BTreeMap<
            String,
            std::collections::BTreeMap<String, String>,
        > = match row.get::<_, String>(2) {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|e| json_col_err("module_base_urls_json", e))?
            }
            Err(_) => Default::default(),
        };
        Ok(EnvRow {
            id: row.get(0)?,
            base_urls,
            module_base_urls,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}
