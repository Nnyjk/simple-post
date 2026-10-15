//! GUI 用来发现「崩溃但留下端口文件」的 daemon 的 HTTP 探测。
//!
//! 路径写死为 `/mcp/healthz`：daemon 即使在自定义挂载前缀下也保留这个别名，改名会
//! 悄悄打断 supervisor 的存活探测。

use std::time::Duration;

/// GUI 用来发现「崩溃但留下端口文件」的 daemon 的 HTTP 探测。2 秒超时——既能跨过
/// 启动时 CPU 繁忙的瞬间，又不至于让渲染进程在启动画面上明显卡住。
pub async fn healthz(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/mcp/healthz");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(target: "port_registry", "healthz client build: {e}");
            return false;
        }
    };
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(e) => {
            tracing::debug!(target: "port_registry", "healthz {url}: {e}");
            false
        }
    }
}
