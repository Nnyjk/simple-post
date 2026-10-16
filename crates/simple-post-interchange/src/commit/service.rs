//! 服务：外来文档的 host → `base_url_definitions` + `environments.module_base_urls`。

use simple_post_core::db::{self, Connection};
use simple_post_core::domain::{uid, BaseUrlDefinition, Environment};
use simple_post_core::error::AppError;

use crate::diagnostics::Diagnostic;
use crate::ir::{Exchange, IrServer};

use super::{AppliedService, CommitReport, DOC_ROOT};

/// 一份外来文档给出的服务，撞上目标项目之后长什么样。
///
/// 预览与落库**共用 [`plan_service`]**：两处各写一份的话，用户会在预览里看到
/// 「将新建服务 A」而落库时得到 B，而中间没有任何一步会报错。落库那一次必须在事务里
/// 重算（复用与否是事务内的读），但规则是这个函数。
#[derive(Debug, Clone, Default)]
pub struct ServicePlan {
    /// 恰好一个 host 时是它。为 `None` 表示**不做**逐接口指派，理由见 `candidates`。
    pub server: Option<IrServer>,
    /// 全部候选，按首次出现顺序、逐个 (服务名, URL) 去重。
    ///
    /// 同一 host 在多个模块里各写一遍只算一个候选；同名服务挂两个协议则是两个——
    /// 服务表按名字唯一，同名两条落不进去。
    pub candidates: Vec<IrServer>,
    /// 项目里已存在的同名服务定义。有它就复用，没有就新建。
    pub existing_id: Option<String>,
    /// 项目的**激活**环境。URL 只能住进一个环境单元格，而「哪个环境」只有一个可答的
    /// 来源：用户当前正在用的那个。为 `None` 时服务仍然建，只是 URL 无处可放。
    pub environment: Option<Environment>,
}

impl ServicePlan {
    /// 多于一个 host 时的那条说明。
    ///
    /// 没有 host 是什么都没发生，不出诊断；多于一个才需要解释「为什么没建服务」。
    pub fn ambiguous_diagnostic(&self) -> Option<Diagnostic> {
        if self.candidates.len() < 2 {
            return None;
        }
        let listed: Vec<String> = self
            .candidates
            .iter()
            .map(|c| format!("{} → {}", c.service, c.url))
            .collect();
        Some(
            Diagnostic::warn(
                DOC_ROOT,
                format!(
                    "这份文档有 {} 台服务，无法逐个接口指派，未创建服务",
                    self.candidates.len()
                ),
            )
            .dropping(listed.join("；")),
        )
    }
}

/// 解析这份文档的服务，并查清目标项目能不能接住它。**只读**，故预览可以直接调。
pub fn plan_service(
    conn: &mut Connection,
    project_id: &str,
    ex: &Exchange,
) -> Result<ServicePlan, AppError> {
    let mut candidates: Vec<IrServer> = Vec::new();
    for m in &ex.modules {
        for s in &m.servers {
            if !candidates
                .iter()
                .any(|c| c.service == s.service && c.url == s.url)
            {
                candidates.push(s.clone());
            }
        }
    }
    let server = (candidates.len() == 1).then(|| candidates[0].clone());

    // 只在「恰好一个」时才去查复用与激活环境：其余分支不会写这两个字段，查了也丢。
    let (existing_id, environment) = match &server {
        Some(s) => {
            let existing_id = db::base_url_defs::list_by_project(conn, project_id)?
                .into_iter()
                .find(|d| d.name == s.service)
                .map(|d| d.id);
            (existing_id, db::environments::get_active(conn, project_id)?)
        }
        None => (None, None),
    };
    Ok(ServicePlan {
        server,
        candidates,
        existing_id,
        environment,
    })
}

/// 端点要指过去的那一对 id。**两个都齐**才会构造出来：没有激活环境就没有格子可填，
/// 于是端点也不指过去。
pub(super) struct ServiceWiring {
    pub(super) definition_id: String,
    pub(super) environment_id: String,
}

/// 把 [`ServicePlan::server`] 落进项目，并回报。多个 host 时什么都不做。
///
/// 复用同名服务是安全的：服务行本身**不持有 URL**，URL 只在
/// `env.module_base_urls[def_id][module_id]` 那一个格子里，故填自己这一格不会毁掉
/// 任何别的模块的地址。
pub(super) fn wire_service(
    conn: &mut Connection,
    project_id: &str,
    module_id: &str,
    plan: &ServicePlan,
    report: &mut CommitReport,
) -> Result<Option<ServiceWiring>, AppError> {
    let Some(srv) = &plan.server else {
        return Ok(None);
    };
    let created = plan.existing_id.is_none();
    let definition_id = match &plan.existing_id {
        Some(id) => id.clone(),
        None => {
            let id = uid();
            db::base_url_defs::create(
                conn,
                &BaseUrlDefinition {
                    id: id.clone(),
                    name: srv.service.clone(),
                    description: None,
                },
                project_id,
            )?;
            id
        }
    };

    if let Some(env) = &plan.environment {
        db::environments::set_module_base_url(
            conn,
            &env.id,
            &definition_id,
            module_id,
            Some(&srv.url),
        )?;
    }

    // 没有激活环境就没有格子可填，于是端点**也不指过去**：指过去会让 URL 解析链走到一个
    // 空格子，那是一个静默的空，而「用项目的 ★」是一个真实答案——项目有 ★ 时这一改
    // 就是一次静默的降级。服务本身仍然建着，用户补上地址与指派即可。
    let wiring = plan.environment.as_ref().map(|env| ServiceWiring {
        definition_id,
        environment_id: env.id.clone(),
    });
    report.service = Some(AppliedService {
        name: srv.service.clone(),
        url: srv.url.clone(),
        created,
        assigned: wiring.is_some(),
        environment_name: plan.environment.as_ref().map(|e| e.name.clone()),
    });
    Ok(wiring)
}
