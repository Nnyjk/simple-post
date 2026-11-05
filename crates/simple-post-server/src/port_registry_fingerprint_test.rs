//! 端口文件里那条配置指纹的回归网。
//!
//! `port_registry.rs` 的同级 `#[path]` 模块（以 `mod fingerprint_tests` 挂载）。
//! 全部是对纯函数的断言：不设环境变量、不写文件，故不会与别的测试抢
//! `SIMPLE_POST_DATA_DIR`。

use super::{parse_entry, serialize_entry, PortRecord, CONFIG_FINGERPRINT_KEY};

fn sample() -> PortRecord {
    PortRecord::new(0, 41234, "127.0.0.1", "C:\\scratch")
}

#[test]
fn the_fingerprint_round_trips_through_the_port_file() {
    let rec = sample();
    let bytes = serialize_entry(&rec, Some("fp-1")).expect("serialize");
    let entry = parse_entry(&bytes).expect("parse");
    assert_eq!(entry.config_fingerprint.as_deref(), Some("fp-1"));
    assert_eq!(
        entry.record, rec,
        "the record itself must survive the extra key"
    );
}

#[test]
fn the_fingerprint_is_an_extra_top_level_key_and_the_record_keeps_its_shape() {
    // 第三方（proxy 二进制、`scripts/mcp-repl.mjs`）按顶层 `port` 读这个文件，
    // 故指纹只能是一个额外的顶层键，不能改变既有键的形状。
    let rec = sample();
    let bytes = serialize_entry(&rec, Some("fp-1")).expect("serialize");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value["port"], 41234);
    assert_eq!(value["pid"], 0);
    assert_eq!(value["bind"], "127.0.0.1");
    assert_eq!(value[CONFIG_FINGERPRINT_KEY], "fp-1");
}

#[test]
fn a_record_without_a_fingerprint_still_parses_and_reports_no_fingerprint() {
    // 旧端口文件里没有这一项：记录照旧读得出来，而指纹那一项是「判断不了」。
    let rec = sample();
    let bytes = serde_json::to_vec(&rec).expect("serialize");
    let entry = parse_entry(&bytes).expect("parse");
    assert_eq!(entry.record, rec);
    assert!(entry.config_fingerprint.is_none());
}

#[test]
fn writing_without_a_fingerprint_leaves_the_key_out() {
    // daemon 侧（`lib.rs`）用的就是不带指纹的写法：它拿不到那份配置。
    let rec = sample();
    let bytes = serialize_entry(&rec, None).expect("serialize");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert!(value.get(CONFIG_FINGERPRINT_KEY).is_none());
    let entry = parse_entry(&bytes).expect("parse");
    assert!(entry.config_fingerprint.is_none());
}
