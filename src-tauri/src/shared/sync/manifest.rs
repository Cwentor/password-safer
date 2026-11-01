//! 追加式同步的元数据层：meta 快照（Manifest）的构造、解析与 head 解析。
//!
//! 远端布局：`/meta/meta_<UTC毫秒>_<device_id>.json` 为提交点，
//! `/blobs/<sha256>.bin` 为不可变密文实体。meta 内带 `supersedes` 记录它已知
//! 的一切 head；**head 集合 = 未被任何 head meta 的 supersedes 引用的 head**。
//! 没有服务器分配版本号，并发 push 天然产生 fork，由调用方做冲突裁决；
//! `kind: "backup"` 的 meta 永不参与 head 解析（冲突备份的可寻址载体）。

// 【allow(dead_code)：阶段 3 session 接线后移除】
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// files 条目：一个语义路径对应的不可变 blob 指针
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    /// blob 文件名（不含路径），= 内容的 sha256 十六进制
    pub blob: String,
    pub size: i64,
    /// unix 毫秒
    pub updated_at: i64,
    /// 逻辑删除标记（实体 blob 永不删除）
    #[serde(default)]
    pub deleted: bool,
}

/// meta 类型：head 参与同步裁决；backup 是冲突备份，钉住 blob 供找回
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetaKind {
    Head,
    Backup,
}

fn default_kind() -> MetaKind {
    MetaKind::Head
}

/// 一次提交的元数据快照
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    #[serde(default = "default_kind")]
    pub kind: MetaKind,
    /// unix 毫秒
    pub updated_at: i64,
    pub device_id: String,
    /// 本 meta 已知的一切 head meta 文件名
    #[serde(default)]
    pub supersedes: Vec<String>,
    /// 语义路径 → blob 指针（BTreeMap 保证序列化顺序稳定）
    pub files: BTreeMap<String, FileEntry>,
}

/// 内容寻址：blob 名 = sha256 十六进制（密文哈希，不含明文信息）
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

/// 生成 meta 文件名（UTC 毫秒 + 设备号保证唯一、可解析出顺序）
pub fn new_filename(device_id: &str, updated_at_ms: i64) -> String {
    format!("meta_{}_{}.json", updated_at_ms, device_id)
}

/// 解析 meta 文件名 → (UTC 毫秒, 设备号)；非 meta 命名返回 None
pub fn parse_filename(name: &str) -> Option<(i64, &str)> {
    let rest = name.strip_prefix("meta_")?.strip_suffix(".json")?;
    let (ms_str, device) = rest.split_once('_')?;
    let ms = ms_str.parse::<i64>().ok()?;
    if ms <= 0 || device.is_empty() {
        return None;
    }
    Some((ms, device))
}

impl Manifest {
    /// 构造一份 head meta（files 由调用方填充）
    pub fn new_head(device_id: &str, updated_at_ms: i64) -> Self {
        Manifest {
            kind: MetaKind::Head,
            updated_at: updated_at_ms,
            device_id: device_id.to_string(),
            supersedes: Vec::new(),
            files: BTreeMap::new(),
        }
    }

    /// 校验结构合法性（解析与测试共用）
    fn validate(&self) -> Result<(), String> {
        if self.updated_at <= 0 {
            return Err("updated_at 非法".to_string());
        }
        if self.device_id.is_empty() {
            return Err("device_id 为空".to_string());
        }
        for (path, entry) in &self.files {
            if !is_sha256_hex(&entry.blob) {
                return Err(format!("文件 '{}' 的 blob 名不是 sha256 十六进制: {}", path, entry.blob));
            }
            if entry.size < 0 {
                return Err(format!("文件 '{}' 的 size 非法", path));
            }
        }
        if self.supersedes.iter().any(|s| s.is_empty()) {
            return Err("supersedes 含空引用".to_string());
        }
        Ok(())
    }

    /// 序列化为 JSON 字节（上传用）
    pub fn serialize(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(self).map_err(|e| format!("meta 序列化失败: {}", e))
    }

    /// 从字节解析并校验
    pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
        let m: Manifest = serde_json::from_slice(bytes).map_err(|e| format!("meta 解析失败: {}", e))?;
        m.validate()?;
        Ok(m)
    }
}

/// 批量解析，跳过非法文件（远端 /meta 里混入垃圾不应让同步崩溃）
pub fn parse_all(entries: &[(String, Vec<u8>)]) -> Vec<(String, Manifest)> {
    entries
        .iter()
        .filter_map(|(name, bytes)| match Manifest::parse(bytes) {
            Ok(m) => Some((name.clone(), m)),
            Err(e) => {
                eprintln!("[manifest] 跳过非法 meta '{}': {}", name, e);
                None
            }
        })
        .collect()
}

/// head 解析：head 集合 = 未被任何 head meta 的 supersedes 引用的 head。
/// backup meta 完全不参与（既不作为 head，其 supersedes 也被忽略）；
/// supersedes 允许引用不在列表中的名字（按名生效，容错另一端已整理的历史）。
/// 返回 head 文件名列表（排序保证确定性）。
pub fn resolve_heads(metas: &[(String, Manifest)]) -> Vec<String> {
    let is_head = |m: &Manifest| m.kind == MetaKind::Head;
    let heads: HashSet<&String> = metas
        .iter()
        .filter(|(_, m)| is_head(m))
        .map(|(n, _)| n)
        .collect();
    let superseded: HashSet<&String> = metas
        .iter()
        .filter(|(_, m)| is_head(m))
        .flat_map(|(_, m)| m.supersedes.iter())
        .collect();
    let mut result: Vec<String> = heads
        .difference(&superseded)
        .map(|s| (*s).clone())
        .collect();
    result.sort();
    result
}

/// 在一组 meta 里按 (updated_at, device_id) 取最新
/// （fork 的展示与合并默认侧用；确定性裁决，同组输入任何客户端结果一致）
pub fn newest<'a>(metas: &'a [(String, Manifest)]) -> Option<&'a Manifest> {
    metas
        .iter()
        .max_by(|a, b| {
            (a.1.updated_at, a.1.device_id.clone()).cmp(&(b.1.updated_at, b.1.device_id.clone()))
        })
        .map(|(_, m)| m)
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(blob_seed: u8) -> FileEntry {
        FileEntry {
            blob: sha256_hex(&[blob_seed]),
            size: 100,
            updated_at: 1000,
            deleted: false,
        }
    }

    fn head(name: &str, ms: i64, device: &str, supersedes: &[&str]) -> (String, Manifest) {
        let mut m = Manifest::new_head(device, ms);
        m.files.insert("vault.json".to_string(), entry(1));
        m.supersedes = supersedes.iter().map(|s| s.to_string()).collect();
        (name.to_string(), m)
    }

    // ---- 命名 ----

    #[test]
    fn filename_roundtrip() {
        let name = new_filename("a1b2c3d4", 1730000000000);
        assert_eq!(parse_filename(&name), Some((1730000000000, "a1b2c3d4")));
        assert!(parse_filename("vault.json").is_none());
        assert!(parse_filename("meta_abc_dev.json").is_none());
        assert!(parse_filename("meta_0_dev.json").is_none());
        assert!(parse_filename("meta_1730000000000_.json").is_none());
    }

    // ---- 序列化与校验 ----

    #[test]
    fn serialize_parse_roundtrip() {
        let (_, m) = head("meta_1_a.json", 1000, "dev1", &["meta_0_b.json"]);
        let bytes = m.serialize().unwrap();
        let parsed = Manifest::parse(&bytes).unwrap();
        assert_eq!(parsed, m);
        assert_eq!(parsed.kind, MetaKind::Head);
    }

    #[test]
    fn parse_rejects_garbage_and_bad_fields() {
        assert!(Manifest::parse(b"not json").is_err());
        // blob 名不是 sha256
        let mut m = Manifest::new_head("dev", 1000);
        m.files.insert("vault.json".to_string(), FileEntry {
            blob: "deadbeef".to_string(),
            size: 1,
            updated_at: 1,
            deleted: false,
        });
        assert!(Manifest::parse(&m.serialize().unwrap()).is_err());
        // updated_at 非法
        let m2 = Manifest::new_head("dev", 0);
        assert!(Manifest::parse(&m2.serialize().unwrap()).is_err());
        // 空设备号
        let m3 = Manifest::new_head("", 1000);
        assert!(Manifest::parse(&m3.serialize().unwrap()).is_err());
    }

    #[test]
    fn parse_all_skips_invalid() {
        let (_, m1) = head("meta_1_a.json", 1000, "dev1", &[]);
        let entries = vec![
            ("meta_1_a.json".to_string(), m1.serialize().unwrap()),
            ("meta_2_broken.json".to_string(), b"garbage".to_vec()),
        ];
        let parsed = parse_all(&entries);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "meta_1_a.json");
    }

    // ---- head 解析 ----

    #[test]
    fn single_meta_is_head() {
        let metas = vec![head("meta_1_a.json", 1000, "dev1", &[])];
        assert_eq!(resolve_heads(&metas), vec!["meta_1_a.json"]);
    }

    #[test]
    fn linear_chain_single_head() {
        let metas = vec![
            head("meta_1_a.json", 1000, "dev1", &[]),
            head("meta_2_b.json", 2000, "dev1", &["meta_1_a.json"]),
            head("meta_3_c.json", 3000, "dev1", &["meta_2_b.json"]),
        ];
        assert_eq!(resolve_heads(&metas), vec!["meta_3_c.json"]);
    }

    #[test]
    fn fork_produces_two_heads() {
        let metas = vec![
            head("meta_1_base.json", 1000, "dev1", &[]),
            head("meta_2_a.json", 2000, "devA", &["meta_1_base.json"]),
            head("meta_2_b.json", 2000, "devB", &["meta_1_base.json"]),
        ];
        assert_eq!(resolve_heads(&metas), vec!["meta_2_a.json", "meta_2_b.json"]);
    }

    #[test]
    fn backup_meta_never_head_and_never_supersedes() {
        // backup 指向的 blob 只是为了可寻址：不参与 head，也不剥夺别人的 head 身份
        let mut backup = Manifest::new_head("devX", 5000);
        backup.kind = MetaKind::Backup;
        backup.files.insert("vault.json".to_string(), entry(2));
        backup.supersedes = vec!["meta_2_a.json".to_string()];
        let metas = vec![
            head("meta_1_base.json", 1000, "dev1", &[]),
            head("meta_2_a.json", 2000, "devA", &["meta_1_base.json"]),
            ("meta_9_backup.json".to_string(), backup),
        ];
        assert_eq!(resolve_heads(&metas), vec!["meta_2_a.json"]);
    }

    #[test]
    fn supersedes_missing_name_still_counts() {
        // 引用不在列表中的名字：不影响解析，meta_1_a 仍是 head（按名容错）
        let metas = vec![
            head("meta_1_a.json", 1000, "dev1", &["meta_0_ghost.json"]),
        ];
        assert_eq!(resolve_heads(&metas), vec!["meta_1_a.json"]);
    }

    // ---- newest 裁决 ----

    #[test]
    fn newest_by_updated_at() {
        let metas = vec![
            head("meta_1_a.json", 1000, "devA", &[]),
            head("meta_2_b.json", 2000, "devB", &[]),
        ];
        assert_eq!(newest(&metas).unwrap().device_id, "devB");
    }

    #[test]
    fn newest_tie_break_by_device_id() {
        let metas = vec![
            head("meta_1_a.json", 2000, "devB", &[]),
            head("meta_2_b.json", 2000, "devC", &[]),
        ];
        assert_eq!(newest(&metas).unwrap().device_id, "devC");
    }

    #[test]
    fn newest_empty_is_none() {
        assert!(newest(&[]).is_none());
    }

    // ---- sha256 ----

    #[test]
    fn sha256_hex_matches_known_vector() {
        // sha256("abc") 的标准测试向量
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
