use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::shared::models::AppConfig;

/// 配置管理器：从 SQLite app_config 表读写配置
pub struct ConfigManager {
    conn: Mutex<Connection>,
}

#[allow(dead_code)]
impl ConfigManager {
    pub fn new(conn: Connection) -> Self {
        ConfigManager {
            conn: Mutex::new(conn),
        }
    }

    /// 从单独的配置数据库连接创建
    pub fn from_db_path(db_path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("打开配置数据库失败: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_config (key TEXT PRIMARY KEY, value TEXT);",
        )
        .map_err(|e| format!("创建配置表失败: {}", e))?;
        Ok(ConfigManager {
            conn: Mutex::new(conn),
        })
    }

    fn get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT value FROM app_config WHERE key=?1",
            params![key],
            |row| row.get(0),
        )
        .ok()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        conn.execute(
            "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| format!("写入配置失败: {}", e))?;
        Ok(())
    }

    /// 读取完整配置
    pub fn load(&self) -> AppConfig {
        AppConfig {
            baidu_cookie: self.get("baidu_cookie").unwrap_or_default(),
            baidu_remote_path: self
                .get("baidu_remote_path")
                .unwrap_or_else(|| "/apps/VAULT/vault.json".to_string()),
            baidu_sync_enabled: self
                .get("baidu_sync_enabled")
                .map(|v| v == "true")
                .unwrap_or(false),
            baidu_sync_interval: self
                .get("baidu_sync_interval")
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            baidu_cookie_expires_at: self
                .get("baidu_cookie_expires_at")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            baidu_last_sync: self.get("baidu_last_sync").unwrap_or_default(),
            quark_cookie: self.get("quark_cookie").unwrap_or_default(),
            quark_remote_path: self
                .get("quark_remote_path")
                .unwrap_or_else(|| "/VAULT/vault.json".to_string()),
            quark_sync_enabled: self
                .get("quark_sync_enabled")
                .map(|v| v == "true")
                .unwrap_or(false),
            quark_sync_interval: self
                .get("quark_sync_interval")
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            quark_cookie_expires_at: self
                .get("quark_cookie_expires_at")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            quark_last_sync: self.get("quark_last_sync").unwrap_or_default(),
            quark_last_remote_mtime: self
                .get("quark_last_remote_mtime")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            auto_lock_minutes: self
                .get("auto_lock_minutes")
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            clipboard_clear_seconds: self
                .get("clipboard_clear_seconds")
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            master_password: self.get("master_password").unwrap_or_default(),
        }
    }

    /// 保存完整配置
    pub fn save(&self, config: &AppConfig) -> Result<(), String> {
        self.set("baidu_cookie", &config.baidu_cookie)?;
        self.set("baidu_remote_path", &config.baidu_remote_path)?;
        self.set(
            "baidu_sync_enabled",
            if config.baidu_sync_enabled { "true" } else { "false" },
        )?;
        self.set(
            "baidu_sync_interval",
            &config.baidu_sync_interval.to_string(),
        )?;
        self.set(
            "baidu_cookie_expires_at",
            &config.baidu_cookie_expires_at.to_string(),
        )?;
        self.set("baidu_last_sync", &config.baidu_last_sync)?;
        self.set("quark_cookie", &config.quark_cookie)?;
        self.set("quark_remote_path", &config.quark_remote_path)?;
        self.set(
            "quark_sync_enabled",
            if config.quark_sync_enabled { "true" } else { "false" },
        )?;
        self.set(
            "quark_sync_interval",
            &config.quark_sync_interval.to_string(),
        )?;
        self.set(
            "quark_cookie_expires_at",
            &config.quark_cookie_expires_at.to_string(),
        )?;
        self.set("quark_last_sync", &config.quark_last_sync)?;
        self.set(
            "quark_last_remote_mtime",
            &config.quark_last_remote_mtime.to_string(),
        )?;
        self.set("auto_lock_minutes", &config.auto_lock_minutes.to_string())?;
        self.set(
            "clipboard_clear_seconds",
            &config.clipboard_clear_seconds.to_string(),
        )?;
        self.set("master_password", &config.master_password)?;
        Ok(())
    }
}
