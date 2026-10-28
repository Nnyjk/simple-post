//! 用户可配置的设置单例：出站请求旋钮、MCP HTTP 监听配置、逐项目 agent 权限。

use serde::{Deserialize, Serialize};

// agent 权限

/// 逐项目的 MCP 权限。`Read < ReadCall < Full`；序关系本身在
/// [`crate::permissions::permission_at_least`]。
///
/// 用 `snake_case` 而非 `lowercase`：后者把 `ReadCall` 映射成 `"readcall"`，一个既不匹配
/// [`Self::as_str`]（`db::permissions` 写的、`parse` 读的），也不匹配 `agent_permissions`
/// 的 CHECK 约束、更不匹配 TypeScript 侧 `AgentPermission` 联合类型的第三种拼法。
/// `snake_case` 让四处一致，且与其兄弟 [`crate::permissions::CapabilityPolicy`] 相同。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Read,
    #[default]
    ReadCall,
    Full,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Read => "read",
            Permission::ReadCall => "read_call",
            Permission::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Permission::Read),
            "read_call" => Some(Permission::ReadCall),
            "full" => Some(Permission::Full),
            _ => None,
        }
    }
}

// 请求设置

/// HTTPS 出站的一组开关与证书位置。
///
/// **证书只存文件路径，不存内容。** 私钥一旦进库，`GET /api/preferences` 会把整行原样
/// 返回（Web 形态下那是给任何持有 bearer 口令的客户端看的），而 SQLite 文件本身也是一份
/// 用户没预期的备份。路径则随时可以被删掉或换掉，这是可接受的。
///
/// 路径由**运行请求的那台机器**解析：桌面端是本机路径，serve-web / Docker 端是服务端容器
/// 里的路径，故 Web 形态下运维需要把证书挂进容器才能用。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TlsSettings {
    /// 跳过服务端证书校验，主机名一并跳过。
    ///
    /// 关闭时服务端证书的过期、自签名、签发者一律不查，故这条请求会被任何一个中间人
    /// 截获与改写。默认值是 `false`——**只有用户明确打开才生效**。
    pub skip_verify: bool,
    /// 自定义 CA 证书的 PEM 文件路径。空 = 用 `reqwest` 内置的公共根证书集。
    ///
    /// 一个 PEM 文件里可以含多张证书（bundle），`reqwest` 会把它们逐张加进信任链。
    pub ca_certificate_path: Option<String>,
    /// 客户端证书的 PEM 文件路径（mTLS）。空 = 不挂客户端证书。
    pub client_certificate_path: Option<String>,
    /// 客户端私钥的 PEM 文件路径。空 = 从 `client_certificate_path` 指向的文件里一起读。
    ///
    /// 单独给私钥路径而无证书路径是非法组合：客户端证书要连着私钥一起出示，只有私钥
    /// 发不出去。
    pub client_key_path: Option<String>,
}

impl TlsSettings {
    /// 这组设置是否会让某次请求挂上客户端证书。
    pub fn has_client_certificate(&self) -> bool {
        self.client_certificate_path
            .as_ref()
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false)
    }

    /// 这组设置是否会让某次请求带上自定义 CA 证书。
    pub fn has_ca_certificate(&self) -> bool {
        self.ca_certificate_path
            .as_ref()
            .map(|p| !p.trim().is_empty())
            .unwrap_or(false)
    }

    /// 校验这三个路径字段：它们必须真的是路径，不能是粘进来的证书正文。
    ///
    /// 类型层面已经挡住了应用自己写证书内容（这三个字段只有路径），但挡不住**用户**把
    /// 私钥正文粘进「客户端私钥」那个输入框 —— 那段文本会原样落进
    /// `preferences` 的 `request_settings` 行或 `endpoint_details.tls_json`，而
    /// `GET /api/preferences` 把整行原样返回。这正是「证书只存路径，不存内容」要防的
    /// 暴露面，故在这里把那条口子堵上。
    ///
    /// 判据：值里出现 PEM 的区块标记，或出现换行 —— 路径两者都不会有。
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        for (label, value) in [
            ("自定义 CA 证书", &self.ca_certificate_path),
            ("客户端证书", &self.client_certificate_path),
            ("客户端私钥", &self.client_key_path),
        ] {
            let Some(raw) = value.as_deref() else {
                continue;
            };
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.contains("-----BEGIN") || trimmed.contains("-----END") {
                return Err(crate::error::AppError::Validation(format!(
                    "{label}填的是证书正文，不是文件路径。证书只存路径：把 PEM 文件存到磁盘上，\
                     这里填那个文件的路径"
                )));
            }
            if trimmed.contains('\n') || trimmed.contains('\r') {
                return Err(crate::error::AppError::Validation(format!(
                    "{label}的路径里不能有换行。它是一个文件路径，不是一段文本"
                )));
            }
        }
        Ok(())
    }
}

/// 出站请求旋钮。单例，作为 JSON blob 存在 `preferences` KV 表里，而非自成一表。
///
/// 容器级 `#[serde(default)]` 的分量在**读**侧而非写侧：存下的 blob 是某个更早构建写的，
/// 故此日新增第四个字段时，每个既有安装的行都能缺它反序列化——否则
/// `request_settings_get` 会让它们全部失败。代价落在写侧：`PUT /api/request_settings`
/// 是整体替换，缺字段的请求体会把该字段重置为默认值而不是被拒。对一个默认值写在
/// [`Default`] 上的设置单例，这更有用；该路由自己的注释也这么写。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RequestSettings {
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
    /// HTTPS 出站的**全局默认**。端点可以整体覆盖它，见 [`crate::tls::resolve`]。
    pub tls: TlsSettings,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            follow_redirects: true,
            max_response_size_kb: 5120,
            tls: TlsSettings::default(),
        }
    }
}

// MCP HTTP 监听配置

/// MCP HTTP 守护进程监听在哪、应答谁。
///
/// 设置单例，作为 JSON blob 存在 `preferences` 的
/// [`crate::db::preferences::keys::MCP_HTTP_CONFIG`] 之下——与所有其他结构化设置同属
/// 「preferences 袋子装 JSON 值」这条规则。
///
/// 容器级 `#[serde(default)]` 的分量在**读**侧，理由同 [`RequestSettings`]：存下的 blob 是
/// 更早构建写的，后来新增的字段不能让每个既有安装的行解析失败。代价落在写侧——缺字段会
/// 把该字段重置为默认值而不是被拒——对设置单例而言这更有用。
///
/// 与安全相关的是 `allow_remote` 加那两张允许表，且它们**结构上 fail-closed**：默认值
/// （`allow_remote: false`，两表皆空）只接受 loopback `Host`，且完全不接受任何 `Origin`。
/// `validate` 拒绝唯一那个自相矛盾的组合——非 loopback 绑定却不开远程访问，那会监听在
/// 局域网上而守卫挡掉所有真正从那里到达的请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct McpHttpConfig {
    /// 绑定地址。`127.0.0.1` 即守护进程仅本机可达。
    pub host: String,
    /// 监听端口。`0` 表示「由 OS 分配」——守护进程自己选定的端口随后经 `mcp.port` 记录公布。
    pub port: u16,
    /// JSON-RPC 端点的挂载路径。必须以 `/` 开头。
    pub path: String,
    /// 非 loopback 访问的总开关。`false`（默认）表示 `Host` 守卫只接受 loopback 名字。
    pub allow_remote: bool,
    /// 开启 `allow_remote` 后接受的 `Host` 值。裸 `"*"` 表示任意。空表示**不接受**任何
    /// 非 loopback host——不是「全都接受」。
    pub allowed_hosts: Vec<String>,
    /// 接受的 `Origin` 值。AI 客户端（curl、Claude Code）根本不发 `Origin`，故永不被此拦；
    /// 确实发了的浏览器式请求必须匹配。裸 `"*"` 表示任意。
    pub allowed_origins: Vec<String>,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_MCP_HOST.to_string(),
            port: 0,
            path: DEFAULT_MCP_PATH.to_string(),
            allow_remote: false,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
        }
    }
}

/// MCP HTTP 守护进程的默认监听地址——只监听回环地址。
pub const DEFAULT_MCP_HOST: &str = "127.0.0.1";
/// MCP JSON-RPC 端点的默认挂载路径。
pub const DEFAULT_MCP_PATH: &str = "/mcp";

impl McpHttpConfig {
    /// axum 路由器想要的挂载路径形态：恒有前导 `/`，恒无尾随 `/`。`"/"`、`""` 与缺斜杠一律
    /// 收敛到 [`DEFAULT_MCP_PATH`]，而不是产出一个把设置面板里看得见的东西全都 404 的路由器。
    pub fn normalized_path(&self) -> String {
        let raw = self.path.trim();
        let trimmed = raw.trim_end_matches('/');
        if trimmed.is_empty() {
            return DEFAULT_MCP_PATH.to_string();
        }
        if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            format!("/{trimmed}")
        }
    }

    /// 拒绝无法自圆其说的配置。
    /// 每次写入都调用（Tauri command 与 REST 路由皆然），故自相矛盾的组合永远到不了
    /// `daemon_cli_args` 生成时读取的那一行 `preferences`。
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        use crate::error::AppError;

        if self.host.trim().is_empty() {
            return Err(AppError::Validation("host must not be empty".into()));
        }
        // 先归一化再检查，好让 `/mcp/` 与 `/mcp` 不会双双通过验证、随后在路由器上打架。
        if self.path.trim().is_empty() {
            return Err(AppError::Validation(format!(
                "path must not be empty (expected something like {DEFAULT_MCP_PATH:?})"
            )));
        }
        if !self.normalized_path().starts_with('/') {
            return Err(AppError::Validation(format!(
                "path must start with '/' (got {:?})",
                self.path
            )));
        }
        // 那个自相矛盾的组合：绑到局域网，守卫却配置为拒掉一切非 loopback——守护进程将不可达，
        // 读起来是「MCP 坏了」而不是「你的设置自相矛盾」。
        if !self.allow_remote && !is_loopback_host(&self.host) {
            return Err(AppError::Validation(format!(
                "host {:?} is not a loopback address, so allowRemote must also be \
                 enabled — otherwise the daemon listens on a network the Host guard \
                 refuses to answer",
                self.host
            )));
        }
        Ok(())
    }
}

/// `host` 是否是守卫恒接受的那几个 loopback 名字之一？
/// 由 [`McpHttpConfig::validate`] 与守护进程的 Host 守卫共用——两者必须一致，否则一份配置
/// 能通过验证、随后又把监听者挡在自己的 healthz 探针之外。
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}
