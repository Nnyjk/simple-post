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
  -- The project's ★ service: what an endpoint naming no service of its
  -- own resolves against. One per project, not per module — the service
  -- list is project-wide, so a ★ that scoped to a module would name a
  -- service other modules cannot see. NULL is a normal state (a project
  -- nobody has given a default service yet), not an absent value.
  default_base_url_definition_id TEXT,
  color TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- A named service, owned by the PROJECT. Every module sees every service
-- in this list — there is no per-module service list, so two modules can
-- never hold services that share a `name`. That is the whole point of the
-- model: 「服务应该绑定到项目，这样每一个模块都会全部包含这些服务」.
--
-- A service row carries no URL. The value lives in
-- `environments.module_base_urls_json`, keyed by (service × module), so a
-- row here answers "what services exist" and nothing else.
--
-- `module_id` and `is_default` below are **dead columns kept on purpose**.
-- They held the module-owned service model (per-module service lists and
-- per-module ★) that this build reversed; `db::migrate` nulls / zeroes
-- them and nothing reads or writes them after that. They stay because
-- dropping a column means rebuilding the table, which means a
-- `SCHEMA_VERSION` bump, and a bump makes `ensure_current_schema` delete
-- the user's store. `db::ADDED_COLUMNS` re-adds both to stores that
-- predate them, for the same reason: the migration has to be able to read
-- the old shape in order to neutralise it.
--
-- Added after this table first shipped, so the `CREATE TABLE IF NOT
-- EXISTS` above is a no-op for a store that already has the table.
CREATE TABLE IF NOT EXISTS base_url_definitions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  -- NULL always. Nullable rather than NOT NULL because `ADDED_COLUMNS`
  -- can only append a nullable column to a live table.
  module_id TEXT REFERENCES nodes(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  -- 0 always. See above.
  is_default INTEGER NOT NULL DEFAULT 0
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
  -- DEPRECATED, kept only so the column survives. It was `def_id -> url`,
  -- the single value layer before services carried a module. Nothing
  -- reads or writes it now; `db::migrate` copied every live value into
  -- `module_base_urls_json` and is the only reader left in the crate.
  --
  -- Not cleared by the migration for the shape it can leave behind (see
  -- `migrate_base_urls_into_value_layer`): those stores re-derive the
  -- identical values on every later run, so the stale blob is inert.
  base_urls_json TEXT NOT NULL DEFAULT '{}',
  -- def_id -> module_id -> url. **The only layer a base URL lives in.**
  -- Each cell is independent: a module with no entry for a service has no
  -- URL for it, and does not inherit another module's or the project's.
  module_base_urls_json TEXT NOT NULL DEFAULT '{}',
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
  -- Path only. The host comes from the environment's moduleBaseUrls at
  -- send time, so one endpoint can be retargeted without a rewrite.
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

-- One persisted HTTP response, kept per endpoint so the response
-- history strip survives a restart. Before this table the history was
-- a renderer-only `Record<endpointId, ApiResponse[]>` that a relaunch,
-- a project switch or a closed tab erased.
--
-- `endpoint_id` cascades, and `nodes.parent_id` / `nodes.root_id`
-- cascade in turn, so deleting an endpoint — or any ancestor, up to the
-- project — takes its history with it in the same DELETE. There is
-- deliberately no application-level prune to forget; the FK is the
-- contract. That does mean a `draft:` endpoint (a renderer-only endpoint
-- that has never been saved, see `DRAFT_ID_PREFIX` in
-- `src/stores/store-types.ts`) cannot be stored here at all: it has no
-- `nodes` row to point at, and the renderer skips persisting those.
--
-- Everything the list view renders (`status`, `created_at`, `method`,
-- `url`, `duration_ms`) is a real column rather than a field inside a
-- JSON blob, because those are what the strip sorts and filters on.
-- `body` is capped on write at
-- `domain::RESPONSE_HISTORY_MAX_BODY_BYTES` — `body_truncated` records
-- that it happened, so the viewer can say so instead of silently
-- showing half a document as if it were the whole one.
CREATE TABLE IF NOT EXISTS response_history (
  id              TEXT PRIMARY KEY,
  endpoint_id     TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  status          INTEGER NOT NULL,
  status_text     TEXT NOT NULL DEFAULT '',
  duration_ms     INTEGER NOT NULL DEFAULT 0,
  size_bytes      INTEGER NOT NULL DEFAULT 0,
  content_type    TEXT NOT NULL DEFAULT '',
  -- Key → value, not ordered pairs: by the time a response reaches
  -- here it has already been through `tuplesToRecord` in
  -- `src/lib/http.ts`, so the renderer holds a map and a map is what
  -- round-trips without inventing an order SQLite cannot restore.
  headers_json    TEXT NOT NULL DEFAULT '{}',
  body            TEXT NOT NULL DEFAULT '',
  body_json       TEXT,
  error           TEXT,
  method          TEXT,
  url             TEXT,
  body_truncated  INTEGER NOT NULL DEFAULT 0,
  created_at      INTEGER NOT NULL
);
-- The one read this table has: the history strip asks for **one
-- endpoint's** rows, and that is the only query the renderer issues.
--
-- It was once "every endpoint in a project", fetched in bulk when a
-- project opened. That was the wrong shape — a panel showing ten rows
-- for the endpoint in front of the user should not cost a read whose
-- size grows with the project — so the bulk read and its index are gone.
--
-- `rowid` is deliberately NOT in the index even though the ORDER BY
-- uses it as the tie-break — SQLite rejects `rowid` in an index
-- definition ("no such column: rowid"), and there is nothing to gain by
-- working around that. The sort it leaves behind runs over at most ten
-- rows per endpoint, because `db::response_history::append` is what
-- keeps the table that small.
CREATE INDEX IF NOT EXISTS idx_response_history_endpoint
  ON response_history(endpoint_id, created_at DESC);

-- Dropped rather than left behind. `CREATE INDEX IF NOT EXISTS` cannot
-- remove anything, so a store written by a build that had the
-- project-scoped index would keep it forever otherwise. Dropping an
-- index is not a schema version event: no row is touched, `user_version`
-- stays put, and `ensure_current_schema` is not involved.
DROP INDEX IF EXISTS idx_response_history_root;

-- Second line of defence for the parent/child matrix.
--
-- `db::nodes` checks every write against
-- `ResourceNodeType::may_under`, but the MCP tools are this project's
-- primary external write path, and a future caller that reaches past
-- `db::nodes` would otherwise be free to build a tree no reader can
-- walk. The old layout got the whole matrix from foreign keys —
-- `endpoints.collection_id REFERENCES collections(id)` among them —
-- which one polymorphic `parent_id` cannot express: SQLite's CHECK
-- constraints cannot read another row, and a self-referencing foreign
-- key says nothing about which types may sit on which.
--
-- These two triggers are dropped and rebuilt on every `migrate`
-- instead of being `IF NOT EXISTS`. A trigger body is part of the
-- contract, and `IF NOT EXISTS` would freeze whatever the first build
-- to touch a given `store.db` wrote: relaxing the matrix to let an
-- endpoint sit under a module would leave every existing database
-- enforcing the old rule, which the application layer no longer
-- agrees with. Bumping `SCHEMA_VERSION` instead would delete those
-- databases outright, and losing a user's tree to widen a predicate
-- is not a trade worth making.
--
-- Deliberately NOT covered here: `root_id` agreeing with the parent's.
-- `move_many` re-derives it across a subtree, and a subtree is
-- rewritten one row at a time — so parent and child legitimately
-- disagree mid-transaction, and a constraint would have to be dropped
-- exactly when it is needed. `db::nodes::validate_placement` checks it
-- on create, and `move_many` owns it after that.
DROP TRIGGER IF EXISTS trg_nodes_parent_matrix_insert;
DROP TRIGGER IF EXISTS trg_nodes_parent_matrix_update;

CREATE TRIGGER trg_nodes_parent_matrix_insert
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
        OR (NEW.type = 'endpoint'   AND p.type IN ('module', 'collection'))
      )
  );
END;

CREATE TRIGGER trg_nodes_parent_matrix_update
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
        OR (NEW.type = 'endpoint'   AND p.type IN ('module', 'collection'))
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
--   2  `nodes` / `endpoint_details` landed additively, so an older v2
--      file gains them on the next `CREATE TABLE IF NOT EXISTS`
--      without needing its own version.
--   3  current. No column changed — what changed is what
--      `endpoint_details.auth_json` holds. It stores the serialised
--      `domain::AuthConfig`, and dropping the `type` → `kind` serde
--      rename (and `in` → `inLocation`) means a v2 row no longer
--      deserialises: `kind` is a required field, so even the `'{}'`
--      default fails. Unreadable rather than stale, hence a bump
--      instead of a reader-side shim.
--
-- After 3: the service list moved to the project and the value layer
-- became (environment × module × service) in
-- `environments.module_base_urls_json`. STILL version 3 — `module_base_urls_json`
-- is additive and arrives via `db::ADDED_COLUMNS`, and the two columns
-- this model retired (`base_url_definitions.module_id` / `.is_default`)
-- are kept and neutralised rather than dropped, because a DROP means a
-- rebuild means a version bump means `ensure_current_schema` deletes every
-- user's store. Rows written by either predecessor layout are repaired by
-- `migrate_base_urls_into_value_layer` in `db::mod`.
--
-- 1 was never shipped: the nodes merge landed before any build could
--      stamp a version of its own, so it has no database anywhere.
--
-- STILL version 3 after `response_history` landed: a new table is the
-- purest additive change there is — every `CREATE` above is
-- `IF NOT EXISTS`, so an existing store gains it on the next
-- `db::migrate` and needs no `ADDED_COLUMNS` entry (that list is for
-- columns added to a table that already exists) and no version bump.
PRAGMA user_version = 3;