//! 变量池的合并规则。
//!
//! 动态变量的值**进变量池**，不去改已经装配好的请求：替换那一步只认池子里的名字。

use std::collections::HashMap;

/// 合成变量池：环境变量（去掉停用的那些）+ 动态变量的值。
///
/// 两条子规则都由「动态变量放在停用过滤之后并入」这一条实现方式成立：
///
/// 1. 动态变量的值覆盖同名环境变量——它是后面并进去的。
/// 2. 被停用的环境变量不影响动态变量——停用过滤只作用于环境变量，停用名单里的同名动态
///    变量照常生效。
pub fn merge_pool(
    env_vars: &HashMap<String, String>,
    disabled: &[String],
    dynamic: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut pool: HashMap<String, String> = env_vars
        .iter()
        .filter(|(k, _)| !disabled.iter().any(|d| d == *k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in dynamic {
        pool.insert(k.clone(), v.clone());
    }
    pool
}
