-- Simple Post schema.
-- Executed once at boot from `db/mod.rs::migrate_and_seed`. Idempotent:
-- every `CREATE` uses `IF NOT EXISTS` so a re-run on an already-migrated
-- database is a no-op.  Foreign keys are enforced; ON DELETE CASCADE
-- keeps project / module / collection deletion cheap (one DELETE on the
-- root removes the whole subtree).
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

-- The `modules` / `collections` / `endpoints` tables that used to live
-- here are gone; `nodes` + `endpoint_details` below replaced them. See
-- the section header there for why the split is two tables and not one.

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

-- ===========================================================================
-- Resource tree — the merge of modules / collections / endpoints.
--
-- One row per tree node, discriminated by `type`. `parent_id` is the
-- only hierarchy link; `root_id` denormalises the owning project onto
-- every node so a whole tree is one indexed read instead of a walk up
-- the parent chain per node.
--
-- Projects are deliberately NOT nodes. A project is the implicit root
-- context — the UI binds exactly one active project — so `root_id`
-- stays single-valued and a module's parent is expressed as NULL.
--
-- Nothing reads these tables yet: modules / collections / endpoints are
-- still the live layout, and get dropped in a later pass. They are
-- additive, so SCHEMA_VERSION does not need a bump — see its doc
-- comment in db/mod.rs.
-- ===========================================================================

CREATE TABLE IF NOT EXISTS nodes (
  id          TEXT PRIMARY KEY,
  -- Owning project. Cascades, so deleting a project still takes the
  -- whole tree with it in a single DELETE.
  root_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  -- NULL for a module: its parent is the project, which is not a node.
  -- Self-referencing CASCADE gives the subtree teardown the old
  -- three-level foreign key chain provided.
  parent_id   TEXT REFERENCES nodes(id) ON DELETE CASCADE,
  type        TEXT NOT NULL CHECK(type IN ('module','collection','endpoint')),
  name        TEXT NOT NULL,
  description TEXT,
  -- Scoped to the parent, not to (parent, type). Under one parent the
  -- children are all the same type anyway, so nothing that reads in
  -- display order changes behaviour.
  sort_order  INTEGER NOT NULL,
  -- Modules and collections had no timestamps before; carrying them
  -- here costs nothing and spares every CRUD path a per-type branch.
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
-- The two tree reads: "this project's whole tree" and "this node's
-- children, in order".
CREATE INDEX IF NOT EXISTS idx_nodes_root   ON nodes(root_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_nodes_parent ON nodes(parent_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_nodes_type   ON nodes(type, name);

-- Endpoint configuration, split out because none of these columns mean
-- anything for a module or a collection. Folding them into `nodes`
-- would give every module and collection row a set of permanently
-- empty columns, and leave `method = ?` filters in a search with
-- nothing to discriminate them.
--
-- `node_id` is both the primary key and the join. No name, sort_order
-- or timestamps here: those live once, on `nodes`.
CREATE TABLE IF NOT EXISTS endpoint_details (
  node_id                TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
  method                 TEXT NOT NULL,
  -- Path only. The host comes from the environment's baseUrls at send
  -- time, so one endpoint can be retargeted without a rewrite.
  url                    TEXT NOT NULL,
  environment_id         TEXT,
  base_url_definition_id TEXT,
  base_url_override      TEXT,
  params_json            TEXT NOT NULL DEFAULT '[]',
  headers_json           TEXT NOT NULL DEFAULT '[]',
  body_json              TEXT NOT NULL DEFAULT '{}',
  auth_json              TEXT NOT NULL DEFAULT '{}',
  docs                   TEXT,
  notes                  TEXT,
  tags_json              TEXT NOT NULL DEFAULT '[]'
);

-- Second line of defence for the parent/child matrix.
--
-- `db::nodes` checks every write against
-- `ResourceNodeType::may_under`, but the MCP tools are this project's
-- primary external write path, and a future caller that reaches past
-- `db::nodes` would otherwise be free to build a tree no reader can
-- walk. The old layout got this from
-- `endpoints.collection_id REFERENCES collections(id)`, which one
-- polymorphic `parent_id` cannot express: SQLite's CHECK constraints
-- cannot read another row, and a self-referencing foreign key says
-- nothing about which types may sit on which.
--
-- Deliberately NOT covered here: `root_id` agreeing with the parent's.
-- `move_many` re-derives it across a subtree, and a subtree is
-- rewritten one row at a time — so parent and child legitimately
-- disagree mid-transaction, and a constraint would have to be dropped
-- exactly when it is needed. `db::nodes::validate_placement` checks it
-- on create, and `move_many` owns it after that.
CREATE TRIGGER IF NOT EXISTS trg_nodes_parent_matrix_insert
BEFORE INSERT ON nodes
BEGIN
  -- A module's parent is the project, which is not a node.
  SELECT RAISE(ABORT, 'nodes: a module has no node parent')
  WHERE NEW.type = 'module' AND NEW.parent_id IS NOT NULL;

  SELECT RAISE(ABORT, 'nodes: parent type does not accept this child type')
  WHERE NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM nodes p
    WHERE p.id = NEW.parent_id
      AND (
        (NEW.type = 'collection' AND p.type IN ('module', 'collection'))
        OR (NEW.type = 'endpoint'   AND p.type = 'collection')
      )
  );
END;

CREATE TRIGGER IF NOT EXISTS trg_nodes_parent_matrix_update
BEFORE UPDATE OF parent_id ON nodes
-- NULL means "promote to the top of the project", which is only legal
-- for a module — handled by the first clause. Every other type would
-- need a parent, and the NOT EXISTS below turns the implicit FK's
-- missing-parent case into an abort too.
WHEN NEW.parent_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'nodes: parent type does not accept this child type')
  WHERE NOT EXISTS (
    SELECT 1 FROM nodes p
    WHERE p.id = NEW.parent_id
      AND (
        (NEW.type = 'collection' AND p.type IN ('module', 'collection'))
        OR (NEW.type = 'endpoint'   AND p.type = 'collection')
      )
  );
END;

-- Schema version marker, read back by `db::ensure_current_schema` before
-- any connection is handed out. Keep in sync with
-- `db::SCHEMA_VERSION` — a mismatch there is caught by
-- `schema_version_matches_the_constant`.
--
-- Version history:
--   0  pre-versioning (the original modules/collections/endpoints
--      three-table layout). Any file stamped 0 is not readable by a
--      current build and is rebuilt from scratch.
--   2  current. `nodes` / `endpoint_details` landed additively, so an
--      older v2 file gains them on the next `CREATE TABLE IF NOT
--      EXISTS` without needing its own version.
--
-- 1 was never shipped: the nodes merge landed before any build could
--      stamp a version of its own, so it has no database anywhere.
PRAGMA user_version = 2;