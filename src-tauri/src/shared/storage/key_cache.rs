//! 本机派生钥缓存（系统凭据库）：日常启动免输库密码，与现状体验一致。
//! 跨设备永不用缓存——新设备无缓存，自然落到"输入库密码"路径；
//! 换库密码后缓存随之更新，旧缓存立即失效。

use base64::{engine::general_purpose, Engine};

const SERVICE: &str = "password-safer";
const USER: &str = "vault-derived-key";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USER).map_err(|e| format!("凭据库不可用: {}", e))
}

/// 缓存派生钥（base64 存储）
pub fn cache_key(key: &[u8; 32]) -> Result<(), String> {
    let encoded = general_purpose::STANDARD.encode(key);
    entry()?
        .set_password(&encoded)
        .map_err(|e| format!("写入凭据库失败: {}", e))
}

/// 读取缓存的派生钥；无缓存返回 None
pub fn load_cached_key() -> Option<[u8; 32]> {
    let encoded = entry().ok()?.get_password().ok()?;
    let bytes = general_purpose::STANDARD.decode(encoded).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Some(key)
}

/// 清除缓存（改钥失败回滚、显式锁定等场景）
pub fn clear_cache() {
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
}
