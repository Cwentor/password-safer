use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::shared::crypto::{self, Crypto, KdfParams, VaultEnvelope};
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

/// 解码并验证密码簿字节（正常读取、.bak 回退、replace_raw、冲突详情共用）。
/// 双格式：v2 自描述信封（'{' 开头）或 v1 base64（master.key 时代）。
pub fn decode_store(bytes: &[u8], crypto: &Crypto) -> Result<StoreData, String> {
    if VaultEnvelope::looks_like_envelope(bytes) {
        let env: VaultEnvelope = serde_json::from_slice(bytes)
            .map_err(|e| format!("v2 信封解析失败: {}", e))?;
        let json_str = crypto.decrypt_envelope(&env)?;
        return serde_json::from_str(&json_str)
            .map_err(|e| format!("反序列化 StoreData 失败: {}", e));
    }
    let encoded = String::from_utf8(bytes.to_vec())
        .map_err(|_| "vault.json 不是有效的 UTF-8".to_string())?;
    let json_str = crypto.decrypt(&encoded)?;
    serde_json::from_str(&json_str).map_err(|e| format!("反序列化 StoreData 失败: {}", e))
}

/// 探测密码簿格式（启动流程决定走 master.key 还是库密码路径）
pub fn vault_format(db_path: &Path) -> &'static str {
    match std::fs::read(db_path) {
        Ok(bytes) if VaultEnvelope::looks_like_envelope(&bytes) => "v2",
        Ok(_) => "v1",
        Err(_) => "none",
    }
}

/// 记录 → DTO（密码字段按给定加密器解密）；本地与远端记录映射共用
pub fn record_to_dto(crypto: &Crypto, r: &PasswordRecord) -> PasswordDto {
    let password = crypto.decrypt(&r.password_encrypted).unwrap_or_default();
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

/// 加密 JSON 文件存储
pub struct JsonStore {
    db_path: PathBuf,
    crypto: Crypto,
    /// v2 信封模式（库密码派生钥）；false = v1 master.key 模式
    envelope_mode: bool,
    /// v2 模式的 KDF 参数（随库写进信封头）
    kdf_params: Option<KdfParams>,
    /// 用户写路径回调（save 成功后触发，用于刷新同步记账的 local_dirty_at；
    /// 同步写路径 replace_raw 不经过 save，不触发——这是区分"用户改过"与"同步落下"的关键）
    on_user_save: Option<Box<dyn Fn() + Send + Sync>>,
}

impl JsonStore {
    /// v1 模式打开（master.key 时代）：文件不存在则创建空库；
    /// 解密失败尝试 .bak 回退；遇到 v2 文件则报错（应走库密码路径）
    pub fn open(db_path: PathBuf, crypto: Crypto) -> Result<Self, String> {
        if db_path.exists() && vault_format(&db_path) == "v2" {
            return Err("密码簿已是库密码加密格式，请输入库密码解锁".to_string());
        }
        let store = JsonStore {
            db_path,
            crypto,
            envelope_mode: false,
            kdf_params: None,
            on_user_save: None,
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
                Err(e) => store.recover_from_bak(&e)?,
            }
        }
        Ok(store)
    }

    /// v2 模式打开：解析信封头 → 派生密钥 → 验证解密。
    /// 返回 (store, 派生钥)（派生钥供本机凭据库缓存）
    pub fn open_with_password(db_path: PathBuf, password: &str) -> Result<(Self, [u8; 32]), String> {
        let bytes = std::fs::read(&db_path)
            .map_err(|e| format!("读取 vault.json 失败: {}", e))?;
        let env: VaultEnvelope = serde_json::from_slice(&bytes)
            .map_err(|_| "密码簿不是 v2 库密码加密格式".to_string())?;
        let key = crypto::derive_key(password, &env.kdf)?;
        let crypto = Crypto::from_key_bytes(key);
        let store = JsonStore {
            db_path,
            crypto,
            envelope_mode: true,
            kdf_params: Some(env.kdf.clone()),
            on_user_save: None,
        };
        store.load().map_err(|e| format!("库密码解锁失败: {}", e))?;
        eprintln!("[json_store] vault.json 已用库密码解锁");
        Ok((store, key))
    }

    /// v2 模式建库（首启）：生成 KDF 参数 → 派生 → 写入空库。
    /// 返回 (store, 派生钥)
    pub fn create_with_password(db_path: PathBuf, password: &str) -> Result<(Self, [u8; 32]), String> {
        let params = KdfParams::generate();
        let key = crypto::derive_key(password, &params)?;
        let crypto = Crypto::from_key_bytes(key);
        let store = JsonStore {
            db_path,
            crypto,
            envelope_mode: true,
            kdf_params: Some(params),
            on_user_save: None,
        };
        store.save(&StoreData::default())?;
        eprintln!("[json_store] 创建库密码加密的空 vault.json: {:?}", store.db_path);
        Ok((store, key))
    }

    /// v2 模式用已有派生钥打开（本机缓存路径，无需密码）
    pub fn open_with_key(db_path: PathBuf, key: [u8; 32]) -> Result<Self, String> {
        let bytes = std::fs::read(&db_path)
            .map_err(|e| format!("读取 vault.json 失败: {}", e))?;
        let env: VaultEnvelope = serde_json::from_slice(&bytes)
            .map_err(|_| "密码簿不是 v2 库密码加密格式".to_string())?;
        let store = JsonStore {
            db_path,
            crypto: Crypto::from_key_bytes(key),
            envelope_mode: true,
            kdf_params: Some(env.kdf.clone()),
            on_user_save: None,
        };
        store.load().map_err(|e| format!("缓存钥匙解锁失败: {}", e))?;
        Ok(store)
    }

    /// 换库密码 / v1→v2 升级迁移：旧钥解密全库（含密码字段）→ 新参数新钥重加密 → 原子写。
    /// 返回新派生钥（缓存更新用）。任何一步失败都不动现有文件。
    pub fn re_encrypt(&mut self, new_password: &str) -> Result<[u8; 32], String> {
        let mut data = self.load()?;
        let params = KdfParams::generate();
        let new_key = crypto::derive_key(new_password, &params)?;
        let new_crypto = Crypto::from_key_bytes(new_key);

        // 密码字段逐条换钥（旧钥解不出来即中止，不动文件）
        for record in &mut data.passwords {
            let plain = self
                .crypto
                .decrypt(&record.password_encrypted)
                .map_err(|e| format!("换钥中止：字段解密失败（{}）", e))?;
            record.password_encrypted = new_crypto.encrypt(&plain)?;
        }

        let old_crypto = std::mem::replace(&mut self.crypto, new_crypto);
        self.kdf_params = Some(params);
        let was_envelope = self.envelope_mode;
        self.envelope_mode = true;
        if let Err(e) = self.save(&data) {
            // 回滚内存态（文件未被写坏：save 原子失败即保持旧内容）
            self.crypto = old_crypto;
            self.envelope_mode = was_envelope;
            return Err(e);
        }
        Ok(new_key)
    }

    /// 校验库密码是否正确（派生钥与当前钥比对）
    pub fn verify_password(&self, password: &str) -> bool {
        match self.kdf_params.as_ref() {
            Some(p) => crypto::derive_key(password, p)
                .map(|k| k == self.crypto.key_bytes())
                .unwrap_or(false),
            None => false,
        }
    }

    /// 主文件解密失败时尝试从 .bak 恢复：先验证 .bak 可解密可解析，再原子覆盖主文件
    fn recover_from_bak(&self, main_err: &str) -> Result<(), String> {
        let bak_path = self.db_path.with_extension("json.bak");
        let bytes = std::fs::read(&bak_path)
            .map_err(|_| format!("vault.json 解密失败（{}）且无可用 .bak", main_err))?;
        decode_store(&bytes, &self.crypto)
            .map_err(|e| format!("vault.json 解密失败（{}）且 .bak 不可用（{}）", main_err, e))?;
        std::fs::write(&self.db_path, bytes)
            .map_err(|e| format!(".bak 恢复写入失败: {}", e))?;
        // 覆盖后复核一次，确保主文件恢复可用
        self.load()
            .map_err(|e| format!(".bak 恢复后复核失败: {}", e))?;
        eprintln!("[json_store] vault.json 已从 .bak 恢复");
        Ok(())
    }

    /// 注册用户写路径回调（需在 open 之后、并发使用之前调用）
    pub fn set_on_user_save(&mut self, f: Box<dyn Fn() + Send + Sync>) {
        self.on_user_save = Some(f);
    }

    /// 读取并解密 JSON 文件
    pub fn load(&self) -> Result<StoreData, String> {
        let bytes = std::fs::read(&self.db_path)
            .map_err(|e| format!("读取 vault.json 失败: {}", e))?;
        decode_store(&bytes, &self.crypto)
    }

    /// 加密并写入 JSON 文件（v2 信封 / v1 base64 按当前模式）
    pub fn save(&self, data: &StoreData) -> Result<(), String> {
        let json_str = serde_json::to_string(data)
            .map_err(|e| format!("序列化 StoreData 失败: {}", e))?;
        let bytes: Vec<u8> = if self.envelope_mode {
            let params = self
                .kdf_params
                .clone()
                .ok_or_else(|| "v2 模式缺少 KDF 参数".to_string())?;
            let env = self.crypto.encrypt_envelope(&json_str, &params)?;
            serde_json::to_vec_pretty(&env).map_err(|e| format!("序列化信封失败: {}", e))?
        } else {
            let encoded = self.crypto.encrypt(&json_str)?;
            encoded.into_bytes()
        };
        self.write_atomic(&bytes)?;
        if let Some(cb) = &self.on_user_save {
            cb();
        }
        Ok(())
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
    /// 跨版本内容（如 v1 云端字节落入 v2 本地）按当前模式重新编码落盘；
    /// 同格式内容原样写入（与云端字节一致，保住内容寻址去重）。
    pub fn replace_raw(&self, bytes: &[u8]) -> Result<(), String> {
        let data = decode_store(bytes, &self.crypto)
            .map_err(|e| format!("新内容解密失败（文件可能已损坏或不是本应用导出）: {}", e))?;
        let same_format = self.envelope_mode == VaultEnvelope::looks_like_envelope(bytes);
        if same_format {
            self.write_atomic(bytes)
        } else {
            self.save(&data)
        }
    }

    /// 当前加密器（冲突详情解密远端侧用）
    pub fn crypto(&self) -> &Crypto {
        &self.crypto
    }

    /// 将 PasswordRecord 转换为 PasswordDto（解密密码字段）
    fn record_to_dto(&self, r: &PasswordRecord) -> PasswordDto {
        record_to_dto(&self.crypto, r)
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
    fn open_recovers_from_bak_when_main_corrupted() {
        let dir = temp_dir("bak-recover");
        let crypto = Crypto::from_key_file(&dir.join("master.key")).unwrap();
        let db_path = dir.join("vault.json");

        let store = JsonStore::open(db_path.clone(), crypto.clone_key()).unwrap();
        store.insert(&sample_input("第一版", "abc123456")).unwrap();
        store.insert(&sample_input("第二版", "xyz987654321")).unwrap();
        // 此时 .bak 持有"第一版"内容，主文件持有"第二版"
        drop(store);

        // 主文件损坏 → 打开时从 .bak 回退
        std::fs::write(&db_path, b"corrupted garbage").unwrap();
        let store2 = JsonStore::open(db_path, crypto).unwrap();
        let all = store2.get_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "第一版");
    }

    #[test]
    fn open_fails_when_corrupted_without_bak() {
        let dir = temp_dir("bak-missing");
        let crypto = Crypto::from_key_file(&dir.join("master.key")).unwrap();
        let db_path = dir.join("vault.json");
        std::fs::write(&db_path, b"corrupted garbage").unwrap();
        assert!(JsonStore::open(db_path, crypto).is_err());
    }

    #[test]
    fn user_save_callback_fires_only_on_save() {
        let dir = temp_dir("callback");
        let crypto = Crypto::from_key_file(&dir.join("master.key")).unwrap();
        let db_path = dir.join("vault.json");
        let mut store = JsonStore::open(db_path, crypto).unwrap();

        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = counter.clone();
        store.set_on_user_save(Box::new(move || {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));

        // 用户写路径：回调触发
        store.insert(&sample_input("站点A", "abc123456")).unwrap();
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);

        // 同步写路径：replace_raw 不触发回调
        let bytes = std::fs::read(&store.db_path).unwrap();
        store.replace_raw(&bytes).unwrap();
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn derive_key_deterministic() {
        let p1 = KdfParams::generate();
        let k1a = crypto::derive_key("密码12345", &p1).unwrap();
        let k1b = crypto::derive_key("密码12345", &p1).unwrap();
        assert_eq!(k1a, k1b);
        // 同密码不同盐 → 不同钥
        let p2 = KdfParams::generate();
        let k2 = crypto::derive_key("密码12345", &p2).unwrap();
        assert_ne!(k1a, k2);
        // 不同密码不同钥
        let k3 = crypto::derive_key("另一些字", &p1).unwrap();
        assert_ne!(k1a, k3);
    }

    #[test]
    fn envelope_roundtrip_and_wrong_key_rejected() {
        let params = KdfParams::generate();
        let c1 = Crypto::from_key_bytes(crypto::derive_key("正确的库密码", &params).unwrap());
        let env = c1.encrypt_envelope("{\"x\":1}", &params).unwrap();
        let bytes = serde_json::to_vec(&env).unwrap();
        assert!(VaultEnvelope::looks_like_envelope(&bytes));
        assert_eq!(c1.decrypt_envelope(&env).unwrap(), "{\"x\":1}");
        // 错误钥匙解不开
        let params2 = KdfParams::generate();
        let c2 = Crypto::from_key_bytes(crypto::derive_key("别的密码", &params2).unwrap());
        assert!(c2.decrypt_envelope(&env).is_err());
    }

    #[test]
    fn v2_create_insert_reopen_roundtrip() {
        let dir = temp_dir("v2-roundtrip");
        let db_path = dir.join("vault.json");

        let (store, _key) = JsonStore::create_with_password(db_path.clone(), "库密码12345").unwrap();
        store.insert(&sample_input("站点A", "abc123456")).unwrap();
        drop(store);

        let (store2, _) = JsonStore::open_with_password(db_path.clone(), "库密码12345").unwrap();
        let all = store2.get_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].password, "abc123456");

        // 错误密码被拒
        assert!(JsonStore::open_with_password(db_path, "错误密码000").is_err());
    }

    #[test]
    fn v2_re_encrypt_rotates_all_keys() {
        let dir = temp_dir("v2-rekey");
        let db_path = dir.join("vault.json");

        let (store, _old_key) =
            JsonStore::create_with_password(db_path.clone(), "旧密码12345").unwrap();
        store.insert(&sample_input("站点A", "abc123456")).unwrap();
        drop(store);

        let (mut store, new_key) =
            JsonStore::open_with_password(db_path.clone(), "旧密码12345").unwrap();
        // 同密码 + 同盐 → 派生钥必然相同（这正是跨设备凭密码可解的根据）
        let new_key2 = store.re_encrypt("新密码67890").unwrap();
        assert_ne!(new_key, new_key2);
        // 新钥下数据完好（含字段级换钥）
        assert_eq!(store.get_all().unwrap()[0].password, "abc123456");
        drop(store);

        // 旧密码打不开，新密码可以
        assert!(JsonStore::open_with_password(db_path.clone(), "旧密码12345").is_err());
        let (store3, _) = JsonStore::open_with_password(db_path, "新密码67890").unwrap();
        assert_eq!(store3.get_all().unwrap()[0].password, "abc123456");
    }

    #[test]
    fn v2_store_rejects_foreign_format_bytes() {
        // v1 库不能用库密码路径打开
        let (_dir, v1_store) = open_store("v1-for-v2");
        v1_store.insert(&sample_input("旧设备数据", "abc123456")).unwrap();
        let v1_bytes = std::fs::read(&v1_store.db_path).unwrap();
        assert!(JsonStore::open_with_password(v1_store.db_path.clone(), "任何密码000").is_err());

        // v1 字节由 master.key 加密，v2 派生钥必然解不开：
        // replace_raw 干净报错且不动本地文件（v1→v2 只能经本机迁移换钥，不能靠同步吸收）
        let dir = temp_dir("v2-cross");
        let db_path = dir.join("vault.json");
        let (store, _) = JsonStore::create_with_password(db_path, "库密码12345").unwrap();
        store.insert(&sample_input("本机数据", "xyz987654")).unwrap();
        let before = std::fs::read(&store.db_path).unwrap();

        assert!(store.replace_raw(&v1_bytes).is_err());
        assert_eq!(before, std::fs::read(&store.db_path).unwrap());
        assert_eq!(store.get_all().unwrap().len(), 1);
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
