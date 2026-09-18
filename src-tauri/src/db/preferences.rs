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
pub fn get<T: DeserializeOwned>(
    conn: &mut Connection,
    key: &str,
) -> Result<Option<T>, AppError> {
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
pub fn set<T: Serialize>(
    conn: &mut Connection,
    key: &str,
    value: &T,
) -> Result<(), AppError> {
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

    /// `"mcp.auto_start"` → bool. **Default: `false`.** When `false`
    /// the Tauri / Web shapes both leave the MCP daemon stopped at
    /// launch; the user opts in from the settings panel or via the
    /// `--mcp-auto-start` CLI flag. See
    /// `docs/distribution-modes-2026-09-18.md` §5 for the rationale
    /// and the three-shape matrix.
    pub const MCP_AUTO_START: &str = "mcp.auto_start";
}

/// Read the `mcp.auto_start` preference with the documented default
/// (`false`). Used by both the Tauri `.setup()` block and the web
/// server's startup path so the two shapes agree on the same row.
pub fn mcp_auto_start(conn: &mut Connection) -> Result<bool, AppError> {
    let raw: Option<bool> = get(conn, keys::MCP_AUTO_START)?;
    Ok(raw.unwrap_or(false))
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Theme {
        mode: String,
        accent: String,
    }

    fn fresh_pool() -> db::DbPool {
        let dir = std::env::temp_dir().join(format!(
            "simple-post-prefs-test-{}-{}",
            std::process::id(),
            crate::db::test_counter::next()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        db::init(&dir).expect("init")
    }

    #[test]
    fn missing_key_returns_none() {
        let pool = fresh_pool();
        let got: Option<Theme> = db::with_conn(&pool, |c| get(c, "theme")).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn set_then_get_round_trips_struct() {
        let pool = fresh_pool();
        let theme = Theme { mode: "dark".into(), accent: "#fff".into() };
        db::with_tx(&pool, |c| set(c, "theme", &theme)).unwrap();
        let back: Theme = db::with_conn(&pool, |c| get(c, "theme"))
            .unwrap()
            .expect("must be present");
        assert_eq!(back, theme);
    }

    #[test]
    fn set_overwrites_existing_value() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            set(c, "theme", &Theme { mode: "dark".into(), accent: "#000".into() })?;
            set(c, "theme", &Theme { mode: "light".into(), accent: "#fff".into() })?;
            Ok(())
        })
        .unwrap();
        let back: Theme = db::with_conn(&pool, |c| get(c, "theme"))
            .unwrap()
            .unwrap();
        assert_eq!(back.mode, "light");
    }

    #[test]
    fn set_with_primitive_types() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| set(c, "last_project", &"p_demo".to_string())).unwrap();
        let back: String = db::with_conn(&pool, |c| get(c, "last_project"))
            .unwrap()
            .unwrap();
        assert_eq!(back, "p_demo");
    }

    #[test]
    fn list_all_returns_every_pair() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| {
            set(c, "a", &"1".to_string())?;
            set(c, "b", &"2".to_string())?;
            Ok(())
        })
        .unwrap();
        let pairs = db::with_conn(&pool, |c| list_all(c)).unwrap();
        let keys: Vec<_> = pairs.iter().map(|(k, _)| k.clone()).collect();
        assert!(keys.contains(&"a".to_string()));
        assert!(keys.contains(&"b".to_string()));
    }

    #[test]
    fn delete_removes_a_key() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| set(c, "tmp", &"v".to_string())).unwrap();
        db::with_tx(&pool, |c| delete(c, "tmp")).unwrap();
        let got: Option<String> = db::with_conn(&pool, |c| get(c, "tmp")).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn get_with_wrong_type_errors() {
        let pool = fresh_pool();
        // Store a number, try to read as a struct.
        db::with_tx(&pool, |c| set(c, "x", &42_i64)).unwrap();
        let err: Result<Option<Theme>, _> = db::with_conn(&pool, |c| get(c, "x"));
        assert!(err.is_err());
    }

    #[test]
    fn mcp_auto_start_defaults_to_false_when_missing() {
        let pool = fresh_pool();
        let v = db::with_conn(&pool, |c| mcp_auto_start(c)).unwrap();
        assert!(!v, "default must be false — see docs/distribution-modes §5");
    }

    #[test]
    fn mcp_auto_start_round_trips() {
        let pool = fresh_pool();
        db::with_tx(&pool, |c| set(c, keys::MCP_AUTO_START, &true)).unwrap();
        assert!(db::with_conn(&pool, |c| mcp_auto_start(c)).unwrap());
        db::with_tx(&pool, |c| set(c, keys::MCP_AUTO_START, &false)).unwrap();
        assert!(!db::with_conn(&pool, |c| mcp_auto_start(c)).unwrap());
    }
}