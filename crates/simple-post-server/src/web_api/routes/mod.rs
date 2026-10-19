//! 每类 REST 资源一个 handler 模块。每个文件拥有自己那族 handler；路由注册本身留在
//! `super::build_router`，好让整张路由表在一处读完。

pub(crate) mod base_url_defs;
pub(crate) mod environments;
pub(crate) mod interchange;
pub(crate) mod mcp;
pub(crate) mod nodes;
pub(crate) mod oauth2;
pub(crate) mod permissions;
pub(crate) mod preferences;
pub(crate) mod projects;
pub(crate) mod request_settings;
pub(crate) mod response_history;

pub(crate) use base_url_defs::{
    base_url_defs_create, base_url_defs_delete, base_url_defs_list, base_url_defs_set_default,
    base_url_defs_update,
};
pub(crate) use environments::{
    environments_create, environments_delete, environments_list, environments_rename_var,
    environments_set_active, environments_set_module_baseurl, environments_set_var,
    environments_update,
};
pub(crate) use interchange::{
    interchange_export_document, interchange_export_preview, interchange_import_commit,
    interchange_import_preview,
};
pub(crate) use mcp::{
    mcp_capability_get, mcp_capability_list_tools, mcp_capability_put, mcp_healthz,
    mcp_http_config_get, mcp_http_config_put, mcp_json_rpc_post, mcp_proxy_path, mcp_restart,
    mcp_start, mcp_status, mcp_stop, mcp_tools,
};
// `http_send` 从这里而非某个独立的 `endpoints` 模块再导出：它现在挂在节点之下，
// 模块归属随地址走。
pub(crate) use nodes::{
    endpoint_details_list, http_send, node_duplicate, node_endpoint_detail_get,
    node_endpoint_detail_patch, nodes_create, nodes_delete, nodes_get, nodes_list, nodes_move,
    nodes_update,
};
pub(crate) use oauth2::oauth2_token;
pub(crate) use permissions::{permissions_get, permissions_set};
pub(crate) use preferences::{preferences_get, preferences_put};
pub(crate) use projects::{
    projects_create, projects_delete, projects_get, projects_list, projects_update,
};
pub(crate) use request_settings::{request_settings_get, request_settings_put};
pub(crate) use response_history::{
    response_history_append, response_history_clear, response_history_list,
};
