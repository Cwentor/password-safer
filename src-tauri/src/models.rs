use serde::{Deserialize, Serialize};

/// 前端交互的密码数据传输对象（密码已解密）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordDto {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub tags: Vec<String>,
    pub notes: String,
    pub favorite: bool,
    pub strength: i32,
    pub created: String,
    pub last_used: String,
}

/// 新建/编辑密码时的输入
#[derive(Debug, Clone, Deserialize)]
pub struct PasswordInput {
    pub name: String,
    pub icon: Option<String>,
    pub url: Option<String>,
    pub username: Option<String>,
    pub password: String,
    pub tags: Option<Vec<String>>,
    pub notes: Option<String>,
    pub favorite: Option<bool>,
}

/// 应用配置（同步设置等）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    // 百度网盘（Cookie 方式）
    pub baidu_cookie: String,
    pub baidu_remote_path: String,
    pub baidu_sync_enabled: bool,
    pub baidu_sync_interval: u64,
    pub baidu_cookie_expires_at: i64,
    pub baidu_last_sync: String,
    // 夸克网盘（Cookie 方式）
    pub quark_cookie: String,
    pub quark_remote_path: String,
    pub quark_sync_enabled: bool,
    pub quark_sync_interval: u64,
    pub quark_cookie_expires_at: i64,
    pub quark_last_sync: String,
    pub quark_last_remote_mtime: i64,
    // 通用设置
    pub auto_lock_minutes: u64,
    pub clipboard_clear_seconds: u64,
    pub master_password: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            baidu_cookie: String::new(),
            baidu_remote_path: "/apps/VAULT/vault.db".to_string(),
            baidu_sync_enabled: false,
            baidu_sync_interval: 300,
            baidu_cookie_expires_at: 0,
            baidu_last_sync: String::new(),
            quark_cookie: String::new(),
            quark_remote_path: "/VAULT/vault.db".to_string(),
            quark_sync_enabled: false,
            quark_sync_interval: 300,
            quark_cookie_expires_at: 0,
            quark_last_sync: String::new(),
            quark_last_remote_mtime: 0,
            auto_lock_minutes: 30,
            clipboard_clear_seconds: 30,
            master_password: String::new(),
        }
    }
}

/// 同步结果
#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    pub success: bool,
    pub message: String,
    pub synced_at: String,
}

/// 扫码轮询结果（前端友好，不暴露 Cookie 原文）
#[derive(Debug, Clone, Serialize)]
pub struct QrPollResult {
    pub status: String,
    pub message: String,
}

/// 数据库导入结果
#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub success: bool,
    pub message: String,
    pub imported_count: usize,
}

/// 密码强度计算
pub fn calculate_strength(password: &str) -> i32 {
    let mut score = 0;
    if password.len() >= 8 {
        score += 1;
    }
    if password.len() >= 12 {
        score += 1;
    }
    if password.chars().any(|c| c.is_uppercase()) && password.chars().any(|c| c.is_lowercase()) {
        score += 1;
    }
    if password.chars().any(|c| c.is_numeric()) && password.chars().any(|c| !c.is_alphanumeric()) {
        score += 1;
    }
    score.max(1).min(4)
}
