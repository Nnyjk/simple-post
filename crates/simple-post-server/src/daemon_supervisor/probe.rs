//! 健康探测，以及它喂给的那个幂等判定。

use super::status::DaemonStatus;

#[cfg(feature = "gui")]
use crate::port_registry;

#[cfg(feature = "gui")]
use super::McpDaemonHandle;

/// 纯函数：一次「启动」请求是否已被满足。
///
/// 手动 ON 开关的幂等规则：若监管者跟踪的 daemon 仍能答 healthz，就返回该 daemon 的身份而不
/// kill 重拉。`tracked_port.is_some()` 必须与状态同时成立——`Running` 但无跟踪端口是「已收养、
/// 随后丢了端口记录」的状态，此处无法确认存活，而重新 spawn 才是唯一诚实的答案。
// 纯决策函数且不含 Tauri，故始终参与编译——`daemon_supervisor_test.rs` 直接测它。
// 它唯一的生产调用方（`is_serving`）受 GUI feature 门控，故在无 GUI 的 feature 上
// 挂 `allow(dead_code)`。
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub(super) fn start_is_noop(
    status: DaemonStatus,
    tracked_port: Option<u16>,
    healthy: bool,
) -> bool {
    status == DaemonStatus::Running && tracked_port.is_some() && healthy
}

#[cfg(feature = "gui")]
impl McpDaemonHandle {
    /// 当前跟踪的 daemon 是否仍在服务？探测在锁外跑（它是一次 HTTP 往返），判断本身
    /// 是纯函数 [`start_is_noop`]。
    pub(super) async fn is_serving(&self) -> bool {
        let (status, port, bind) = {
            let g = self.inner.lock().await;
            (
                g.status,
                g.port,
                g.port_record.as_ref().map(|r| r.bind.clone()),
            )
        };
        // 探测要按 daemon 实际绑定的那个地址来：绑具体网卡地址（如 192.168.12.81）
        // 时回环不通，只探 127.0.0.1 会把活着的 daemon 误判成起不来。
        let healthy = match port {
            Some(p) => {
                let host = bind.as_deref().unwrap_or("127.0.0.1");
                port_registry::healthz(p, host).await
            }
            None => false,
        };
        start_is_noop(status, port, healthy)
    }
}
