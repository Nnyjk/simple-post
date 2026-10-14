// =========================================================================
// MCP HTTP listener configuration
// =========================================================================
//
// The daemon's bind address, port, mount path and Host/Origin allowlists
// are persisted as a JSON blob in `preferences` under `mcp.http_config`.
//
// **Why a helper pair and not a plain preferences write.** The daemon
// is a *child process*: whatever is in this row is what
// `daemon_supervisor::daemon_cli_args` puts on its command line at the
// next spawn. Saving therefore does not reconfigure a running daemon,
// so the response carries `requires_restart` and the UI can say so
// rather than silently showing settings that are not in effect.
//
// Both transports go through the two plain functions below — the Tauri
// commands in `src-tauri/src/commands/mcp_http_config.rs` and the REST
// routes in `web_api/routes/mcp.rs` are both thin wrappers, so the
// desktop and web shapes cannot drift on validation or on the restart
// verdict.
//
// This file is host-agnostic and moved into the server crate with the
// rest of the `serve-web` surface; it needs `crate::port_registry`,
// which lives here too. The two `#[tauri::command]` wrappers that used
// to sit at the bottom of this file stayed in `src-tauri` and call the
// functions below.

use serde::Serialize;

use crate::db::DbPool;
use crate::domain::McpHttpConfig;
use crate::error::AppError;
use crate::port_registry::{self, PortRecord};

/// Response envelope for a write: the stored config plus whether the
/// running daemon is already serving something else.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpHttpConfigDto {
    pub config: McpHttpConfig,
    /// The new config differs from what the running daemon is actually
    /// serving, so it only takes effect after a restart.
    pub requires_restart: bool,
}

/// Read the persisted config. An absent row yields
/// [`McpHttpConfig::default`]; an unparseable one is an error (see
/// `db::preferences::mcp_http_config`).
pub fn read_http_config(pool: &DbPool) -> Result<McpHttpConfig, AppError> {
    crate::db::with_conn(pool, crate::db::preferences::mcp_http_config)
}

/// Validate, persist, and report whether a restart is needed.
pub fn write_http_config(
    pool: &DbPool,
    config: McpHttpConfig,
) -> Result<McpHttpConfigDto, AppError> {
    config.validate()?;
    let previous = read_http_config(pool)?;
    let requires_restart = requires_restart(&config, &previous, port_registry::read());
    crate::db::with_tx(pool, |c| {
        crate::db::preferences::set_mcp_http_config(c, &config)
    })?;
    Ok(McpHttpConfigDto {
        config,
        requires_restart,
    })
}

/// Does the new config differ from the one the daemon is serving now?
///
/// Three sources of truth, and only two of them are knowable at save
/// time:
///
///   * **host / port** — the running daemon's own `--bind` and the port
///     it actually bound are published in the `mcp.port` record. A
///     configured `port: 0` means "OS-assigned", which the running
///     daemon has already resolved, so it is never a difference.
///   * **path / allow_remote / allow lists** — the record does not carry
///     these, so the previously *persisted* row is the only available
///     reference. That is accurate for every daemon this app spawned
///     (the supervisor builds its CLI from the same row), and errs
///     toward "restart" for a hand-launched one, which is the safe
///     direction for a flag the user cannot otherwise verify.
///
/// `record: None` is the "cannot tell" case and answers `true`. A record
/// whose PID is gone answers `false`: nothing is serving, so the next
/// start picks the new row up with no restart step in between.
fn requires_restart(
    new: &McpHttpConfig,
    previous: &McpHttpConfig,
    record: Option<PortRecord>,
) -> bool {
    let Some(record) = record else {
        return true;
    };
    if !port_registry::is_pid_alive(record.pid) {
        return false;
    }
    new.host != record.bind
        || (new.port != 0 && new.port != record.port)
        || new.path != previous.path
        || new.allow_remote != previous.allow_remote
        || new.allowed_hosts != previous.allowed_hosts
        || new.allowed_origins != previous.allowed_origins
}
