//! `/api/dynamic-variables` 一组。
//!
//! 一条声明：变量名 → 数据源 → 抽取规则 → 缓存时长。消费端点的请求体里写
//! `{{sns}}`，发送时程序取数、抽取、缓存并替换。
//!
//! 定义挂在**项目**里：一条定义属于一个项目，变量名在项目内唯一。消费端点只引用
//! 同一个项目里的定义。
//!
//! 列表只读缓存行，**不取数**；`/resolve` 才会真的发出站请求。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "GET",
            path: "/api/dynamic-variables",
            summary: "List dynamic variables with their current value and cache state; \
                      `projectId` narrows the list to one project, and the read never fetches",
            stability: "public",
            request: None,
            response: Some("DynamicVariableView[]"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/dynamic-variables",
            summary: "Create a dynamic variable owned by the project named by `projectId`; \
                      the name is unique within that project",
            stability: "public",
            request: Some("DynamicVariableCreate"),
            response: Some("DynamicVariable"),
        },
        RouteSpec {
            method: "PATCH",
            path: "/api/dynamic-variables/{id}",
            summary: "Partially update a dynamic variable's definition",
            stability: "public",
            request: Some("DynamicVariablePatch"),
            response: Some("DynamicVariable"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/dynamic-variables/{id}",
            summary: "Delete a dynamic variable by id",
            stability: "public",
            request: None,
            response: None,
        },
        RouteSpec {
            method: "POST",
            path: "/api/dynamic-variables/resolve",
            summary: "Resolve dynamic variables by name; fetches from the data source when \
                      the cache is cold or expired",
            stability: "public",
            request: Some("DynamicVariableResolve"),
            response: Some("DynamicResolution"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/dynamic-variables/{id}/override",
            summary: "Set a dynamic variable's value by hand; a null value clears the \
                      override so the next resolution fetches again",
            stability: "public",
            request: Some("DynamicVariableOverride"),
            response: Some("DynamicVariableView"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/dynamic-variables/{id}/refresh",
            summary: "Force a dynamic variable to be fetched again",
            stability: "public",
            // 刷新不需要请求体：取数上下文由定义自己决定。
            request: None,
            response: Some("DynamicValueStatus"),
        },
    ]
}
