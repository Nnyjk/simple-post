//! 指纹去重：每个端点的下场（写还是跳过），以及容器里还有没有要写的。

use std::collections::BTreeSet;

use crate::fingerprint::{FingerprintSet, RequestIdentity};
use crate::ir::{endpoint_keys_of, IrCollection, IrEndpoint, IrModule};

/// 一个被指纹认出来而跳过的请求。名字与请求描述给报告用。
pub(super) struct SkippedDuplicate {
    pub(super) name: String,
    pub(super) method: String,
    pub(super) path: String,
}

/// 每个端点的下场：会被写入，还是已被认出而跳过。
pub(super) struct Fate {
    pub(super) writable: BTreeSet<String>,
    pub(super) duplicates: Vec<SkippedDuplicate>,
}

/// 按写入顺序走一遍，判定每个勾中的端点是写还是跳过。
///
/// `ledger` 是去重账本，本遍会往里记账，故调用方必须传一份克隆。顺序必须与真正落库
/// 一致（先本层 endpoint，再各 collection 依次深入），否则「哪些是重复」会有另一套答案。
pub(super) fn decide(
    modules: &[IrModule],
    ledger: &mut FingerprintSet,
    selected: &BTreeSet<String>,
) -> Fate {
    let mut writable = BTreeSet::new();
    let mut duplicates = Vec::new();
    let mut claim = |ep: &IrEndpoint| {
        if !selected.contains(&ep.key) {
            return;
        }
        if ledger.seen(&RequestIdentity::of_ir(ep)) {
            duplicates.push(SkippedDuplicate {
                name: ep.name.clone(),
                method: ep.method.as_str().to_string(),
                path: ep.path.clone(),
            });
        } else {
            writable.insert(ep.key.clone());
        }
    };
    for m in modules {
        for ep in &m.endpoints {
            claim(ep);
        }
        let mut stack: Vec<&IrCollection> = m.collections.iter().rev().collect();
        while let Some(c) = stack.pop() {
            for ep in &c.endpoints {
                claim(ep);
            }
            stack.extend(c.collections.iter().rev());
        }
    }
    Fate {
        writable,
        duplicates,
    }
}

/// 这个分组里还有没有会被写入的端点。空分组不写。
pub(super) fn has_any(m: &IrModule, writable: &BTreeSet<String>) -> bool {
    endpoint_keys_of(&m.endpoints, &m.collections)
        .iter()
        .any(|k| writable.contains(k))
}

pub(super) fn has_any_collection(c: &IrCollection, writable: &BTreeSet<String>) -> bool {
    endpoint_keys_of(&c.endpoints, &c.collections)
        .iter()
        .any(|k| writable.contains(k))
}
