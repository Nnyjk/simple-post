//! Handler modules, one per REST resource. Each file owns the handlers
//! for its family; the router registrations themselves stay in
//! `super::build_router` so the full route table remains readable in
//! one place.

pub(crate) mod base_url_defs;
pub(crate) mod environments;
pub(crate) mod mcp;
pub(crate) mod nodes;
pub(crate) mod permissions;
pub(crate) mod preferences;
pub(crate) mod projects;
pub(crate) mod request_settings;

pub(crate) use base_url_defs::{
    base_url_defs_create, base_url_defs_delete, base_url_defs_list, base_url_defs_set_default,
    base_url_defs_update,
};
pub(crate) use environments::{
    environments_create, environments_delete, environments_list, environments_rename_var,
    environments_set_active, environments_set_module_baseurl, environments_set_var,
    environments_update,
};
pub(crate) use mcp::{
    mcp_capability_get, mcp_capability_list_tools, mcp_capability_put, mcp_healthz,
    mcp_json_rpc_post, mcp_proxy_path, mcp_restart, mcp_start, mcp_status, mcp_stop, mcp_tools,
};
// `http_send` is re-exported from here rather than from a separate
// `endpoints` module: it is addressed under a node now, and the module
// it lives in follows the address.
pub(crate) use nodes::{
    endpoint_details_list, http_send, node_duplicate, node_endpoint_detail_get,
    node_endpoint_detail_patch, nodes_create, nodes_delete, nodes_get, nodes_list, nodes_move,
    nodes_update,
};
pub(crate) use permissions::{permissions_get, permissions_set};
pub(crate) use preferences::{preferences_get, preferences_put};
pub(crate) use projects::{
    projects_create, projects_delete, projects_get, projects_list, projects_update,
};
pub(crate) use request_settings::{request_settings_get, request_settings_put};
