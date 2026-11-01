//! 冲突记录级合并（纯函数）：两侧解密后的 StoreData 按密码记录 id 对齐取舍。
//!
//! 语义（拍板 v2）：
//! - passwords 按 id 取**并集**：仅一侧存在的记录一律保留——StoreData 没有墓碑，
//!   无法区分"本侧删除"与"另一侧新建"，误复活好过误删除；
//! - 双侧同 id 但内容不同的记录按调用方给的选择取舍，缺省取较新者
//!   （last_used 大者优先，相同取本地）；
//! - config（Cookie、设置等）不逐字段合并，整体取调用方指定的较新侧。

use crate::shared::storage::json_store::{PasswordRecord, StoreData};
use std::collections::HashMap;

/// 单条冲突记录的取舍
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Local,
    Remote,
}

impl Choice {
    /// 从 "local"/"remote" 字符串解析（前端 selections 用）
    pub fn parse(s: &str) -> Option<Choice> {
        match s {
            "local" => Some(Choice::Local),
            "remote" => Some(Choice::Remote),
            _ => None,
        }
    }
}

/// 记录级差异（不含 id 与 next_id 等簿记字段）
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

/// 较新者判定：last_used 字符串按字典序（YYYY-MM-DD）比较，相同取本地
pub fn prefer_local(a: &PasswordRecord, b: &PasswordRecord) -> bool {
    a.last_used >= b.last_used
}

/// 合并两侧密码簿。
/// `choices`：双侧同 id 且内容不同时的取舍；缺失的 id 用缺省规则（较新者胜）。
/// `config_take_local`：config 整体取本地（false 取云端）。
pub fn merge_vaults(
    local: StoreData,
    remote: StoreData,
    config_take_local: bool,
    choices: &HashMap<i64, Choice>,
) -> StoreData {
    let mut merged_ids: Vec<i64> = Vec::new();
    let mut by_id: HashMap<i64, PasswordRecord> = HashMap::new();

    for r in local.passwords {
        merged_ids.push(r.id);
        by_id.insert(r.id, r);
    }
    for r in remote.passwords {
        match by_id.get_mut(&r.id) {
            None => {
                // 仅云端存在：保留（并集语义）
                merged_ids.push(r.id);
                by_id.insert(r.id, r);
            }
            Some(local_r) => {
                // 双侧同 id：内容相同取本地原样；不同按选择/缺省较新者
                if records_differ(local_r, &r) {
                    let take = match choices.get(&r.id) {
                        Some(Choice::Local) => true,
                        Some(Choice::Remote) => false,
                        None => prefer_local(local_r, &r),
                    };
                    if !take {
                        *local_r = r;
                    }
                }
            }
        }
    }

    // 稳定输出：按 id 升序
    let mut passwords: Vec<PasswordRecord> = merged_ids
        .into_iter()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect();
    passwords.sort_by_key(|r| r.id);

    // next_id 覆盖两侧所有 id
    let next_id = passwords
        .iter()
        .map(|r| r.id)
        .max()
        .map(|m| m + 1)
        .unwrap_or(1);

    StoreData {
        passwords,
        config: if config_take_local {
            local.config
        } else {
            remote.config
        },
        version: local.version,
        next_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: i64, name: &str, last_used: &str) -> PasswordRecord {
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
            last_used: last_used.to_string(),
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
    fn disjoint_union_keeps_both() {
        let m = merge_vaults(
            vault(vec![record(1, "仅本地", "2026-01-01")]),
            vault(vec![record(2, "仅云端", "2026-01-02")]),
            true,
            &HashMap::new(),
        );
        let ids: Vec<i64> = m.passwords.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn identical_records_take_local() {
        let m = merge_vaults(
            vault(vec![record(1, "同名", "2026-01-01")]),
            vault(vec![record(1, "同名", "2026-01-01")]),
            true,
            &HashMap::new(),
        );
        assert_eq!(m.passwords.len(), 1);
        assert_eq!(m.passwords[0].password_encrypted, "enc-同名");
    }

    #[test]
    fn differing_record_default_prefers_newer() {
        let m = merge_vaults(
            vault(vec![record(1, "旧", "2026-01-01")]),
            vault(vec![record(1, "新", "2026-02-01")]),
            true,
            &HashMap::new(),
        );
        assert_eq!(m.passwords[0].name, "新");
    }

    #[test]
    fn differing_record_tie_prefers_local() {
        let m = merge_vaults(
            vault(vec![record(1, "本地", "2026-01-01")]),
            vault(vec![record(1, "云端", "2026-01-01")]),
            true,
            &HashMap::new(),
        );
        assert_eq!(m.passwords[0].name, "本地");
    }

    #[test]
    fn explicit_choice_overrides_default() {
        let mut choices = HashMap::new();
        choices.insert(1, Choice::Local);
        let m = merge_vaults(
            vault(vec![record(1, "旧但用户选了它", "2026-01-01")]),
            vault(vec![record(1, "新", "2026-02-01")]),
            true,
            &choices,
        );
        assert_eq!(m.passwords[0].name, "旧但用户选了它");

        let mut choices = HashMap::new();
        choices.insert(1, Choice::Remote);
        let m = merge_vaults(
            vault(vec![record(1, "本地", "2026-01-01")]),
            vault(vec![record(1, "云端覆盖", "2026-01-01")]),
            true,
            &choices,
        );
        assert_eq!(m.passwords[0].name, "云端覆盖");
    }

    #[test]
    fn config_takes_designated_side() {
        let mut local = vault(vec![]);
        local.config.auto_lock_minutes = 11;
        let mut remote = vault(vec![]);
        remote.config.auto_lock_minutes = 22;

        assert_eq!(merge_vaults(local.clone(), remote.clone(), true, &HashMap::new()).config.auto_lock_minutes, 11);
        assert_eq!(merge_vaults(local, remote, false, &HashMap::new()).config.auto_lock_minutes, 22);
    }

    #[test]
    fn next_id_covers_all_records() {
        let m = merge_vaults(
            vault(vec![record(1, "a", "2026-01-01")]),
            vault(vec![record(7, "b", "2026-01-01")]),
            true,
            &HashMap::new(),
        );
        assert_eq!(m.next_id, 8);
    }

    #[test]
    fn empty_sides_degrade_gracefully() {
        let m = merge_vaults(vault(vec![]), vault(vec![]), true, &HashMap::new());
        assert!(m.passwords.is_empty());
        assert_eq!(m.next_id, 1);

        let m = merge_vaults(vault(vec![record(3, "x", "2026-01-01")]), vault(vec![]), true, &HashMap::new());
        assert_eq!(m.passwords.len(), 1);
    }
}
