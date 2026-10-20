//! 沙箱引擎：建立 QuickJS 运行时、装上资源上限、跑脚本、把改动读回来。
//!
//! **这里不得 panic。** release profile 是 `panic = "abort"`，脚本路径上任何 panic 都会
//! 直接终止进程——而宿主（MCP 守护进程）正替别人握着连接。所有 rquickjs 的 `Result`
//! 都必须显式处理，错误一律变成 [`ScriptFailure`]。
//!
//! **这里不得写 stdout。** `simple-post-mcp` 的 stdout 只允许 JSON-RPC 帧，而本模块
//! 被它链接。`console.log` 走内存里的日志数组，不碰任何进程输出。
//!
//! 引擎实例**不跨线程**（`Runtime` / `Context` 非 `Send`），故整条脚本链在一个
//! `spawn_blocking` 闭包里跑完，见 [`super::run`]。

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rquickjs::{Context, Runtime, Value};
use serde::Deserialize;
use serde_json::{Map, Value as Json};

use super::limits::ScriptLimits;
use super::prelude::PRELUDE;
use super::types::{
    AssertionResult, EnvSnapshot, EnvWrite, ScriptErrorKind, ScriptLog, ScriptRequest,
    ScriptResponse,
};

/// 一段脚本跑完后的产出。
#[derive(Debug, Clone, Default)]
pub struct ScriptEffects {
    /// 脚本可能改过的请求。失败时已回滚到脚本执行前。
    pub request: ScriptRequest,
    /// 脚本写下的环境变量。失败时为空——失败脚本的副作用不落库。
    pub env_writes: Vec<EnvWrite>,
    pub logs: Vec<ScriptLog>,
    pub assertions: Vec<AssertionResult>,
}

/// 一段脚本的失败。
#[derive(Debug, Clone)]
pub struct ScriptFailure {
    pub kind: ScriptErrorKind,
    pub message: String,
}

/// 一段脚本的完整结果。失败不抛异常——脚本崩溃不得影响主请求的成败。
#[derive(Debug, Clone)]
pub struct ScriptRun {
    pub effects: ScriptEffects,
    pub failure: Option<ScriptFailure>,
}

/// 沙箱。持有一次执行所需的全部状态。
pub struct Sandbox {
    /// 上下文。它内部持有一份运行时的克隆，故运行时比它活得久——中断处理挂在运行时上，
    /// 随运行时一起活着。
    context: Context,
    /// 中断处理被触发过没有。引擎对「超时」的报错形态不保证，故用这个旗标判定。
    interrupted: Rc<Cell<bool>>,
}

impl Sandbox {
    /// 建一个带上限的沙箱。
    ///
    /// 截止时刻在**这里**定下：整个阶段（整条脚本链）共享一份预算。
    pub fn new(limits: &ScriptLimits) -> Result<Self, ScriptFailure> {
        let runtime = Runtime::new().map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot create the script engine: {e}"),
        })?;
        // 内存上限在**使用自定义分配器时是 no-op**。本项目不开 `rust-alloc`，
        // 走的是 libc 分配器，故这一行真的生效——别去开那个 feature。
        runtime.set_memory_limit(limits.memory_bytes);
        // 显式设置栈上限：默认值虽已是 256 KB，但深递归的检出行为依赖它。
        runtime.set_max_stack_size(limits.stack_bytes);

        let interrupted = Rc::new(Cell::new(false));
        let flag = Rc::clone(&interrupted);
        let deadline = Rc::new(Cell::new(
            Instant::now() + Duration::from_millis(limits.timeout_ms),
        ));
        let until = Rc::clone(&deadline);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            if Instant::now() >= until.get() {
                // 返回 true 让引擎抛一个**不可捕获**的异常：死循环真的被打断，
                // 而不是等它自己跑完。
                flag.set(true);
                true
            } else {
                false
            }
        })));

        let context = Context::full(&runtime).map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot create the script context: {e}"),
        })?;

        let sandbox = Self {
            context,
            interrupted,
        };
        // 前导是宿主 API 的全部实现。它自己抛错说明引擎坏了，不是用户脚本的问题。
        sandbox.eval(PRELUDE).map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot install the script prelude: {}", e.message),
        })?;
        Ok(sandbox)
    }

    /// 把输入注入沙箱：请求、响应、环境、限额。
    pub fn init(
        &self,
        request: &ScriptRequest,
        response: Option<&ScriptResponse>,
        env: &EnvSnapshot,
        limits: &ScriptLimits,
    ) -> Result<(), ScriptFailure> {
        let payload = serde_json::json!({
            "request": request_json(request),
            "response": response.map(response_json),
            "environment": {
                "id": env.id,
                "variables": env.variables,
            },
            "variables": {},
        });
        // 经 JSON 字符串字面量再 parse：JSON 文本直接内联进 JS 源码要操心
        // U+2028 / U+2029 之类的边角，转成字符串字面量则万无一失。
        let text = serde_json::to_string(&payload).map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot encode the script input: {e}"),
        })?;
        let literal = serde_json::to_string(&text).map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot encode the script input: {e}"),
        })?;
        let src = format!(
            "globalThis.__SP_INIT__(JSON.parse({}), {{\"maxLogs\":{},\"maxAssertions\":{}}});",
            literal, limits.max_logs, limits.max_assertions
        );
        self.eval(&src).map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot initialise the script context: {}", e.message),
        })
    }

    /// 跑一段脚本。
    ///
    /// `request_before` 是脚本执行前的请求：**失败时用它回滚**，故部分生效的请求
    /// 不会出现——那比完全没生效更难排查。
    pub fn run(&self, script: &str, request_before: &ScriptRequest) -> ScriptRun {
        let outcome = self.eval(script);
        let drained = self.drain();

        let mut effects = ScriptEffects {
            request: request_before.clone(),
            env_writes: drained.env_writes,
            logs: drained.logs,
            assertions: drained.assertions,
        };

        let failure = match outcome {
            Ok(()) => {
                effects.request = merge_request(request_before, drained.request);
                None
            }
            Err(failure) => {
                // 失败脚本的副作用不落库：变量写入丢弃，诊断（日志 / 断言）保留。
                effects.env_writes.clear();
                // 引擎里的请求状态也要换回来，否则下一段脚本会看到失败脚本的改动。
                let _ = self.restore_request(request_before);
                Some(failure)
            }
        };

        ScriptRun { effects, failure }
    }

    /// 把沙箱里的请求状态换回给定的那一份（回滚失败脚本的改动）。
    fn restore_request(&self, request: &ScriptRequest) -> Result<(), ScriptFailure> {
        let payload = serde_json::to_string(&request_json(request)).map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot encode the request: {e}"),
        })?;
        let literal = serde_json::to_string(&payload).map_err(|e| ScriptFailure {
            kind: ScriptErrorKind::Runtime,
            message: format!("cannot encode the request: {e}"),
        })?;
        self.eval(&format!(
            "globalThis.__SP_SET_REQUEST__(JSON.parse({}));",
            literal
        ))
    }

    /// 执行一段源码，把 JS 异常翻译成 [`ScriptFailure`]。
    fn eval(&self, source: &str) -> Result<(), ScriptFailure> {
        self.context.with(|ctx| match ctx.eval::<Value, _>(source) {
            Ok(_) => Ok(()),
            Err(err) => Err(self.classify(&ctx, err)),
        })
    }

    /// 把引擎抛出的东西归类成超时 / 内存 / 栈 / 运行时错误。
    fn classify(&self, ctx: &rquickjs::Ctx, err: rquickjs::Error) -> ScriptFailure {
        // 中断处理触发过 → 一定是超时。引擎对这种异常的形态不保证，故用旗标判定。
        if self.interrupted.get() {
            return ScriptFailure {
                kind: ScriptErrorKind::Timeout,
                message: "script exceeded its time limit and was interrupted".to_string(),
            };
        }
        // 只有「脚本抛了东西」才有可读的异常值；其余是引擎自己的错误。
        let rquickjs::Error::Exception = err else {
            return ScriptFailure {
                kind: ScriptErrorKind::Runtime,
                message: err.to_string(),
            };
        };
        let value = ctx.catch();
        if value.is_uncatchable_error() {
            return ScriptFailure {
                kind: ScriptErrorKind::Timeout,
                message: "script exceeded its time limit and was interrupted".to_string(),
            };
        }
        let text = value_text(&value);
        let lowered = text.to_lowercase();
        // 引擎的两种措辞都要认：QuickJS-NG 抛 "Maximum call stack size exceeded"，
        // 上游 QuickJS 抛 "stack overflow"。
        let kind = if lowered.contains("out of memory") {
            ScriptErrorKind::Memory
        } else if lowered.contains("stack overflow")
            || lowered.contains("maximum call stack size exceeded")
        {
            ScriptErrorKind::Stack
        } else {
            ScriptErrorKind::Runtime
        };
        ScriptFailure {
            kind,
            message: text,
        }
    }

    /// 取走本段脚本的产出并清空累积区。
    fn drain(&self) -> Drained {
        let text = self
            .context
            .with(|ctx| -> rquickjs::Result<Option<String>> {
                let value: Value = ctx.eval("globalThis.__SP_DRAIN__()")?;
                Ok(ctx
                    .json_stringify(value)?
                    .map(|s| s.to_string().unwrap_or_default()))
            });
        let text = match text {
            Ok(Some(t)) => t,
            _ => String::new(),
        };
        // 读不出来意味着引擎状态不对；当作「这段脚本没产出」比 panic 好。
        serde_json::from_str::<Drained>(&text).unwrap_or_default()
    }
}

/// 沙箱吐回来的东西。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Drained {
    #[serde(default)]
    request: Option<Json>,
    #[serde(default)]
    env_writes: Vec<EnvWrite>,
    #[serde(default)]
    logs: Vec<ScriptLog>,
    #[serde(default)]
    assertions: Vec<AssertionResult>,
}

/// 把请求转成沙箱看得懂的 JSON 形状。header 折叠成对象（同名取最后一个）。
fn request_json(request: &ScriptRequest) -> Json {
    serde_json::json!({
        "method": request.method,
        "url": request.url,
        "headers": headers_map(&request.headers),
        "body": request.body,
    })
}

fn response_json(response: &ScriptResponse) -> Json {
    serde_json::json!({
        "code": response.code,
        "status": response.status,
        "headers": headers_map(&response.headers),
        "body": response.body,
        "responseTime": response.response_time_ms,
    })
}

/// header 列表 → 对象。同名取最后一个：脚本看到的是一个名字一个值。
fn headers_map(headers: &[(String, String)]) -> Map<String, Json> {
    let mut map = Map::new();
    for (k, v) in headers {
        map.insert(k.clone(), Json::String(v.clone()));
    }
    map
}

/// 沙箱改过的请求 → 请求。header 没被改过就原样保留（含重名与顺序）。
fn merge_request(before: &ScriptRequest, after: Option<Json>) -> ScriptRequest {
    let Some(after) = after else {
        return before.clone();
    };
    let Some(obj) = after.as_object() else {
        return before.clone();
    };
    let headers = obj
        .get("headers")
        .and_then(|h| h.as_object())
        .cloned()
        .unwrap_or_default();
    // 脚本没碰过 header：原列表原样保留。HTTP 允许重名 header，折叠会让请求变形。
    // 只换 header，URL / method / body 仍取脚本改过的值。
    let headers = if headers == headers_map(&before.headers) {
        before.headers.clone()
    } else {
        headers_to_list(&headers, &before.headers)
    };
    ScriptRequest {
        method: obj
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or(&before.method)
            .to_string(),
        url: obj
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or(&before.url)
            .to_string(),
        headers,
        body: match obj.get("body") {
            Some(Json::String(s)) => Some(s.clone()),
            _ => before.body.clone(),
        },
    }
}

/// 沙箱的 header 对象 → 有序对列表。原顺序优先，新名字追加。
fn headers_to_list(
    headers: &Map<String, Json>,
    original: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::with_capacity(headers.len());
    let mut seen: HashSet<String> = HashSet::new();
    for (k, _) in original {
        if !seen.insert(k.clone()) {
            continue;
        }
        if let Some(v) = headers.get(k) {
            out.push((k.clone(), v.as_str().unwrap_or_default().to_string()));
        }
    }
    for (k, v) in headers {
        if seen.contains(k) {
            continue;
        }
        out.push((k.clone(), v.as_str().unwrap_or_default().to_string()));
    }
    out
}

/// 把 JS 值转成可读文本。用 `Coerced` 走 JS 的 `ToString` 语义——`Error` 对象的
/// `message` 不在可枚举属性里，JSON 序列化拿不到它。
fn value_text(value: &Value) -> String {
    match value.get::<rquickjs::Coerced<String>>() {
        Ok(text) => text.as_ref().clone(),
        Err(_) => "script error".to_string(),
    }
}
