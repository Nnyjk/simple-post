//! 两个二进制共用的 CLI 片段。
//!
//! 桌面二进制 `simple-post` 与容器二进制 `simple-post-server` 都有 `serve-web` 与
//! `mcp` 两个子命令，两边的开关名、默认值与启动顺序必须一致 —— 它们服务的是同一个
//! REST + MCP 面。故这段解析与启动流程住在这里，两个 `main` 各调一次。

use crate::domain::McpHttpConfig;

/// 把 `--allowed-*` 的 CLI 列表规范成真正的 `Vec<String>`。
///
/// `None`（flag 缺席）与 `Some("")`（supervisor 对空列表发的 `--allowed-hosts=`）
/// 都必须落到空列表，绝不能是单元素 `[""]`。空 host 白名单意味着 daemon *拒绝*所有
/// 非 loopback 的 `Host`，故一个多余的空项既会漏进持久化配置，又会变成永不匹配的
/// 规则。
pub fn split_allow_list(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `mcp` 子命令的全部开关。
pub struct McpOptions {
    pub transport: String,
    pub port: u16,
    pub bind: String,
    pub path: String,
    pub allow_remote: bool,
    pub allowed_hosts: Option<String>,
    pub allowed_origins: Option<String>,
}

/// 起 MCP daemon。监听配置由本函数按 CLI 开关拼出。
pub fn run_mcp(opts: McpOptions) {
    let http_config = McpHttpConfig {
        host: opts.bind.clone(),
        port: opts.port,
        path: opts.path,
        allow_remote: opts.allow_remote,
        allowed_hosts: split_allow_list(opts.allowed_hosts),
        allowed_origins: split_allow_list(opts.allowed_origins),
    };
    crate::run_mcp(opts.transport, opts.port, opts.bind, http_config);
}

/// `serve-web` 的全部开关。
///
/// 打包成一个结构体再往下传：位置参数排到八个之后，调用点只能靠顺序认哪一个
/// 是端口、哪一个是口令，而 `Option<bool>` 这种「`None` 走持久化值、`Some`
/// 走本次覆盖」的开关混在中间尤其容易接错位。
pub struct ServeWebOptions {
    pub bind: String,
    pub port: u16,
    pub spa_dir: Option<String>,
    pub data_dir: Option<String>,
    pub mcp_auto_start: Option<bool>,
    pub web_password: Option<String>,
    pub public_base_path: Option<String>,
}

/// 把 `serve-web` 的 flag 解析成具体路径并跑起服务器。
pub fn serve_web(opts: ServeWebOptions) {
    let ServeWebOptions {
        bind,
        port,
        spa_dir,
        data_dir,
        mcp_auto_start,
        web_password,
        public_base_path,
    } = opts;
    let bind_addr: std::net::SocketAddr = match format!("{bind}:{port}").parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[serve-web] invalid --bind/--port: {e}");
            std::process::exit(2);
        }
    };
    let spa_path = match spa_dir {
        Some(p) => std::path::PathBuf::from(p),
        None => crate::web_api::default_spa_dir(),
    };
    let data_path = match data_dir {
        Some(p) => std::path::PathBuf::from(p),
        None => match crate::web_api::default_data_dir() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[serve-web] cannot resolve data dir: {e:?}");
                std::process::exit(2);
            }
        },
    };

    // CLI flag 存在时覆盖持久化的那一行。
    //
    // `None` 原样向上传而不在此解析：`web_api::serve` 会用它自己已经打开的连接池读
    // 持久化值，在这里另开一个只为读一个 preference，等于对同一个文件付第二次 schema
    // 检查与第二个连接池。
    //
    // 但显式 flag 仍经这里把覆盖值写回，使强制值照旧被持久化。
    let auto_start = match mcp_auto_start {
        Some(v) => {
            if let Ok(pool) = crate::db::init(&data_path) {
                let _ = crate::db::with_tx(&pool, |c| {
                    crate::db::preferences::set(c, crate::db::preferences::keys::MCP_AUTO_START, &v)
                });
            }
            Some(v)
        }
        None => None,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("[serve-web] failed to build tokio runtime");
    runtime.block_on(crate::run_serve_web(
        bind_addr,
        spa_path,
        data_path,
        auto_start,
        web_password,
        public_base_path,
    ));
}

/// CLI 子命令 `mcp-proxy` 的入口。proxy 是独立二进制
/// （`simple-post-mcp[.exe]`）；用主二进制跑它几乎肯定是用户搞错了（PATH 不对、
/// 快捷方式指错），故指路到正确的二进制并以非零退出，好让 CI 脚本能抓到。
pub fn run_mcp_proxy() {
    eprintln!(
        "mcp-proxy is a separate binary:\n  \
         - build it from source: cargo install --path crates/simple-post-mcp\n  \
           (run that in the repository root; it installs the `simple-post-mcp` binary)\n  \
         - the published release assets are desktop installers only, so that\n  \
           binary is not among them\n  \
         - or connect the client to the daemon over HTTP instead"
    );
    std::process::exit(2);
}
