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
        std::fs::write(&self.db_path, encoded.as_bytes())
            .map_err(|e| format!("写入 vault.json 失败: {}", e))?;
        Ok(())
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

    /// 搜索密码（name/username/url/notes/tags 任一字段不区分大小写包含 query）
    pub fn search(&self, query: &str) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        let q = query.to_lowercase();
        let filtered: Vec<PasswordDto> = all
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&q)
                    || p.username.to_lowercase().contains(&q)
                    || p.url.to_lowercase().contains(&q)
                    || p.notes.to_lowercase().contains(&q)
                    || p.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .collect();
        Ok(filtered)
    }

    /// 按标签筛选
    pub fn get_by_tag(&self, tag: &str) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        Ok(all
            .into_iter()
            .filter(|p| p.tags.iter().any(|t| t == tag))
            .collect())
    }

    /// 获取收藏
    pub fn get_favorites(&self) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        Ok(all.into_iter().filter(|p| p.favorite).collect())
    }

    /// 获取弱密码（strength <= 2）
    pub fn get_weak(&self) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        Ok(all.into_iter().filter(|p| p.strength <= 2).collect())
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
