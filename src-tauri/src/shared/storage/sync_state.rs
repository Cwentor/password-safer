//! 云同步本地记账（sync_state.json，与密码簿同目录、不上云）。
//!
//! vault.json 是要上云的 E2E 密文，任何同步记账写进去都会形成回声：
//! 记账 → 文件变化 → 下轮被判为"本地较新" → 再同步 → 再记账。
//! 因此设备标识、同步时间、游标等副状态全部住在这个本地文件里。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 单个网盘的同步分账
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderSyncState {
    /// 已知的远端 meta head 文件名（追加式同步阶段使用）
    #[serde(default)]
    pub last_heads: Vec<String>,
    /// 最近一次同步完成时间（展示用）
    #[serde(default)]
    pub last_sync: String,
}

/// 云同步本地记账
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// 本设备标识（meta 命名与冲突裁决显示用），首启随机生成
    pub device_id: String,
    /// 本地密码簿最后一次被"用户写"的时间（unix 秒）；同步写路径不刷新
    pub local_dirty_at: i64,
    /// 夸克网盘分账
    #[serde(default)]
    pub quark: ProviderSyncState,
    /// 百度网盘分账
    #[serde(default)]
    pub baidu: ProviderSyncState,
    /// 冲突挂起状态（冲突裁决阶段使用）
    #[serde(default)]
    pub conflict_pending: Option<serde_json::Value>,
    /// legacy 远端 vault.json 待补删标记
    #[serde(default)]
    pub legacy_pending_delete: bool,
    /// 记账文件路径（不序列化）
    #[serde(skip)]
    path: PathBuf,
}

impl SyncState {
    /// 读取本地记账；不存在或损坏则重建（device_id 重新生成并立即落盘）
    pub fn load_or_create(path: PathBuf) -> Result<Self, String> {
        if let Ok(bytes) = std::fs::read(&path) {
            match serde_json::from_slice::<SyncState>(&bytes) {
                Ok(mut s) => {
                    s.path = path;
                    return Ok(s);
                }
                Err(e) => {
                    // 记账文件损坏不致命：重建只丢展示信息与游标，远端以 list 为准
                    eprintln!("[sync_state] 解析失败（{}），重建", e);
                }
            }
        }
        let mut s = Self::new();
        s.path = path;
        s.save()?;
        Ok(s)
    }

    fn new() -> Self {
        SyncState {
            device_id: generate_device_id(),
            local_dirty_at: 0,
            quark: ProviderSyncState::default(),
            baidu: ProviderSyncState::default(),
            conflict_pending: None,
            legacy_pending_delete: false,
            path: PathBuf::new(),
        }
    }

    /// 原子落盘（临时文件 + 改名，与密码簿同策略）
    pub fn save(&self) -> Result<(), String> {
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("序列化 sync_state 失败: {}", e))?;
        std::fs::write(&tmp, bytes).map_err(|e| format!("写入 sync_state 失败: {}", e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("替换 sync_state 失败: {}", e))
    }

    /// 按网盘名取分账（未知网盘返回 None）
    pub fn provider_mut(&mut self, name: &str) -> Option<&mut ProviderSyncState> {
        match name {
            "quark" => Some(&mut self.quark),
            "baidu" => Some(&mut self.baidu),
            _ => None,
        }
    }
}

/// 生成 8 位十六进制设备标识
fn generate_device_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ps-sync-state-{}-{}-{}",
            std::process::id(),
            tag,
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("sync_state.json")
    }

    #[test]
    fn create_then_reload_keeps_device_id() {
        let path = temp_file("reload");
        let s1 = SyncState::load_or_create(path.clone()).unwrap();
        assert!(!s1.device_id.is_empty());
        assert_eq!(s1.local_dirty_at, 0);

        let s2 = SyncState::load_or_create(path).unwrap();
        assert_eq!(s1.device_id, s2.device_id);
    }

    #[test]
    fn provider_state_roundtrip() {
        let path = temp_file("provider");
        let mut s = SyncState::load_or_create(path.clone()).unwrap();
        s.provider_mut("quark").unwrap().last_sync = "2026-11-01 12:00:00".to_string();
        s.provider_mut("quark").unwrap().last_heads = vec!["meta_1.json".to_string()];
        s.save().unwrap();

        let mut s2 = SyncState::load_or_create(path).unwrap();
        assert_eq!(s2.quark.last_sync, "2026-11-01 12:00:00");
        assert_eq!(s2.quark.last_heads, vec!["meta_1.json".to_string()]);
        assert!(s2.baidu.last_sync.is_empty());
        assert!(s2.provider_mut("unknown").is_none());
    }

    #[test]
    fn corrupt_file_is_rebuilt() {
        let path = temp_file("corrupt");
        let s1 = SyncState::load_or_create(path.clone()).unwrap();
        std::fs::write(&path, b"not json at all").unwrap();

        let s2 = SyncState::load_or_create(path.clone()).unwrap();
        // 重建后 device_id 换新，且文件可再次加载
        assert!(!s2.device_id.is_empty());
        assert!(SyncState::load_or_create(path).is_ok());
        drop(s1);
    }
}
