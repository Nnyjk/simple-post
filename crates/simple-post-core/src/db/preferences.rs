//! Free-form key/value preference bag.
//!
//! Used for renderer prefs that don't fit a structured column
//! (last-opened project id, theme mode, recent env ids, etc.).
//! Values are JSON-encoded so the schema stays flat — the caller
//! picks the JSON shape via the type parameter on [`get`] / [`set`].
//!
//! No DB / no domain types — the renderer is the schema.

use rusqlite::{params, Connection};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::AppError;

// -------------------------------------------------------------------------
// Reads.
// -------------------------------------------------------------------------

/// Read a preference by key. Returns `Ok(None)` if the key is absent,
/// so callers can fall back to a default without a separate "exists"
/// query.
///
/// Returns `AppError::Db` if the stored value isn't valid JSON for
/// `T` — the renderer is expected to bump a schema version if it
/// reshapes a preference key.
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

// -------------------------------------------------------------------------
// Writes.
// -------------------------------------------------------------------------

/// Upsert a preference. The value is JSON-encoded via serde so any
/// `Serialize` type can be stored.
pub fn set<T: Serialize>(conn: &mut Connection, key: &str, value: &T) -> Result<(), AppError> {
    let raw = serde_json::to_string(value)?;
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

/// Return every (key, value) pair. Used by the renderer on boot to
/// hydrate its in-memory store in one round-trip.
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

// -------------------------------------------------------------------------
// Well-known keys.
// -------------------------------------------------------------------------

/// Preference keys shared across the app. Stored as JSON values to
/// match the rest of `preferences` (see `set` for the encoding). Add
/// new entries here when a new persisted setting is needed so callers
/// don't string-literal the key.
pub mod keys {
    /// `"ui.theme"` → string. Allowed: "light" | "dark" | "system".
    pub const THEME: &str = "ui.theme";

    /// `"mcp.auto_start"` → bool. **Default: `false`.**
    ///
    /// Why `false`: the daemon is a long-lived background process, and
    /// the desktop app's Tauri `.setup()` would otherwise spawn one on
    /// every launch with no user intent behind it. Defaulting to stopped
    /// keeps "an MCP server is running" an explicit opt-in — the settings
    /// toggle or `--mcp-auto-start` both flip it, and the key is shared
    /// by the desktop and `serve-web` shapes so one switch covers both.
    /// When `false` both shapes report `stopped` until something starts it.
    pub const MCP_AUTO_START: &str = "mcp.auto_start";

    /// `"mcp.policy"` → string. **Default: `read_only`.** The global
    /// MCP capability tier (`"read_only"` / `"safe_write"` /
    /// `"full_access"`) the user has selected from the settings panel.
    /// Read by `permissions::resolve_capability` and the Tauri command
    /// `mcp_capability_get`; written by `mcp_capability_set`. A missing
    /// row resolves to `read_only`, so an AI client can list and get but
    /// cannot change anything until the user raises the tier.
    pub const MCP_POLICY: &str = "mcp.policy";

    /// `"db.recreated_at"` → integer (unix ms). **Absent on a normal
    /// boot.**
    ///
    /// Written by [`crate::db::init`] only when it had to replace a
    /// `store.db` written by an older schema version (see
    /// [`crate::db::SCHEMA_VERSION`]). The renderer reads it once at
    /// bootstrap to show "your previous workspace was reset", because
    /// the alternative — a silently empty tree — reads as "you never
    /// had any projects" rather than "we deleted them".
    pub const DB_RECREATED_AT: &str = "db.recreated_at";
}

/// Read the `mcp.auto_start` preference with the documented default
/// (`false`). Used by both the Tauri `.setup()` block and the web
/// server's startup path so the two shapes agree on the same row.
pub fn mcp_auto_start(conn: &mut Connection) -> Result<bool, AppError> {
    let raw: Option<bool> = get(conn, keys::MCP_AUTO_START)?;
    Ok(raw.unwrap_or(false))
}

/// Read the `mcp.policy` preference as a [`CapabilityPolicy`]. Falls
/// back to [`CapabilityPolicy::ReadOnly`] when the row is missing or
/// carries an unrecognised string (forward-compat — a future release
/// can ship a new tier without breaking older clients).
///
/// `pick` is the appropriate verb here — we are decoding a wire form
/// that the user can tamper with via the preferences table directly.
pub fn mcp_policy(conn: &mut Connection) -> Result<crate::permissions::CapabilityPolicy, AppError> {
    let raw: Option<String> = get(conn, keys::MCP_POLICY)?;
    Ok(match raw.as_deref() {
        Some("safe_write") => crate::permissions::CapabilityPolicy::SafeWrite,
        Some("full_access") => crate::permissions::CapabilityPolicy::FullAccess,
        // Default: ReadOnly. Includes the missing-row case AND the
        // forward-compat unknown-value case — see doc-comment.
        _ => crate::permissions::CapabilityPolicy::default(),
    })
}

/// Persist the global MCP capability tier. Encoded as the wire form
/// (`"read_only"` / `"safe_write"` / `"full_access"`) — see
/// [`crate::permissions::CapabilityPolicy::as_str`].
pub fn set_mcp_policy(
    conn: &mut Connection,
    p: crate::permissions::CapabilityPolicy,
) -> Result<(), AppError> {
    set(conn, keys::MCP_POLICY, &p.as_str())
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "preferences_test.rs"]
mod tests;
