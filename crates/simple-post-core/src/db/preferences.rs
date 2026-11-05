//! 自由键值对偏好袋子。
//!
//! 用于不适合结构化列的渲染层偏好（上次打开的项目 id、主题模式、最近用的环境 id 等）。
//! 值经 JSON 编码，故 schema 保持扁平——JSON 形状由 [`get`] / [`set`] 上的类型参数决定。
//! 渲染层即 schema，故此处没有 DB 类型也没有领域类型。

use rusqlite::{params, Connection};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::AppError;

// 读。

/// 按键读一个偏好。键不存在时为 `Ok(None)`，故调用方无需另发一次「是否存在」的查询就能退回默认值。
///
/// 存下的值对 `T` 而言不是合法 JSON 时为 `AppError::Db`。
pub fn get<T: DeserializeOwned>(conn: &mut Connection, key: &str) -> Result<Option<T>, AppError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM preferences WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .ok();
    match raw {
        Some(s) => Ok(Some(serde_json::from_str(&s)?)),
        None => Ok(None),
    }
}

// 写。

/// Upsert 一个偏好。值经 serde 做 JSON 编码，故任何 `Serialize` 类型都能存。
///
/// **`request_settings` 这一行要额外校验**：它带着 HTTPS 出站的三个证书路径字段，而
/// `GET /api/preferences` 把整行原样返回，用户把私钥正文粘进路径输入框，那段文本就会原样落进
/// 这一行。校验放在这里而不是各个写入口，是因为这一行有三条写路径（桌面端的专用命令、
/// `PUT /api/request-settings` 与 `PUT /api/preferences`），放在一处三条路径都拦得住。
///
/// - 读不出 [`crate::domain::RequestSettings`] 的值一律报错，不当作「这不是设置、照旧存下去」。
///   放过它就留下一条绕路：请求体里 `timeoutMs` 的类型写错让整体反序列化失败，校验根本没跑。
/// - 存重新序列化后的那一份，不是请求体原样。域类型不收陌生键，原样存会把调用方塞进来的陌生键
///   连同其中的证书正文留在这一行里。
pub fn set<T: Serialize>(conn: &mut Connection, key: &str, value: &T) -> Result<(), AppError> {
    let raw = serde_json::to_string(value)?;
    let raw = if key == keys::REQUEST_SETTINGS {
        let settings: crate::domain::RequestSettings = serde_json::from_str(&raw).map_err(|e| {
            AppError::Validation(format!(
                "request_settings 这一行的形状不对：{e}。这一行只存出站请求设置，\
                 字段名是 timeoutMs / followRedirects / maxResponseSizeKb / tls"
            ))
        })?;
        settings.tls.validate()?;
        serde_json::to_string(&settings)?
    } else {
        raw
    };
    conn.execute(
        "INSERT INTO preferences (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, raw],
    )?;
    Ok(())
}

pub fn delete(conn: &mut Connection, key: &str) -> Result<(), AppError> {
    conn.execute("DELETE FROM preferences WHERE key = ?1", params![key])?;
    Ok(())
}

/// 返回全部 (key, value) 对。渲染层启动时用它一次往返补水自己的内存 store。
pub fn list_all(conn: &mut Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn.prepare("SELECT key, value FROM preferences")?;
    let rows = stmt.query_map([], |r| {
        let k: String = r.get(0)?;
        let v: String = r.get(1)?;
        Ok((k, v))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// 约定俗成的键。

/// 全应用共享的偏好键。一律按 JSON 值存储，与 `preferences` 其余部分一致（编码见 `set`）。
/// 需要新的持久化设置时在此加一条，免得调用方到处写字符串字面量。
pub mod keys {
    /// `"ui.theme"` → string。允许："light" | "dark" | "system"。
    pub const THEME: &str = "ui.theme";

    /// `"mcp.auto_start"` → bool。**默认 `false`。**
    ///
    /// 守护进程是长驻后台进程，默认启动会让桌面应用的 Tauri `.setup()` 每次启动都拉起一个，
    /// 背后没有任何用户意图。默认停止使「MCP 服务器在跑」成为显式 opt-in；该键由桌面与
    /// `serve-web` 两种形态共用，故一个开关覆盖两者。
    pub const MCP_AUTO_START: &str = "mcp.auto_start";

    /// `"mcp.policy"` → string。**默认 `read_only`。** 用户在设置面板里选定的全局 MCP 能力档位
    /// （`"read_only"` / `"safe_write"` / `"full_access"`）。
    ///
    /// 行缺失即解析为 `read_only`，故在用户提高档位之前，AI 客户端可以 list 与 get，但不能改动
    /// 任何东西。
    pub const MCP_POLICY: &str = "mcp.policy";

    /// `"mcp.http_config"` → object。**默认 loopback `/mcp`，无远程访问。** MCP HTTP 守护进程监听在哪
    /// （`host`、`port`、`path`），以及它应答哪些 `Host` / `Origin`（`allowRemote`、
    /// `allowedHosts`、`allowedOrigins`）。
    ///
    /// 经 [`set`] 存为 **JSON 编码的** [`crate::domain::McpHttpConfig`]——编码不是可选项，手工写入的
    /// 裸对象在读取时解析失败，[`mcp_http_config`] 会把它报成错误而不是静默退回默认值。
    pub const MCP_HTTP_CONFIG: &str = "mcp.http_config";

    /// `"request_settings"` → object。**用户可配置的出站请求旋钮**（超时 / 跟随重定向 /
    /// 最大响应体积），值是 JSON 编码的 [`crate::domain::RequestSettings`]。
    ///
    /// 桌面端在设置面板里改的就是这一行，故 MCP 侧与界面用的是同一份取值。
    pub const REQUEST_SETTINGS: &str = "request_settings";

    /// `"response_history.keep"` → number。**每个 endpoint 保留多少条未收藏的发送历史。**
    ///
    /// 收藏的行不计入这个条数（见 `db::response_history::set_favorite`）。这是全局设置：一次修改对
    /// 所有项目、所有接口都生效，故它住在跨项目的 `preferences` 袋子里，而不是某个项目上。
    pub const RESPONSE_HISTORY_KEEP: &str = "response_history.keep";
}

/// 每个 endpoint 最多保留多少条未收藏历史的上限。
///
/// 用户在设置里能改这个数，但再大的值也没有意义：历史条带一次也显示不完那么多行，
/// 而每一行都在用户的 `store.db` 里占着地方。
pub const RESPONSE_HISTORY_KEEP_MAX: i64 = 500;

/// 读 `mcp.auto_start` 偏好，取文档化的默认值（`true`）。桌面 `.setup()` 与 Web 服务端的
/// 启动路径都用它，故两种形态对同一行达成一致。
///
/// 默认**开**：MCP 进程是 MCP 唯一的对外端点（主端口不再挂 `/mcp`），关掉它就没有任何
/// MCP 端点可连。容器重启后 MCP 静默消失的坑正是这样来的——偏好行缺失时读出 `false`。
pub fn mcp_auto_start(conn: &mut Connection) -> Result<bool, AppError> {
    let raw: Option<bool> = get(conn, keys::MCP_AUTO_START)?;
    Ok(raw.unwrap_or(true))
}

/// 把 `mcp.policy` 偏好读成 [`CapabilityPolicy`]。行缺失、或带着无法识别的字符串时退回
/// [`CapabilityPolicy::ReadOnly`]。
/// 行是用户可以直接改偏好表来篡改的线格式，未知档位不报错，是为了让将来的版本发新档位时
/// 不弄坏旧客户端。
pub fn mcp_policy(conn: &mut Connection) -> Result<crate::permissions::CapabilityPolicy, AppError> {
    let raw: Option<String> = get(conn, keys::MCP_POLICY)?;
    // 默认 ReadOnly。既涵盖缺行，也涵盖向前兼容的未知值——见文档注释。
    Ok(raw
        .as_deref()
        .and_then(crate::permissions::CapabilityPolicy::parse)
        .unwrap_or_default())
}

/// 持久化全局 MCP 能力档位。编码为线格式（`"read_only"` / `"safe_write"` /
/// `"full_access"`）——见 [`crate::permissions::CapabilityPolicy::as_str`]。
pub fn set_mcp_policy(
    conn: &mut Connection,
    p: crate::permissions::CapabilityPolicy,
) -> Result<(), AppError> {
    set(conn, keys::MCP_POLICY, &p.as_str())
}

/// 读 `mcp.http_config` 偏好，行缺失时退回 [`McpHttpConfig::default`]。
/// **存在但无法解析**的行是错误，不是回退：`preferences.value` 存的是 JSON 编码的值，手工写入的
/// 裸对象若被当作「缺失」，就会把一个暴露在局域网上的守护进程静默降级成仅 loopback。
pub fn mcp_http_config(conn: &mut Connection) -> Result<crate::domain::McpHttpConfig, AppError> {
    let raw: Option<crate::domain::McpHttpConfig> = get(conn, keys::MCP_HTTP_CONFIG)?;
    Ok(raw.unwrap_or_default())
}

/// 持久化 MCP HTTP 监听配置。
/// 走 [`set`]，由它跑 `serde_json::to_string`。手工写入裸对象正是本模块文档警告的那个
/// 失败：每个读者都跑 `from_str`，故未加引号的值永远读不回来。
pub fn set_mcp_http_config(
    conn: &mut Connection,
    config: &crate::domain::McpHttpConfig,
) -> Result<(), AppError> {
    set(conn, keys::MCP_HTTP_CONFIG, config)
}

/// 读用户可配置的出站请求旋钮，行缺失时退回 [`crate::domain::RequestSettings::default`]。
///
/// 桌面端在设置面板里改的就是这一行，故 MCP 侧与界面用的是同一份取值。
pub fn request_settings(conn: &mut Connection) -> Result<crate::domain::RequestSettings, AppError> {
    let raw: Option<crate::domain::RequestSettings> = get(conn, keys::REQUEST_SETTINGS)?;
    Ok(raw.unwrap_or_default())
}

/// 持久化出站请求旋钮。
///
/// TLS 那三个路径字段的校验在 [`set`] 里做：那一行有三条写路径（桌面端的专用命令、
/// `PUT /api/request-settings` 与 `PUT /api/preferences`），把校验放在 [`set`] 那一处，三条路径都
/// 拦得住。形状不对的值与域类型不收的陌生键也在那里处理。
pub fn set_request_settings(
    conn: &mut Connection,
    settings: &crate::domain::RequestSettings,
) -> Result<(), AppError> {
    set(conn, keys::REQUEST_SETTINGS, settings)
}

/// 读「每个 endpoint 保留多少条未收藏历史」，行缺失时退回缺省值。
///
/// 读不出来（行里的值不是合法 JSON）时同样退回缺省值：这个数只是裁剪历史的上限，退到缺省值
/// 比让每一次发送都写不进历史更合算。
pub fn response_history_keep(conn: &mut Connection) -> Result<i64, AppError> {
    let raw: Option<i64> = match get(conn, keys::RESPONSE_HISTORY_KEEP) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "preferences: response_history.keep is unreadable, using the default: {e}"
            );
            None
        }
    };
    Ok(raw
        .unwrap_or(crate::domain::RESPONSE_HISTORY_KEEP_DEFAULT)
        .clamp(1, RESPONSE_HISTORY_KEEP_MAX))
}

/// 持久化那个条数。写入前先夹到 `[1, RESPONSE_HISTORY_KEEP_MAX]`。
pub fn set_response_history_keep(conn: &mut Connection, keep: i64) -> Result<(), AppError> {
    set(
        conn,
        keys::RESPONSE_HISTORY_KEEP,
        &keep.clamp(1, RESPONSE_HISTORY_KEEP_MAX),
    )
}

// 测试。

#[cfg(test)]
#[path = "preferences_test.rs"]
mod tests;
