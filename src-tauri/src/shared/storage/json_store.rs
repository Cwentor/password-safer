use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::shared::crypto::Crypto;
use crate::shared::models::{calculate_strength, AppConfig, PasswordDto, PasswordInput};

/// JSON 文件存储的完整数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreData {
    pub passwords: Vec<PasswordRecord>,
    pub config: AppConfig,
    /// 数据版本号，用于未来迁移
    pub version: u32,
    /// 下一条密码记录的 ID（自增）
    pub next_id: i64,
}

/// 单条密码记录（密码字段已加密）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordRecord {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub url: String,
    pub username: String,
    /// 已用 crypto.encrypt 加密的密码字段（base64）
    pub password_encrypted: String,
    pub tags: Vec<String>,
    pub notes: String,
    pub favorite: bool,
    pub strength: i32,
    pub created: String,
    pub last_used: String,
}

impl Default for StoreData {
    fn default() -> Self {
        StoreData {
            passwords: Vec::new(),
            config: AppConfig::default(),
            version: 1,
            next_id: 1,
        }
    }
}

/// 加密 JSON 文件存储
pub struct JsonStore {
    db_path: PathBuf,
    crypto: Crypto,
}

impl JsonStore {
    /// 打开或创建 JSON 存储
    /// 若文件不存在则创建空 StoreData 并保存
    /// 若文件存在则验证可解密（不验证则启动失败）
    pub fn open(db_path: PathBuf, crypto: Crypto) -> Result<Self, String> {
        let store = JsonStore {
            db_path,
            crypto,
        };
        // 如果文件不存在，创建空 StoreData 并保存
        if !store.db_path.exists() {
            let empty = StoreData::default();
            store.save(&empty)?;
            eprintln!("[json_store] 创建空 vault.json: {:?}", store.db_path);
        } else {
            // 验证可解密
            match store.load() {
                Ok(_) => eprintln!("[json_store] vault.json 加载验证成功"),
                Err(e) => return Err(format!("vault.json 解密失败: {}", e)),
            }
        }
        Ok(store)
    }

    /// 读取并解密 JSON 文件
    pub fn load(&self) -> Result<StoreData, String> {
        let bytes = std::fs::read(&self.db_path)
            .map_err(|e| format!("读取 vault.json 失败: {}", e))?;
        // 文件内容是 base64 字符串（crypto.encrypt 的输出）
        let encoded = String::from_utf8(bytes)
            .map_err(|e| format!("vault.json 不是有效的 UTF-8: {}", e))?;
        let json_str = self.crypto.decrypt(&encoded)?;
        let data: StoreData = serde_json::from_str(&json_str)
            .map_err(|e| format!("反序列化 StoreData 失败: {}", e))?;
        Ok(data)
    }

    /// 加密并写入 JSON 文件
    pub fn save(&self, data: &StoreData) -> Result<(), String> {
        let json_str = serde_json::to_string(data)
            .map_err(|e| format!("序列化 StoreData 失败: {}", e))?;
        let encoded = self.crypto.encrypt(&json_str)?;
        self.write_atomic(encoded.as_bytes())
    }

    /// 原子写入：先写临时文件再改名替换，并发读不会读到半截文件。
    /// 改名前把当前文件备份为 .bak，供失败排查与回滚。
    fn write_atomic(&self, bytes: &[u8]) -> Result<(), String> {
        let tmp_path = self.db_path.with_extension("json.tmp");
        let bak_path = self.db_path.with_extension("json.bak");
        std::fs::write(&tmp_path, bytes).map_err(|e| format!("写入临时文件失败: {}", e))?;
        if self.db_path.exists() {
            std::fs::copy(&self.db_path, &bak_path).map_err(|e| format!("备份密码簿失败: {}", e))?;
        }
        std::fs::rename(&tmp_path, &self.db_path).map_err(|e| format!("替换密码簿失败: {}", e))?;
        Ok(())
    }

    /// 用外部内容整体替换密码簿（云同步下载、导入共用此单一入口）。
    /// 先验证内容可解密、可解析，验证不过则不动现有文件。
    pub fn replace_raw(&self, bytes: &[u8]) -> Result<(), String> {
        let encoded = String::from_utf8(bytes.to_vec())
            .map_err(|_| "新内容不是有效的 UTF-8 文本".to_string())?;
        let json_str = self
            .crypto
            .decrypt(&encoded)
            .map_err(|e| format!("新内容解密失败（文件可能已损坏或不是本应用导出）: {}", e))?;
        let _: StoreData = serde_json::from_str(&json_str)
            .map_err(|e| format!("新内容无法解析为密码簿结构: {}", e))?;
        self.write_atomic(bytes)
    }

    /// 将 PasswordRecord 转换为 PasswordDto（解密密码字段）
    fn record_to_dto(&self, r: &PasswordRecord) -> PasswordDto {
        let password = self.crypto.decrypt(&r.password_encrypted).unwrap_or_default();
        PasswordDto {
            id: r.id,
            name: r.name.clone(),
            icon: r.icon.clone(),
            url: r.url.clone(),
            username: r.username.clone(),
            password,
            tags: r.tags.clone(),
            notes: r.notes.clone(),
            favorite: r.favorite,
            strength: r.strength,
            created: r.created.clone(),
            last_used: r.last_used.clone(),
        }
    }

    /// 获取所有密码（已解密），按 name 不区分大小写排序
    pub fn get_all(&self) -> Result<Vec<PasswordDto>, String> {
        let data = self.load()?;
        let mut dtos: Vec<PasswordDto> = data
            .passwords
            .iter()
            .map(|r| self.record_to_dto(r))
            .collect();
        dtos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(dtos)
    }

    /// 新增密码
    pub fn insert(&self, input: &PasswordInput) -> Result<PasswordDto, String> {
        let mut data = self.load()?;
        let encrypted = self.crypto.encrypt(&input.password)?;
        let strength = calculate_strength(&input.password);
        let now = chrono::Local::now().format("%Y-%m-%d").to_string();
        let id = data.next_id;
        data.next_id += 1;

        let record = PasswordRecord {
            id,
            name: input.name.clone(),
            icon: input.icon.clone().unwrap_or_else(|| "🔑".to_string()),
            url: input.url.clone().unwrap_or_default(),
            username: input.username.clone().unwrap_or_default(),
            password_encrypted: encrypted,
            tags: input.tags.clone().unwrap_or_default(),
            notes: input.notes.clone().unwrap_or_default(),
            favorite: input.favorite.unwrap_or(false),
            strength,
            created: now.clone(),
            last_used: now,
        };

        let dto = self.record_to_dto(&record);
        data.passwords.push(record);
        self.save(&data)?;
        Ok(dto)
    }

    /// 更新密码（保留原 created，不更新 last_used）
    pub fn update(&self, id: i64, input: &PasswordInput) -> Result<PasswordDto, String> {
        let mut data = self.load()?;
        let record = data
            .passwords
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("找不到 ID 为 {} 的密码记录", id))?;

        let encrypted = self.crypto.encrypt(&input.password)?;
        let strength = calculate_strength(&input.password);

        record.name = input.name.clone();
        record.icon = input.icon.clone().unwrap_or_else(|| "🔑".to_string());
        record.url = input.url.clone().unwrap_or_default();
        record.username = input.username.clone().unwrap_or_default();
        record.password_encrypted = encrypted;
        record.tags = input.tags.clone().unwrap_or_default();
        record.notes = input.notes.clone().unwrap_or_default();
        record.favorite = input.favorite.unwrap_or(false);
        record.strength = strength;
        // 保留原 created，不更新 last_used（保持原值）

        let dto = self.record_to_dto(record);
        self.save(&data)?;
        Ok(dto)
    }

    /// 删除密码
    pub fn delete(&self, id: i64) -> Result<(), String> {
        let mut data = self.load()?;
        data.passwords.retain(|p| p.id != id);
        self.save(&data)
    }

    /// 切换收藏
    pub fn toggle_favorite(&self, id: i64) -> Result<(), String> {
        let mut data = self.load()?;
        let target = data.passwords.iter_mut().find(|p| p.id == id);
        if let Some(record) = target {
            record.favorite = !record.favorite;
        }
        self.save(&data)?;
        Ok(())
    }

    /// 更新最后使用时间为当前日期
    pub fn update_last_used(&self, id: i64) -> Result<(), String> {
        let mut data = self.load()?;
        let now = chrono::Local::now().format("%Y-%m-%d").to_string();
        let target = data.passwords.iter_mut().find(|p| p.id == id);
        if let Some(record) = target {
            record.last_used = now;
        }
        self.save(&data)?;
        Ok(())
    }

    /// 读取配置
    pub fn load_config(&self) -> Result<AppConfig, String> {
        let data = self.load()?;
        Ok(data.config.clone())
    }

    /// 保存配置
    pub fn save_config(&self, cfg: &AppConfig) -> Result<(), String> {
        let mut data = self.load()?;
        data.config = cfg.clone();
        self.save(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 唯一的临时目录（按纳秒时间戳 + 进程号区分，测试结束不清理也无碍）
    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ps-test-{}-{}-{}",
            std::process::id(),
            tag,
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open_store(tag: &str) -> (PathBuf, JsonStore) {
        let dir = temp_dir(tag);
        let key_path = dir.join("master.key");
        let crypto = Crypto::from_key_file(&key_path).unwrap();
        let db_path = dir.join("vault.json");
        let store = JsonStore::open(db_path.clone(), crypto).unwrap();
        (dir, store)
    }

    fn sample_input(name: &str, password: &str) -> PasswordInput {
        PasswordInput {
            name: name.to_string(),
            icon: None,
            url: None,
            username: None,
            password: password.to_string(),
            tags: Some(vec!["work".to_string()]),
            notes: None,
            favorite: Some(false),
        }
    }

    #[test]
    fn crypto_roundtrip() {
        let dir = temp_dir("crypto");
        let crypto = Crypto::from_key_file(&dir.join("master.key")).unwrap();
        let text = "p@ssw0rd 密码 \\ backslash\nnewline";
        let enc = crypto.encrypt(text).unwrap();
        assert_eq!(crypto.decrypt(&enc).unwrap(), text);
    }

    #[test]
    fn crypto_wrong_key_fails() {
        let dir = temp_dir("crypto-wrong");
        let c1 = Crypto::from_key_file(&dir.join("k1.key")).unwrap();
        let c2 = Crypto::from_key_file(&dir.join("k2.key")).unwrap();
        let enc = c1.encrypt("secret").unwrap();
        assert!(c2.decrypt(&enc).is_err());
    }

    #[test]
    fn crud_roundtrip() {
        let (_dir, store) = open_store("crud");
        let dto = store.insert(&sample_input("站点A", "abc123456")).unwrap();
        assert_eq!(dto.id, 1);
        assert!(!dto.password.is_empty());

        let all = store.get_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].password, "abc123456");
        assert!(all[0].strength >= 1);

        store.toggle_favorite(1).unwrap();
        let all = store.get_all().unwrap();
        assert!(all[0].favorite);

        store.delete(1).unwrap();
        assert!(store.get_all().unwrap().is_empty());
    }

    #[test]
    fn replace_raw_rejects_garbage_and_keeps_file() {
        let (_dir, store) = open_store("replace-bad");
        store.insert(&sample_input("站点A", "abc123456")).unwrap();
        let before = std::fs::read(&store.db_path).unwrap();

        assert!(store.replace_raw(b"not valid at all").is_err());

        // 原文件未被破坏
        let after = std::fs::read(&store.db_path).unwrap();
        assert_eq!(before, after);
        assert_eq!(store.get_all().unwrap().len(), 1);
    }

    #[test]
    fn replace_raw_accepts_valid_export() {
        let dir = temp_dir("replace-ok");
        let crypto = Crypto::from_key_file(&dir.join("master.key")).unwrap();

        // 造一份"另一台设备"的密码簿
        let src_path = dir.join("other.json");
        let other = JsonStore::open(src_path.clone(), crypto.clone_key()).unwrap();
        other.insert(&sample_input("别的设备", "xyz987654321")).unwrap();
        let bytes = std::fs::read(&src_path).unwrap();
        drop(other);

        // 目标库已有一条数据，导入后应被整体替换
        let db_path = dir.join("vault.json");
        let store = JsonStore::open(db_path, crypto).unwrap();
        store.insert(&sample_input("本地旧数据", "abc123456")).unwrap();

        store.replace_raw(&bytes).unwrap();
        let all = store.get_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "别的设备");
        assert_eq!(all[0].password, "xyz987654321");
    }

    #[test]
    fn config_roundtrip() {
        let (_dir, store) = open_store("config");
        let mut cfg = store.load_config().unwrap();
        cfg.quark_sync_enabled = true;
        cfg.quark_sync_interval = 600;
        store.save_config(&cfg).unwrap();

        let cfg2 = store.load_config().unwrap();
        assert!(cfg2.quark_sync_enabled);
        assert_eq!(cfg2.quark_sync_interval, 600);
    }
}
