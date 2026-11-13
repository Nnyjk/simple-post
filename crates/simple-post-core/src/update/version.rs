//! 版本号解析与比较。
//!
//! 已发布 tag 的形状是 `v0.1.34`，预发布 tag 是 `0.1.34-rc1`。比较规则：
//!
//! * 去掉一个前导 `v`；
//! * 切掉第一个 `-` 及其后的后缀，故 `0.1.34-rc1` 与 `0.1.34` 视为同版。取舍：发布说明只写在
//!   正式 tag 上，而「-rc1 算不算新版」没有可靠答案；代价是 `0.2.0-rc1` 不会被判成比 `0.1.99`
//!   新，这一条由发布流程负责（预发布不挂 `releases/latest`）。
//! * 逐段按数字比，段数不同时缺段按 0：`1.0 == 1.0.0`。逐段比而不是比字符串，否则
//!   `0.1.9` 与 `0.1.10` 会按字典序判反；
//! * 任一段不是纯数字 → [`BadVersion`]。

use std::cmp::Ordering;
use std::fmt;

/// 拆好段的版本号。段数保留原样（不补零），比较时缺段按 0。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version(Vec<u64>);

/// 版本号里出现了非数字段，或整串为空。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadVersion;

impl fmt::Display for BadVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("version is not a dotted numeric string")
    }
}

impl std::error::Error for BadVersion {}

/// 解析 `v0.1.34` / `0.1.34-rc1` / `1.0` 这几种形状。
pub fn parse(raw: &str) -> Result<Version, BadVersion> {
    let trimmed = raw.trim();
    let without_v = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let numeric = without_v.split('-').next().unwrap_or("");
    let mut parts = Vec::new();
    for part in numeric.split('.') {
        // 空段（`1..0`、尾随点、整串为空）不是数字，`u64::from_str` 会拒，但显式写出来
        // 才好读：`""` 单独解析会得到 `ParseIntError`，两者都归 `BadVersion`。
        if part.is_empty() {
            return Err(BadVersion);
        }
        parts.push(part.parse::<u64>().map_err(|_| BadVersion)?);
    }
    if parts.is_empty() {
        return Err(BadVersion);
    }
    Ok(Version(parts))
}

/// 两个版本串的偏序。任一侧解析失败即 `Err`。
pub fn compare(a: &str, b: &str) -> Result<Ordering, BadVersion> {
    let left = parse(a)?;
    let right = parse(b)?;
    let len = left.0.len().max(right.0.len());
    for index in 0..len {
        let l = left.0.get(index).copied().unwrap_or(0);
        let r = right.0.get(index).copied().unwrap_or(0);
        match l.cmp(&r) {
            Ordering::Equal => continue,
            other => return Ok(other),
        }
    }
    Ok(Ordering::Equal)
}

/// `candidate` 是否比 `current` 新。
pub fn is_newer(candidate: &str, current: &str) -> Result<bool, BadVersion> {
    Ok(compare(candidate, current)? == Ordering::Greater)
}

#[cfg(test)]
#[path = "version_test.rs"]
mod tests;
