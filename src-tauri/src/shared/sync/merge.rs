//! 冲突记录级合并（纯函数）：两侧解密后的 StoreData 按密码记录 id 对齐。
//!
//! 语义（拍板 S1 重构）：冲突**不再挂起阻塞同步**——同 id 内容不同的记录
//! **两个版本都保留**（云端版分配新 id），合并结果立即推送为新 head，
//! 记录对返回给调用方写进 sync_state 公告，由用户在首页逐对裁决删留。
//! 仅一侧存在的记录一律保留（StoreData 无墓碑，误复活好过误删除）；
//! config 不逐字段合并，整体取调用方指定的侧。

use crate::shared::storage::json_store::{PasswordRecord, StoreData};
use std::collections::HashMap;

/// 一对冲突记录：本机版保留原 id，云端版已分配新 id
#[derive(Debug, Clone)]
pub struct MergePair {
    pub local: PasswordRecord,
    pub remote: PasswordRecord,
    /// 云端版是否较新（公告展示提示用）
    pub remote_newer: bool,
}

/// 记录级差异（不含 id 与簿记字段）
pub fn records_differ(a: &PasswordRecord, b: &PasswordRecord) -> bool {
    a.name != b.name
        || a.icon != b.icon
        || a.url != b.url
        || a.username != b.username
        || a.password_encrypted != b.password_encrypted
        || a.tags != b.tags
        || a.notes != b.notes
        || a.favorite != b.favorite
        || a.strength != b.strength
}

/// 较新判定（拍板 S5）：modified_ms 大者新；一方为 0（旧数据）退化用
/// last_used 字符串比较；仍相同视为本机新。
pub fn remote_is_newer(local: &PasswordRecord, remote: &PasswordRecord) -> bool {
    if local.modified_ms > 0 && remote.modified_ms > 0 {
        return remote.modified_ms > local.modified_ms;
    }
    remote.last_used > local.last_used
}

/// 并集合并（两版都保留）：返回合并结果与冲突记录对。
/// 云端版记录在本机版之后追加，id 从现有最大 +1 起顺延。
pub fn merge_union_keep_both(
    local: StoreData,
    remote: StoreData,
    config_take_remote: bool,
) -> (StoreData, Vec<MergePair>) {
    let mut next_id = local
        .passwords
        .iter()
        .map(|r| r.id)
        .chain(remote.passwords.iter().map(|r| r.id))
        .max()
        .map(|m| m + 1)
        .unwrap_or(local.next_id.max(1));
    let mut by_id: HashMap<i64, PasswordRecord> = HashMap::new();
    for r in local.passwords {
        by_id.insert(r.id, r);
    }

    let mut pairs = Vec::new();
    for r in remote.passwords {
        let conflict = match by_id.get(&r.id) {
            None => {
                // 仅云端存在：直接收编（非冲突，不进公告）
                by_id.insert(r.id, r);
                None
            }
            Some(local_r) => {
                if records_differ(local_r, &r) {
                    // 双侧同 id 内容不同：两版都保留，云端版换新 id
                    let local_r = local_r.clone();
                    let mut copy = r.clone();
                    copy.id = next_id;
                    next_id += 1;
                    let remote_newer = remote_is_newer(&local_r, &copy);
                    Some((local_r, copy, remote_newer))
                } else {
                    None
                }
            }
        };
        if let Some((local_r, copy, remote_newer)) = conflict {
            by_id.insert(copy.id, copy.clone());
            pairs.push(MergePair {
                local: local_r,
                remote: copy,
                remote_newer,
            });
        }
    }

    let mut passwords: Vec<PasswordRecord> = by_id.into_values().collect();
    passwords.sort_by_key(|r| r.id);

    (
        StoreData {
            passwords,
            config: if config_take_remote {
                remote.config
            } else {
                local.config
            },
            version: local.version,
            next_id,
        },
        pairs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: i64, name: &str, modified_ms: i64) -> PasswordRecord {
        PasswordRecord {
            id,
            name: name.to_string(),
            icon: "🔑".to_string(),
            url: String::new(),
            username: "user".to_string(),
            password_encrypted: format!("enc-{}", name),
            tags: vec![],
            notes: String::new(),
            favorite: false,
            strength: 2,
            created: "2026-01-01".to_string(),
            last_used: "2026-01-01".to_string(),
            modified_ms,
        }
    }

    fn vault(records: Vec<PasswordRecord>) -> StoreData {
        StoreData {
            passwords: records,
            config: Default::default(),
            version: 1,
            next_id: 100,
        }
    }

    #[test]
    fn disjoint_union_keeps_both_without_pairs() {
        let (m, pairs) = merge_union_keep_both(
            vault(vec![record(1, "仅本地", 10)]),
            vault(vec![record(2, "仅云端", 20)]),
            false,
        );
        let ids: Vec<i64> = m.passwords.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1, 2]);
        assert!(pairs.is_empty(), "仅一侧存在的记录不是冲突");
    }

    #[test]
    fn identical_records_merge_silently() {
        let (m, pairs) = merge_union_keep_both(
            vault(vec![record(1, "同名", 10)]),
            vault(vec![record(1, "同名", 10)]),
            true,
        );
        assert_eq!(m.passwords.len(), 1);
        assert!(pairs.is_empty());
    }

    #[test]
    fn conflicting_record_keeps_both_versions() {
        let (m, pairs) = merge_union_keep_both(
            vault(vec![record(1, "本机版", 100)]),
            vault(vec![record(1, "云端版", 200)]),
            false,
        );
        assert_eq!(m.passwords.len(), 2);
        assert_eq!(pairs.len(), 1);
        let p = &pairs[0];
        // 本机版保留原 id，云端版拿新 id
        assert_eq!(p.local.id, 1);
        assert_eq!(p.remote.id, m.next_id - 1);
        assert_eq!(p.remote.name, "云端版");
        // 云端 modified_ms 更大 → 标记云端较新
        assert!(p.remote_newer);
        // 两版内容都在
        assert!(m.passwords.iter().any(|r| r.password_encrypted == "enc-本机版"));
        assert!(m.passwords.iter().any(|r| r.password_encrypted == "enc-云端版"));
    }

    #[test]
    fn newer_judgement_falls_back_to_last_used_for_legacy() {
        // 旧数据 modified_ms = 0 → 退化用 last_used
        let mut l = record(1, "旧数据本机", 0);
        l.last_used = "2026-01-01".to_string();
        let mut r = record(1, "旧数据云端", 0);
        r.last_used = "2026-02-01".to_string();
        let (_, pairs) = merge_union_keep_both(vault(vec![l]), vault(vec![r]), false);
        assert!(pairs[0].remote_newer);
    }

    #[test]
    fn config_takes_designated_side() {
        let mut local = vault(vec![]);
        local.config.auto_lock_minutes = 11;
        let mut remote = vault(vec![]);
        remote.config.auto_lock_minutes = 22;

        assert_eq!(
            merge_union_keep_both(local.clone(), remote.clone(), true).0.config.auto_lock_minutes,
            22
        );
        assert_eq!(
            merge_union_keep_both(local, remote, false).0.config.auto_lock_minutes,
            11
        );
    }

    #[test]
    fn next_id_covers_all_records() {
        let (m, _) = merge_union_keep_both(
            vault(vec![record(1, "a", 1)]),
            vault(vec![record(7, "b", 1), record(1, "冲突副本", 2)]),
            false,
        );
        // 冲突副本拿 id 8，next_id 随之指向 9
        assert_eq!(m.next_id, 9);
        assert!(m.passwords.iter().any(|r| r.id == 8 && r.name == "冲突副本"));
    }

    #[test]
    fn empty_sides_degrade_gracefully() {
        let (m, pairs) = merge_union_keep_both(vault(vec![]), vault(vec![]), true);
        assert!(m.passwords.is_empty());
        assert!(pairs.is_empty());
        // 无任何记录时回退到 local.next_id（vault() 里是 100）
        assert_eq!(m.next_id, 100);
    }
}
