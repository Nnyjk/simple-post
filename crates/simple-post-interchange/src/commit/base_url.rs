//! 地址：外来文档的 host → 环境的 (环境 × 模块) 那一格。

use simple_post_core::db::{self, Connection};
use simple_post_core::domain::Environment;
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{endpoints_of, Exchange};

use super::{CommitReport, DOC_ROOT};

/// 一份外来文档给出的地址，撞上目标项目之后长什么样。
///
/// 预览与落库**共用 [`plan_base_url`]**：两处各写一份的话，用户会在预览里看到
/// 「将写入地址 A」而落库时得到 B，而中间没有任何一步会报错。落库那一次必须在事务里
/// 重算（读的是事务内的状态），但规则是这个函数。
#[derive(Debug, Clone, Default)]
pub struct BaseUrlPlan {
    /// 整份文档里的地址，按首次出现顺序去重。
    ///
    /// 同一个地址在多个模块里各写一遍只算一个候选——那是同一台机器，不是多台。
    pub candidates: Vec<String>,
    /// 项目的**激活**环境。地址只能住进一个环境里，而「哪个环境」只有一个可答的
    /// 来源：用户当前正在用的那个。为 `None` 时地址无处可放。
    pub environment: Option<Environment>,
}

impl BaseUrlPlan {
    /// 多于一个地址时的那条说明。
    ///
    /// 一个模块一格，故多个地址不冲突：模块的地址分别落在各自的模块格子里，端点自己
    /// 带着的那个留在端点上。这条说明只是告诉用户「有几台机器」，不是「有件事没做成」。
    pub fn multiple_diagnostic(&self) -> Option<Diagnostic> {
        if self.candidates.len() < 2 {
            return None;
        }
        Some(
            Diagnostic::info(
                DOC_ROOT,
                format!(
                    "这份文档有 {} 个地址，分别写进各自的模块格子与端点",
                    self.candidates.len()
                ),
            )
            .dropping(self.candidates.join("；")),
        )
    }
}

/// 解析这份文档的地址，并查清目标项目能不能接住它。**只读**，故预览可以直接调。
pub fn plan_base_url(
    conn: &mut Connection,
    project_id: &str,
    ex: &Exchange,
) -> Result<BaseUrlPlan, AppError> {
    let mut candidates: Vec<String> = Vec::new();
    for m in &ex.modules {
        if let Some(cell) = &m.base_url {
            push_candidate(&mut candidates, &cell.url);
        }
        for ep in endpoints_of(m) {
            if let Some(own) = &ep.base_url_override {
                push_candidate(&mut candidates, own);
            }
        }
    }
    let environment = db::environments::get_active(conn, project_id)?;
    Ok(BaseUrlPlan {
        candidates,
        environment,
    })
}

/// 记下一个地址。空串不算地址，重复的地址只算一个候选。
fn push_candidate(candidates: &mut Vec<String>, url: &str) {
    if url.is_empty() {
        return;
    }
    if !candidates.iter().any(|c| c == url) {
        candidates.push(url.to_string());
    }
}

/// 把一个模块的地址写进它那一格，并回报。
///
/// 导入只写规格里给出的那一段：规格给了地址就写地址，规格没提代理时那一格原有的代理保留，
/// 用户配的网关不被一次导入抹掉。
///
/// 报告里写回那一格**实际**的代理值：规格给了代理就是规格那个，规格没给就是那一格原来的
/// 那个（可能没有代理）。
///
/// 没有激活环境就没有格子可填，于是地址无处可去——报告里 `environment_name` 为 `None`，
/// 用户能看见「地址没有落进任何一格」，而不是以为它已经生效了。
pub(super) fn wire_base_url(
    conn: &mut Connection,
    module_id: &str,
    url: &str,
    proxy: Option<&str>,
    environment: Option<&Environment>,
    report: &mut CommitReport,
) -> Result<(), AppError> {
    let mut applied_proxy = None;
    if let Some(env) = environment {
        // `proxy` 传 `None` = 那一格的代理不动，故规格没提代理时该格原有的代理保留。
        let updated =
            db::environments::set_module_base_url(conn, &env.id, module_id, Some(url), proxy)?;
        // 读回那一格实际的代理，而不是把规格里那份原样抄进报告：规格没给代理时那一格
        // 可能本来就配着网关。
        applied_proxy = updated
            .module_base_urls
            .get(module_id)
            .and_then(|cell| cell.proxy.clone());
    }
    report.base_urls.push(super::AppliedBaseUrl {
        url: url.to_string(),
        proxy: applied_proxy,
        module_id: module_id.to_string(),
        environment_name: environment.map(|e| e.name.clone()),
    });
    Ok(())
}
