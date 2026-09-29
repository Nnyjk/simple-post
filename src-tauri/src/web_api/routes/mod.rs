//! Handler modules, one per REST resource. Each file owns the handlers
//! for its family; the router registrations themselves stay in
//! `super::build_router` so the full route table remains readable in
//! one place.

pub(crate) mod base_url_defs;
pub(crate) mod collections;
pub(crate) mod endpoints;
pub(crate) mod environments;
pub(crate) mod mcp;
pub(crate) mod modules;
pub(crate) mod permissions;
pub(crate) mod preferences;
pub(crate) mod projects;
pub(crate) mod request_settings;

pub(crate) use base_url_defs::{
    base_url_defs_create, base_url_defs_delete, base_url_defs_list, base_url_defs_set_default,
    base_url_defs_update,
};
pub(crate) use collections::{
    collections_create, collections_delete, collections_list, collections_move, collections_update,
};
pub(crate) use endpoints::{
    endpoint_send, endpoints_create, endpoints_delete, endpoints_duplicate, endpoints_get,
    endpoints_list, endpoints_move, endpoints_search, endpoints_update, http_send,
};
pub(crate) use environments::{
    environments_create, environments_delete, environments_list, environments_rename_var,
    environments_set_active, environments_set_baseurl, environments_set_var, environments_update,
};
pub(crate) use mcp::{
    mcp_capability_get, mcp_capability_list_tools, mcp_capability_put, mcp_healthz,
    mcp_json_rpc_post, mcp_proxy_path, mcp_restart, mcp_start, mcp_status, mcp_stop, mcp_tools,
};
pub(crate) use modules::{
    modules_create, modules_delete, modules_list, modules_move, modules_update,
};
pub(crate) use permissions::{permissions_get, permissions_set};
pub(crate) use preferences::{db_recreated_at, preferences_get, preferences_put};
pub(crate) use projects::{
    projects_create, projects_delete, projects_get, projects_list, projects_update,
};
pub(crate) use request_settings::{request_settings_get, request_settings_put};
