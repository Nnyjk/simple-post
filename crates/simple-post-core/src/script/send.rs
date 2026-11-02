//! 「装配脚本链 → 前置脚本 → 发出 → 测试脚本 → 变量回写」这条序列的**唯一**实现。
//!
//! 桌面端与 MCP 端都调它，故两侧不可能对「脚本插在请求构造的哪一步」分叉。顺序若由
//! 调用方编排，就会变成第二份实现——那正是 issue 点名要消灭的那类漂移。
//!
//! 变量回写由**宿主**落库，于是持久化只有一条路径；渲染层那份只是缓存。

use std::collections::HashMap;

use crate::db;
use crate::domain::{
    EnvWrite, EnvironmentPatch, RequestSettings, ScriptError, ScriptErrorKind, ScriptOutcome,
    ScriptPhase, TlsSettings,
};
use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput, HttpResult};

use super::chain::{collect_chain, merge_env};
use super::limits::DEFAULT_SCRIPT_LIMITS;
use super::run::run_phase;
use super::types::{EnvSnapshot, ScriptRequest, ScriptResponse, ScriptSource, ScriptUnit};

/// 一次「带脚本的发送」的输入。
pub struct ScriptedSendInput {
    /// 端点 id。`None` = 临时请求，没有脚本链。
    pub endpoint_id: Option<String>,
    /// 已经构造好的请求（变量已解析、鉴权已注入）。
    pub request: ScriptRequest,
    /// 脚本看得到的环境。`id` 决定变量往哪写。
    pub env: EnvSnapshot,
    pub settings: RequestSettings,
    /// 该端点自己的 HTTPS 出站设置。`None` = 继承全局默认。
    ///
    /// 全局默认与端点的覆盖在 [`crate::tls::resolve`] 里合成，故调用方只管
    /// 交出端点这一份，不必自己读库、也不必自己合成——桌面端、`serve-web` 端与 MCP 端
    /// 于是不会对「这次请求用哪份证书」各说各话。
    pub tls_override: Option<TlsSettings>,
    /// 端点自己那两段脚本的覆盖。
    ///
    /// `None` = 用存的那份；`Some` = 用这份（空串 = 没有脚本）。
    ///
    /// 渲染层送的是**界面上此刻生效的那份**（草稿优先，回落到已存的那份），
    /// 否则「所见」与「所发」会分叉：用户改完脚本还没保存就发送，跑的却是旧脚本。
    pub script_override: Option<ScriptOverride>,
}

/// 端点自己那两段脚本的覆盖。
#[derive(Debug, Clone, Default)]
pub struct ScriptOverride {
    pub pre_request: Option<String>,
    pub test: Option<String>,
}

/// 一次「带脚本的发送」的结果。
pub struct ScriptedSendOutcome {
    /// 原样的传输层信封。
    pub http: HttpResult,
    /// 链上没有任何脚本时为 `None`。
    pub script: Option<ScriptOutcome>,
    /// **真正发出去的那个请求**——前置脚本可能改写过它。
    pub request: ScriptRequest,
}

/// 发一次请求，途中按链跑脚本。
pub async fn send_with_scripts(
    pool: &db::DbPool,
    input: ScriptedSendInput,
) -> Result<ScriptedSendOutcome, AppError> {
    // HTTPS 出站设置：全局默认（`settings.tls`）与端点自己的那份（`tls_override`）合成。
    // 合成只有这一处，故桌面端、`serve-web` 端与 MCP 端对「这次请求用哪份证书」不会各说各话。
    let tls = crate::tls::resolve(&input.settings.tls, input.tls_override.as_ref());

    let chain = match input.endpoint_id.as_deref() {
        Some(id) => db::with_conn(pool, |c| {
            collect_chain(c, id, input.script_override.as_ref())
        })?,
        None => match &input.script_override {
            // 没有端点 id，但请求里带了两段脚本：照样要跑。
            //
            // 草稿接口就是走这条路的——它没落库的行，故没有集合链，但它自己的那两段
            // 脚本是在界面上写出来的，照样该跑。
            Some(override_) if override_.pre_request.is_some() || override_.test.is_some() => {
                vec![ScriptUnit {
                    source: ScriptSource::Endpoint {
                        id: String::new(),
                        name: String::new(),
                    },
                    pre_request: override_.pre_request.clone(),
                    test: override_.test.clone(),
                }]
            }
            _ => Vec::new(),
        },
    };
    if chain.is_empty() {
        // 没有脚本：与今天完全同路径。
        let http =
            http_executor::execute(&to_http_input(&input.request, &input.settings, &tls)).await;
        return Ok(ScriptedSendOutcome {
            http,
            script: None,
            request: input.request,
        });
    }

    // 前置脚本：变量替换之后、请求发出之前。
    let pre = run_phase_off_thread(
        ScriptPhase::Pre,
        chain.clone(),
        input.request.clone(),
        None,
        input.env.clone(),
    )
    .await;

    // 发出去的是脚本改写后的请求。
    let http = http_executor::execute(&to_http_input(&pre.request, &input.settings, &tls)).await;

    // 测试脚本：响应回来之后。
    //
    // 环境快照要先并上前置阶段的写入：Postman 让测试脚本看得到前置脚本写下的变量，
    // 否则「前置写 token、测试读 token」这种最常见的组合读到的还是旧值。
    let response = ScriptResponse::from_http(&http);
    let mut test_env = input.env.clone();
    merge_env(&mut test_env.variables, &pre.outcome.env_writes);
    let test = run_phase_off_thread(
        ScriptPhase::Test,
        chain,
        pre.request.clone(),
        Some(response),
        test_env,
    )
    .await;

    let mut outcome = ScriptOutcome::empty();
    outcome.merge(pre.outcome);
    outcome.merge(test.outcome);

    // 变量回写。落库在这里，故只有一条持久化路径。
    //
    // 写不进去**不能**让整次发送失败：请求已经发出去了，脚本的产出也已经在手，
    // 丢掉它们只因为一个环境写失败，是把「脚本的事」升级成「请求的事」。
    // 故失败被记成一条脚本错误，请求的成败照旧。
    match apply_env_writes(pool, &input.env, &outcome.env_writes) {
        Ok(applied) => outcome.env_writes = applied,
        Err(e) => {
            outcome.env_writes.clear();
            outcome.errors.push(ScriptError {
                kind: ScriptErrorKind::Runtime,
                message: format!("cannot persist the environment writes: {e}"),
                source: String::new(),
                phase: ScriptPhase::Test,
            });
        }
    }

    Ok(ScriptedSendOutcome {
        http,
        script: Some(outcome),
        request: pre.request,
    })
}

/// 一个阶段跑完的产物：改写后的请求与这次阶段的产出。
struct PhaseRun {
    request: ScriptRequest,
    outcome: ScriptOutcome,
}

/// 跑一个阶段。**放到阻塞线程上**：引擎是同步且 CPU 密集的，在异步 worker 上跑会卡住
/// 整个运行时（issue 明令：不得在主进程 / tokio runtime 里直接 eval 用户代码）。
async fn run_phase_off_thread(
    phase: ScriptPhase,
    chain: Vec<ScriptUnit>,
    request: ScriptRequest,
    response: Option<ScriptResponse>,
    env: EnvSnapshot,
) -> PhaseRun {
    let fallback = request.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let mut request = request;
        let outcome = run_phase(
            phase,
            &chain,
            &mut request,
            response.as_ref(),
            &env,
            &DEFAULT_SCRIPT_LIMITS,
        );
        PhaseRun { request, outcome }
    });
    match handle.await {
        Ok(run) => run,
        // 阻塞线程里的 panic。release profile 是 `panic = "abort"`，那里到不了这里；
        // 测试构建下才可能，故仍要给调用方一个能看懂的结果。
        Err(join_error) => PhaseRun {
            request: fallback,
            outcome: ScriptOutcome {
                errors: vec![ScriptError {
                    kind: ScriptErrorKind::Runtime,
                    message: format!("the script engine panicked: {join_error}"),
                    source: String::new(),
                    phase,
                }],
                ..ScriptOutcome::empty()
            },
        },
    }
}

/// 把脚本写下的变量落到环境行。
///
/// 返回**已应用**的写入：没有环境可写时返回空，渲染层据此知道自己的副本不必更新。
fn apply_env_writes(
    pool: &db::DbPool,
    env: &EnvSnapshot,
    writes: &[EnvWrite],
) -> Result<Vec<EnvWrite>, AppError> {
    if writes.is_empty() {
        return Ok(Vec::new());
    }
    let Some(env_id) = env.id.as_deref() else {
        // 没有环境可写。脚本仍能读到变量（快照里有），只是写不进去。
        return Ok(Vec::new());
    };
    db::with_tx(pool, |c| {
        let current = db::environments::get(c, env_id)?;
        let mut variables: std::collections::BTreeMap<String, String> = current
            .variables
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        merge_env(&mut variables, writes);
        let variables: HashMap<String, String> = variables.into_iter().collect();
        db::environments::update(
            c,
            env_id,
            EnvironmentPatch {
                variables: Some(variables),
                ..Default::default()
            },
        )?;
        Ok(())
    })?;
    Ok(writes.to_vec())
}

/// 脚本的请求形状 → 发送器的输入形状。
fn to_http_input(
    request: &ScriptRequest,
    settings: &RequestSettings,
    tls: &TlsSettings,
) -> HttpRequestInput {
    HttpRequestInput {
        method: request.method.clone(),
        url: request.url.clone(),
        headers: request.headers.clone(),
        body: request.body.clone(),
        body_base64: request.body_base64.clone(),
        timeout_ms: settings.timeout_ms,
        follow_redirects: settings.follow_redirects,
        max_response_size_kb: settings.max_response_size_kb,
        tls: tls.clone(),
    }
}

impl ScriptResponse {
    /// 传输层的信封 → 脚本看得见的响应。
    ///
    /// 请求根本没发出去时给一份空的：test 脚本于是看到 `code === 0`，而不是崩掉。
    pub fn from_http(result: &HttpResult) -> Self {
        match &result.response {
            Some(response) => Self {
                code: response.status,
                status: response.status_text.clone(),
                headers: response
                    .headers
                    .iter()
                    .map(|[name, value]| (name.clone(), value.clone()))
                    .collect(),
                body: response.body.clone(),
                body_base64: response.body_base64.clone(),
                response_time_ms: response.duration_ms as u64,
            },
            None => Self::default(),
        }
    }
}
