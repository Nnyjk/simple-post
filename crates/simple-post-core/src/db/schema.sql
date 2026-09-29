-- Simple Post schema.
-- Executed once at boot from `db/mod.rs::migrate`. Idempotent:
-- every `CREATE` uses `IF NOT EXISTS` so a re-run on an already-migrated
-- database is a no-op.  Foreign keys are enforced; ON DELETE CASCADE
-- keeps project / module / collection deletion cheap (one DELETE on the
-- root removes the whole subtree).
--
-- Migrations create tables and insert **no rows**. A fresh install is
-- empty by design — test fixtures live in `fixtures.json` behind
-- `#[cfg(test)]`, not here.
--
-- Timestamp convention: every `*_at` column is a unix-ms integer
-- (what JS / TS `Date.now()` returns). Easier to compare, sort, and
-- ship through serde_json than RFC-3339 strings.

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  default_base_url_definition_id TEXT,
  color TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS base_url_definitions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_base_url_defs_project
  ON base_url_definitions(project_id);

CREATE TABLE IF NOT EXISTS modules (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  sort_order INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_modules_project ON modules(project_id);

CREATE TABLE IF NOT EXISTS collections (
  id TEXT PRIMARY KEY,
  module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
  parent_collection_id TEXT REFERENCES collections(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  sort_order INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_collections_module ON collections(module_id);

CREATE TABLE IF NOT EXISTS endpoints (
  id TEXT PRIMARY KEY,
  collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  method TEXT NOT NULL,
  url TEXT NOT NULL,
  environment_id TEXT,
  base_url_definition_id TEXT,
  base_url_override TEXT,
  params_json TEXT NOT NULL DEFAULT '[]',
  headers_json TEXT NOT NULL DEFAULT '[]',
  body_json    TEXT NOT NULL DEFAULT '{}',
  auth_json    TEXT NOT NULL DEFAULT '{}',
  docs TEXT,
  notes TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  sort_order INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_endpoints_collection ON endpoints(collection_id);

CREATE TABLE IF NOT EXISTS environments (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 0,
  base_urls_json TEXT NOT NULL DEFAULT '{}',
  variables_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_envs_project ON environments(project_id);

CREATE TABLE IF NOT EXISTS agent_permissions (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  permission TEXT NOT NULL CHECK(permission IN ('read','read_call','full'))
);

-- Singleton row (CHECK id=1). Holds the global HTTP client knobs
-- surfaced in the Settings UI.
CREATE TABLE IF NOT EXISTS request_settings (
  id INTEGER PRIMARY KEY CHECK(id=1),
  timeout_ms INTEGER NOT NULL DEFAULT 30000,
  follow_redirects INTEGER NOT NULL DEFAULT 1,
  max_response_size_kb INTEGER NOT NULL DEFAULT 5120
);

-- Free-form key/value bag used for the renderer's last-opened project,
-- UI prefs, recent envs, etc. Values are JSON-encoded so the schema
-- doesn't need to know about specific keys up front.
CREATE TABLE IF NOT EXISTS preferences (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);