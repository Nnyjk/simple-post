//! 稳定请求指纹：判断「这两处是不是同一个接口」。
//!
//! 算在 [`IrEndpoint`] 上而非 `EndpointDetail` 上，是两种导入器互相认得的**唯一**前提：
//! 同一份接口用 Postman 导一次、OpenAPI 导一次，若各按自己的格式算，第二份就认不出第一份。
//! 落在 IR 上，两条路先汇成同一棵树，指纹才有可比的对象。
//!
//! ## 排除什么，为什么
//!
//! 指纹回答的是「这是不是同一个接口」，不是「这两份配置逐字节一样吗」。故取
//! **method + path + 启用的 query 参数 + 启用的路径参数 + body**：
//!
//! - **name / description / tags** —— 改名不该造出副本；用户改个名再导一次应当被认出来。
//! - **auth** —— 认证方案随环境变，同一个接口在两份文件里用不同的 auth 是常态。
//! - **headers** —— 传输层的修饰（Accept、追踪 id），不是接口的身份。
//! - **module / collection 的位置** —— 外来格式的分组与本仓库的模块没有对应关系，
//!   Postman 的 folder 和 simple-post 的模块是两套命名，凭位置去重两边都不认。
//!
//! **body 反过来要算进去**：一组 Postman 集合里 `POST /users` 配两份不同 payload 是常见写法，
//! 排除 body 会把第二个判成重复而跳过——那是丢请求，不是去重。两害相权，宁可在用户改过
//! body 后再导一次时多出一个**看得见**的副本（预览会报数量与重名），也不要看不见的丢失。
//! 代价是 JSON 空白差异会影响命中，故同一份文件的重复导入仍然可靠，跨文件的 body 命中要逐字节相同。

use std::collections::HashMap;

use simple_post_core::domain::{BodyMode, EndpointDetail, HttpMethod, KeyValue, RequestBody};

use crate::ir::IrEndpoint;

/// 一个请求的身份部分。与 [`IrEndpoint`] 分开，是因为去重要拿它跟**已落库**的行比，
/// 而那是 `EndpointDetail`——两边各写一份字段表就会漂。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIdentity {
    /// 大写动词。`HttpMethod::as_str` 对 `Custom` 原样返回，落到这里统一一次。
    pub method: String,
    /// 归一化后的路径。
    pub path: String,
    /// 启用的 query 参数，按 `key=value` 排序，故书写顺序不影响身份。
    pub query: Vec<String>,
    /// 启用的路径参数（URL 里那些 `{占位符}` 的值），同样按 `key=value` 排序。
    ///
    /// **值要算进去**，理由与 body 一样：同一组接口里 `/users/{id}` 配两个不同的 id
    /// 是常见写法，排除值会把第二个判成重复而跳过——那是丢请求，不是去重。
    pub path_params: Vec<String>,
    /// body 的身份片段：模式 + 原文。模式要算进去——`raw` 的 `{` 与 `json` 的 `{`
    /// 发出去是两种请求。
    pub body: String,
}

impl RequestIdentity {
    pub fn of_ir(ep: &IrEndpoint) -> Self {
        Self::new(&ep.method, &ep.path, &ep.params, &ep.path_params, &ep.body)
    }

    /// 已落库的行。落库只补 id / `root_id` / 时间戳，不碰这几个字段，
    /// 故同一份请求在两侧算出同一个身份。
    pub fn of_detail(d: &EndpointDetail) -> Self {
        Self::new(&d.method, &d.url, &d.params, &d.path_params, &d.body)
    }

    fn new(
        method: &HttpMethod,
        path: &str,
        params: &[KeyValue],
        path_params: &[KeyValue],
        body: &RequestBody,
    ) -> Self {
        let mut query: Vec<String> = params
            .iter()
            .filter(|p| p.enabled)
            .map(|p| format!("{}={}", p.key, p.value))
            .collect();
        query.sort();
        let mut path_values: Vec<String> = path_params
            .iter()
            .filter(|p| p.enabled)
            .map(|p| format!("{}={}", p.key, p.value))
            .collect();
        path_values.sort();
        Self {
            method: method.as_str().to_ascii_uppercase(),
            path: normalize_path(path),
            query,
            path_params: path_values,
            body: format!("{}:{}", body_mode_tag(&body.mode), body.content),
        }
    }

    /// 指纹本体：FNV-1a 128 位，取前 16 个十六进制字符。
    ///
    /// 刻意不用 `std::collections::hash_map::DefaultHasher`：它明确不保证跨版本稳定，
    /// 换个 Rust 版本重编就会让全部历史指纹失效——而指纹的用处恰恰是跨时间比对。
    /// 也刻意不用 `sha2`：为一个短 id 拖一个哈希依赖不划算，这里要的只是稳定的短标识。
    pub fn fingerprint(&self) -> String {
        const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
        const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

        let mut hash = OFFSET;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                hash ^= *b as u128;
                hash = hash.wrapping_mul(PRIME);
            }
        };
        eat(self.method.as_bytes());
        eat(b"\n");
        eat(self.path.as_bytes());
        for q in &self.query {
            eat(b"\n");
            eat(q.as_bytes());
        }
        eat(b"\n");
        eat(self.body.as_bytes());
        format!("{hash:032x}")[..16].to_string()
    }
}

/// `BodyMode` 的线上取值。它没有 `as_str()`（不像 `AuthType`），而为一个短标签
/// 引入序列化不值得，故就地匹配。**要跟着 `domain::BodyMode` 的变体一起改**——
/// 漏一个变体不会编译失败，只会让该模式的 body 不参与身份区分。
fn body_mode_tag(mode: &BodyMode) -> &'static str {
    match mode {
        BodyMode::None => "none",
        BodyMode::Json => "json",
        BodyMode::Form => "form",
        BodyMode::Raw => "raw",
        BodyMode::Multipart => "multipart",
    }
}

/// 路径归一化。外来格式在同一个接口上给出不同字面量是常态，不归一化就等于不去重：
///
/// - 补前导 `/` —— Postman 的 `raw` 常写成 `users`，OpenAPI 的 path 键自带 `/`。
/// - 合并重复斜杠 —— `{{baseUrl}}//users` 与 `{{baseUrl}}/users` 是同一个接口。
/// - 去尾斜杠 —— `/users` 与 `/users/` 落到多数服务端等价，仍是两个字符串。
/// - **不**折叠大小写 —— 路径大小写在 HTTP 里是有意义的。
fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    let with_slash = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };

    let mut out = String::with_capacity(with_slash.len());
    let mut last_was_slash = false;
    for ch in with_slash.chars() {
        if ch == '/' {
            if !last_was_slash {
                out.push('/');
            }
            last_was_slash = true;
        } else {
            out.push(ch);
            last_was_slash = false;
        }
    }

    // 根路径 `/` 不去尾斜杠，否则会变成空串。
    if out.len() > 1 {
        out.trim_end_matches('/').to_string()
    } else {
        out
    }
}

/// 一次导入内的指纹账本。跨导入识别靠它：`commit` 先把目标项目里**已落库**的
/// endpoint 全部灌进来，再逐个问 [`Self::seen`]。
#[derive(Debug, Default, Clone)]
pub struct FingerprintSet {
    seen: HashMap<String, RequestIdentity>,
}

impl FingerprintSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 灌入一批已落库的行。
    pub fn absorb_details<'a>(&mut self, details: impl IntoIterator<Item = &'a EndpointDetail>) {
        for d in details {
            let id = RequestIdentity::of_detail(d);
            self.seen.insert(id.fingerprint(), id);
        }
    }

    /// 这个请求是不是已经见过。见过的同时记下，故同一个 IR 内部重复的条目
    /// 第二次就会被认出来——一个 Postman 文件里重复粘贴的请求不该落两行。
    pub fn seen(&mut self, id: &RequestIdentity) -> bool {
        let fp = id.fingerprint();
        if self.seen.contains_key(&fp) {
            return true;
        }
        self.seen.insert(fp, id.clone());
        false
    }

    /// 只问，不记。**预览走这条**：预览要标出「这些条目在项目里已经有了」，标完之后
    /// 同一批数据还要原样交给落库方，那里的 [`Self::seen`] 才负责去重。预览若用
    /// `seen`，一份预览就会把本次导入的账先记上一遍，于是落库时第一个条目就被自己的
    /// 预览当成重复跳过。
    pub fn contains(&self, id: &RequestIdentity) -> bool {
        self.seen.contains_key(&id.fingerprint())
    }
}

// 测试——指纹的输入与排除项。

#[cfg(test)]
#[path = "fingerprint_test.rs"]
mod tests;
