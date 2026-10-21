//! 导入 / 导出的编排层：**两条 transport 共用的那一份**。
//!
//! 住在本 crate 而不是 Tauri command 里，是 AGENTS.md 的 dual-transport invariant 的
//! 直接后果：逻辑写在某一侧，另一侧就会静默地少一半。Tauri command 与 REST handler
//! 都只调这里。
//!
//! 四件事，每件都只做一次：
//!
//! 1. **取内容。** 文件内容由渲染层读过；URL 则在这里拉——拉取必须走
//!    [`simple_post_core::http_executor::execute_with_policy`]，它保留 SSRF 防护
//!    （`check_url`）与超时 / 重定向 / 大小上限。**不要**另开一条裸 `reqwest` 通道。
//! 2. **翻成 IR。** [`simple_post_interchange::import`] 的 `sniff` + `parse_text`。
//! 3. **折成线形状。** IR → [`TransferNode`] 树，顺手把指纹命中的条目标成「已存在」、
//!    把诊断按端点归属。
//! 4. **落库 / 落文档。** 落库与创建 Postman 环境在**同一个事务**里。

use std::collections::BTreeSet;

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::uid;
use simple_post_core::error::AppError;
use simple_post_core::http_executor::{self, HttpRequestInput, OutboundPolicy};
use simple_post_interchange::commit::{commit_in_tx, plan_service, CommitTarget};
use simple_post_interchange::fingerprint::{FingerprintSet, RequestIdentity};
use simple_post_interchange::import;
use simple_post_interchange::ir::{Exchange, IrCollection, IrEndpoint, IrModule};
use simple_post_interchange::openapi::DocFormat;
use simple_post_interchange::project::module_to_ir;

use crate::dto::{
    AppliedServiceDto, ExportDocumentDto, ExportDocumentOpts, ExportPreviewDto, ExportPreviewOpts,
    ImportCommitDto, ImportCommitOpts, ImportPreviewDto, ImportPreviewOpts, ImportedEnvironmentDto,
    ModuleServersDto, ProposedServiceDto, TransferCounts, TransferEndpoint, TransferNode,
};

/// 拉 OpenAPI / Postman 文档时的大小上限。
///
/// **必须显式给。** [`HttpRequestInput::max_response_size_kb`] 由调用方给，
/// 不给就是 0——而 [`http_executor`] 会把超限的响应**截断**后照样返回一个 200。
/// 一份 3 MB 的 OpenAPI yaml 于是变成一个尾部带着截断标记、解析必然失败的文档，
/// 而错误信息只会说「JSON 解析失败」，看不出真正的原因是「被我们自己砍了」。
const FETCH_MAX_KB: u64 = 8 * 1024;

/// 这四个端点的 HTTP 请求体上限，与 [`FETCH_MAX_KB`] 挨着放，因为两者讲的是同一件事。
///
/// **不给就是 axum 的 2 MB 默认值**，而导入导出恰恰是唯一一类「请求体装着整份文档」的动词。
/// 那时超限回的是 `413 Failed to buffer the request body`，用户看到的是传输层错误而不是
/// 「文档太大」，且桌面形态（Tauri IPC 无此限制）同一份文件能正常导入——两侧行为分叉，
/// 没有任何一层会报错。
///
/// 取两倍余量：URL 模式下原文要在 preview 与 commit 之间来回传两次，而 JSON 字符串的
/// 转义会让同样的字节数再涨一截。两倍仍是**有界**缓冲，不是无上限。
pub(crate) const TRANSFER_BODY_LIMIT_BYTES: usize = (FETCH_MAX_KB * 1024 * 2) as usize;

/// 内容来源。`from_url` 为真时，预览要把原文回传给前端，让提交那一步不必再拉一次。
struct Resolved {
    text: String,
    from_url: bool,
    name: String,
}

/// 导入预览。渲染层先拿它渲染中间态，用户勾完再提交。
pub async fn import_preview(
    pool: &DbPool,
    opts: &ImportPreviewOpts,
) -> Result<ImportPreviewDto, AppError> {
    let source = resolve_content(opts.content.as_deref(), opts.url.as_deref()).await?;
    let (format, parsed) = import::sniff_and_parse(&source.text)?;

    // 去重账本只灌**已落库**的行：预览要回答的是「这些条目项目里已经有了吗」，
    // 而这个答案必须与真正落库时用的是同一本账。用 `contains`（不记账），
    // 否则这份预览会替落库先把账记上一遍。
    //
    // 「这个项目能不能接住这份文档」是同一批只读，搭同一次连接走完。
    let (duplicates, plan) = db::with_conn(pool, |conn| {
        let existing = db::endpoint_details::list_by_root(conn, &opts.project_id)?;
        let mut set = FingerprintSet::new();
        set.absorb_details(existing.iter());
        let dup = mark_duplicates(&parsed.exchange, &set);
        Ok((dup, plan_service(conn, &opts.project_id, &parsed.exchange)?))
    })?;

    let groups = project_modules(&parsed.exchange.modules, &duplicates, &parsed.exchange);
    let counts = TransferCounts {
        groups: count_groups(&groups),
        endpoints: parsed.exchange.endpoint_count(),
        duplicates: duplicates.len(),
    };

    // 多 host 的理由在预览里就得说，否则用户勾完才发现服务没建。
    let mut diagnostics = parsed.exchange.diagnostics.clone();
    diagnostics.extend(plan.ambiguous_diagnostic());

    Ok(ImportPreviewDto {
        format: format.as_str().to_string(),
        format_label: format.label().to_string(),
        title: non_empty(parsed.exchange.project_name.clone())
            .or_else(|| parsed.exchange.project_description.clone()),
        source_name: source.name,
        groups,
        counts,
        diagnostics,
        service: plan.server.as_ref().map(|s| ProposedServiceDto {
            service: s.service.clone(),
            url: s.url.clone(),
            existing: plan.existing_id.is_some(),
            environment_name: plan.environment.as_ref().map(|e| e.name.clone()),
        }),
        environments: parsed
            .environments
            .iter()
            .map(|e| ImportedEnvironmentDto {
                name: e.name.clone(),
                variable_count: e.variables.len(),
                disabled_variable_count: e.disabled_variables.len(),
            })
            .collect(),
        resolved_content: source.from_url.then_some(source.text),
    })
}

/// 把勾中的条目落进目标模块。
///
/// **重新解析同一份 `content`，不重跑 URL 拉取** —— 预览已经把 URL 模式的原文回传，
/// 前端原样送到这里。缓存解析结果意味着要加临时表 + TTL + 清理路径，而解析是纯函数：
/// 重跑一次的成本远低于那套复杂度，换来的是「预览里看到的和落下去的一定是同一棵树」。
pub fn import_commit(pool: &DbPool, opts: &ImportCommitOpts) -> Result<ImportCommitDto, AppError> {
    let (_format, parsed) = import::sniff_and_parse(&opts.content)?;
    let selected: BTreeSet<String> = opts.selected.iter().cloned().collect();

    db::with_tx(pool, |conn| {
        let report = commit_in_tx(
            conn,
            &opts.project_id,
            CommitTarget::Module {
                module_id: &opts.module_id,
            },
            &parsed.exchange,
            &selected,
            // 缺席为真：文档带了 host 就该有服务，用户在预览里可以逐次取消。
            opts.apply_service.unwrap_or(true),
        )?;

        // 环境与树同事务：一次导入要么全进去，要么一行不进。半棵树加半份环境比
        // 什么都没有更难收拾。
        let mut environments_created = Vec::new();
        for env in &parsed.environments {
            // id 与 project 由落库方铸。解析器只负责「这个环境叫什么、有哪些变量」，
            // 身份是本仓库的事——所以在这里 clone 一份改，而不是就地改 borrowed 值。
            //
            // `is_active` 恒 false：导入一个环境不等于切换当前环境，那是一次显式决定。
            let mut owned = env.clone();
            owned.id = uid();
            owned.project_id = opts.project_id.clone();
            owned.is_active = false;
            db::environments::create(conn, &owned)?;
            environments_created.push(owned.name.clone());
        }

        Ok(ImportCommitDto {
            created_endpoints: report.created_endpoint_ids.len(),
            skipped_duplicates: report.skipped_duplicates.len(),
            created_collection_ids: report.created_collection_ids,
            created_endpoint_ids: report.created_endpoint_ids,
            environments_created,
            service: report.service.map(|s| AppliedServiceDto {
                name: s.name,
                url: s.url,
                created: s.created,
                assigned: s.assigned,
                environment_name: s.environment_name,
            }),
            diagnostics: report.diagnostics,
        })
    })
}

/// 导出候选预览。
///
/// `opts.environment_id` 指名按**哪个环境**解析 `servers`：缺席或空串取激活环境，
/// 给了 id 就按那个环境解析——渲染层在下拉里换了环境就会重取一趟，否则那份清单
/// 仍是旧环境下的事实，界面上的提示会按旧环境点名。
pub fn export_preview(
    pool: &DbPool,
    opts: &ExportPreviewOpts,
) -> Result<ExportPreviewDto, AppError> {
    // 空串 = 渲染层「不选环境」那一项，与「没传」等价：都退回激活环境。
    let env = opts
        .environment_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let ex = module_to_ir(pool, &opts.project_id, &opts.module_id, env)?;
    let duplicates: BTreeSet<String> = BTreeSet::new();
    let groups = project_modules(&ex.modules, &duplicates, &ex);

    Ok(ExportPreviewDto {
        counts: TransferCounts {
            groups: count_groups(&groups),
            endpoints: ex.endpoint_count(),
            duplicates: 0,
        },
        environment_id: ex.environment_id.clone(),
        servers: ex
            .modules
            .iter()
            .map(|m| ModuleServersDto {
                module_key: m.key.clone(),
                module_name: m.name.clone(),
                servers: m.servers.clone(),
            })
            .collect(),
        groups,
        diagnostics: ex.diagnostics.clone(),
    })
}

/// 按选中的环境与勾选的条目产出文档本身。
pub fn export_document(
    pool: &DbPool,
    opts: &ExportDocumentOpts,
) -> Result<ExportDocumentDto, AppError> {
    let format = DocFormat::parse(&opts.format)?;
    let env = if opts.environment_id.trim().is_empty() {
        None
    } else {
        Some(opts.environment_id.as_str())
    };
    let ex = module_to_ir(pool, &opts.project_id, &opts.module_id, env)?;
    let selected: BTreeSet<String> = opts.selected.iter().cloned().collect();
    let pruned = ex.retain_selected(&selected);

    let doc = simple_post_interchange::openapi::to_document(&pruned, format)?;
    // 导出器自己的诊断并进来，而不是替换掉 IR 里的那些：投影阶段也可能发现问题
    // （比如 verb 落在 `Custom` 上），两类诊断用户都要看见。
    let mut diagnostics = pruned.diagnostics;
    diagnostics.extend(doc.diagnostics);

    Ok(ExportDocumentDto {
        file_name: file_name(&ex.project_name, format),
        media_type: format.media_type().to_string(),
        content: doc.content,
        diagnostics,
    })
}

// -------------------------------------------------------------------------
// 内容来源
// -------------------------------------------------------------------------

/// 文件内容与 URL 二选一。URL 分支的出站请求**必须**经 `execute_with_policy`：
/// 那里的 `check_url` 是本仓库唯一的 SSRF 防护，绕开它等于把服务端变成开放代理。
async fn resolve_content(content: Option<&str>, url: Option<&str>) -> Result<Resolved, AppError> {
    if let Some(c) = content.filter(|c| !c.trim().is_empty()) {
        return Ok(Resolved {
            text: c.to_string(),
            from_url: false,
            name: String::new(),
        });
    }
    let Some(url) = url.filter(|u| !u.trim().is_empty()) else {
        return Err(AppError::Validation("请给一个文件或一个文档链接".into()));
    };

    // `execute_with_policy` 返回的是 `HttpResult` 而不是 `Result`：传输层失败被塞进
    // `HttpResult.error`，故这里不能 `?`，得先自己判 `ok`。
    let result = http_executor::execute_with_policy(
        &HttpRequestInput {
            method: "GET".into(),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            timeout_ms: 30_000,
            follow_redirects: true,
            max_response_size_kb: FETCH_MAX_KB,
        },
        OutboundPolicy::from_env(),
    )
    .await;

    if !result.ok {
        let detail = result
            .error
            .map(|e| e.message)
            .unwrap_or_else(|| "未知原因".into());
        return Err(AppError::Validation(format!("拉取文档失败：{detail}")));
    }
    let response = result
        .response
        .ok_or_else(|| AppError::Http("拉取没有拿到响应".into()))?;

    // 截断是可读的失败，不是解析失败。不在这里拦，用户看到的会是「YAML 解析失败」，
    // 而真正的原因是「我们自己把它砍了」。
    if response.truncated {
        return Err(AppError::Validation(format!(
            "文档超过 {FETCH_MAX_KB} KB 上限，已被截断，无法解析"
        )));
    }

    Ok(Resolved {
        text: response.body,
        from_url: true,
        name: url.split('?').next().unwrap_or(url).to_string(),
    })
}

// -------------------------------------------------------------------------
// IR → 线形状
// -------------------------------------------------------------------------

/// 标出「项目里已经有的」那些端点。用 [`FingerprintSet::contains`] 而不是 `seen`：
/// 预览只是问，不能记账。
fn mark_duplicates(ex: &Exchange, set: &FingerprintSet) -> BTreeSet<String> {
    all_endpoints(ex)
        .into_iter()
        .filter(|ep| set.contains(&RequestIdentity::of_ir(ep)))
        .map(|ep| ep.key.clone())
        .collect()
}

/// 整棵交换里的全部端点。集合树深度没有上限，故走显式栈而不是递归。
fn all_endpoints(ex: &Exchange) -> Vec<&IrEndpoint> {
    let mut out: Vec<&IrEndpoint> = Vec::new();
    let mut stack: Vec<&IrCollection> = ex.modules.iter().flat_map(|m| &m.collections).collect();
    for m in &ex.modules {
        out.extend(&m.endpoints);
    }
    while let Some(c) = stack.pop() {
        out.extend(&c.endpoints);
        stack.extend(c.collections.iter().rev());
    }
    out
}

fn project_modules(
    modules: &[IrModule],
    duplicates: &BTreeSet<String>,
    ex: &Exchange,
) -> Vec<TransferNode> {
    modules
        .iter()
        .map(|m| project_module(m, duplicates, ex))
        .collect()
}

fn project_module(m: &IrModule, duplicates: &BTreeSet<String>, ex: &Exchange) -> TransferNode {
    TransferNode {
        key: m.key.clone(),
        name: m.name.clone(),
        endpoints: m
            .endpoints
            .iter()
            .map(|e| project_endpoint(e, duplicates, ex))
            .collect(),
        children: m
            .collections
            .iter()
            .map(|c| project_collection(c, duplicates, ex))
            .collect(),
        endpoint_total: m.endpoint_count(),
    }
}

fn project_collection(
    c: &IrCollection,
    duplicates: &BTreeSet<String>,
    ex: &Exchange,
) -> TransferNode {
    TransferNode {
        key: c.key.clone(),
        name: c.name.clone(),
        endpoints: c
            .endpoints
            .iter()
            .map(|e| project_endpoint(e, duplicates, ex))
            .collect(),
        children: c
            .collections
            .iter()
            .map(|k| project_collection(k, duplicates, ex))
            .collect(),
        endpoint_total: c.endpoint_count(),
    }
}

fn project_endpoint(
    e: &IrEndpoint,
    duplicates: &BTreeSet<String>,
    ex: &Exchange,
) -> TransferEndpoint {
    TransferEndpoint {
        key: e.key.clone(),
        name: e.name.clone(),
        method: e.method.as_str().to_string(),
        path: e.path.clone(),
        body_mode: e.body.mode.clone(),
        auth: e.auth.kind.clone(),
        duplicate: duplicates.contains(&e.key),
        diagnostic_indexes: diagnostics_of(ex, &e.key),
    }
}

/// 落在某个端点上的诊断下标。
///
/// 按 pointer 的前缀归属：一个 `Warning` 的 pointer 是 `/item/2/request/url`，
/// 它属于 key 为 `/item/2/request` 的那一行。前缀比对补一个 `/` 才比，
/// 否则 `/item/2` 会认领掉 `/item/21/...` 的诊断。
fn diagnostics_of(ex: &Exchange, key: &str) -> Vec<usize> {
    let prefix = format!("{key}/");
    ex.diagnostics
        .iter()
        .enumerate()
        .filter(|(_, d)| d.pointer == key || d.pointer.starts_with(&prefix))
        .map(|(i, _)| i)
        .collect()
}

fn count_groups(nodes: &[TransferNode]) -> usize {
    nodes
        .iter()
        .map(|n| 1 + count_groups(&n.children))
        .sum::<usize>()
}

fn non_empty(s: String) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 文件名。`safe` 是路径分隔符与控制字符都拿掉了的项目名——它来自用户，
/// 而它会拼进一个下载路径。
fn file_name(project_name: &str, format: DocFormat) -> String {
    let stem: String = project_name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let stem = stem.trim();
    let stem = if stem.is_empty() { "simple-post" } else { stem };
    format!("{stem}{}", format.extension())
}

// 测试——取内容、投影与文件名。

#[cfg(test)]
#[path = "interchange_api_test.rs"]
mod tests;
