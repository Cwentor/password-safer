use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose, Engine};
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};

/// KDF 默认参数：显存墙 + 高单次成本，抬高云端密文被离线爆破的代价
/// （解锁一次约 0.5～1s，与拍板方案一致）
pub const KDF_M_COST: u32 = 256 * 1024; // KiB，即 256 MiB
pub const KDF_T_COST: u32 = 4;
pub const KDF_PARALLELISM: u32 = 4;
pub const KDF_SALT_LEN: usize = 16;

/// Argon2id 派生参数（随库保存在 v2 信封头里，新设备拿到即可派生）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KdfParams {
    pub algo: String,
    pub salt_b64: String,
    pub m_cost: u32,
    pub t_cost: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// 生成一组新参数（随机盐）
    pub fn generate() -> Self {
        let mut salt = [0u8; KDF_SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        KdfParams {
            algo: "argon2id".to_string(),
            salt_b64: general_purpose::STANDARD.encode(salt),
            m_cost: KDF_M_COST,
            t_cost: KDF_T_COST,
            parallelism: KDF_PARALLELISM,
        }
    }
}

/// 从库密码派生 32 字节密钥（Argon2id）
pub fn derive_key(password: &str, params: &KdfParams) -> Result<[u8; 32], String> {
    use argon2::{Algorithm, Argon2, Params, Version};
    if params.algo != "argon2id" {
        return Err(format!("不支持的 KDF 算法: {}", params.algo));
    }
    let salt = general_purpose::STANDARD
        .decode(&params.salt_b64)
        .map_err(|_| "KDF 盐不是有效的 base64".to_string())?;
    if salt.len() < 8 {
        return Err("KDF 盐长度不足".to_string());
    }
    let a2 = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(params.m_cost, params.t_cost, params.parallelism, Some(32))
            .map_err(|e| format!("KDF 参数非法: {}", e))?,
    );
    let mut out = [0u8; 32];
    a2.hash_password_into(password.as_bytes(), &salt, &mut out)
        .map_err(|e| format!("KDF 派生失败: {}", e))?;
    Ok(out)
}

/// vault v2 自描述信封：盐与 KDF 参数随库走，任何设备凭密码即可解密
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VaultEnvelope {
    pub v: u32,
    pub kdf: KdfParams,
    pub nonce_b64: String,
    /// AES-256-GCM 密文（明文为密码簿 JSON 字符串）
    pub ct_b64: String,
}

impl VaultEnvelope {
    pub const VERSION: u32 = 2;

    /// 判断一段字节是否为 v2 信封（v1 是纯 base64，'{' 不在 base64 字母表内）
    pub fn looks_like_envelope(bytes: &[u8]) -> bool {
        bytes
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            .map(|b| *b == b'{')
            .unwrap_or(false)
    }
}

/// 加密器：使用 AES-256-GCM 加密密码字段
pub struct Crypto {
    key: [u8; 32],
}

impl Crypto {
    /// 从密钥文件加载或创建新密钥
    pub fn from_key_file(key_path: &std::path::Path) -> Result<Self, String> {
        if key_path.exists() {
            let key_bytes = std::fs::read(key_path)
                .map_err(|e| format!("读取密钥文件失败: {}", e))?;
            if key_bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&key_bytes);
                return Ok(Crypto { key });
            }
        }

        // 生成新密钥
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        std::fs::write(key_path, key)
            .map_err(|e| format!("写入密钥文件失败: {}", e))?;
        Ok(Crypto { key })
    }

    /// 从已有 32 字节密钥构造（v2 库密码派生钥 / keyring 缓存恢复用）
    pub fn from_key_bytes(key: [u8; 32]) -> Self {
        Crypto { key }
    }

    /// 当前密钥字节（keyring 缓存用）
    pub fn key_bytes(&self) -> [u8; 32] {
        self.key
    }

    /// v2 信封加密：把密码簿 JSON 明文加密为自描述信封
    pub fn encrypt_envelope(&self, plaintext: &str, params: &KdfParams) -> Result<VaultEnvelope, String> {
        let cipher = Aes256Gcm::new(&self.key.into());
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| format!("加密失败: {}", e))?;
        Ok(VaultEnvelope {
            v: VaultEnvelope::VERSION,
            kdf: params.clone(),
            nonce_b64: general_purpose::STANDARD.encode(nonce_bytes),
            ct_b64: general_purpose::STANDARD.encode(ct),
        })
    }

    /// v2 信封解密：从信封还原密码簿 JSON 明文
    pub fn decrypt_envelope(&self, env: &VaultEnvelope) -> Result<String, String> {
        if env.v != VaultEnvelope::VERSION {
            return Err(format!("不支持的信封版本: {}", env.v));
        }
        let nonce_bytes = general_purpose::STANDARD
            .decode(&env.nonce_b64)
            .map_err(|_| "信封 nonce 不是有效的 base64".to_string())?;
        if nonce_bytes.len() != 12 {
            return Err("信封 nonce 长度非法".to_string());
        }
        let ct = general_purpose::STANDARD
            .decode(&env.ct_b64)
            .map_err(|_| "信封密文不是有效的 base64".to_string())?;
        let cipher = Aes256Gcm::new(&self.key.into());
        let pt = cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ct.as_ref())
            .map_err(|_| "解密失败（密码错误或数据损坏）".to_string())?;
        String::from_utf8(pt).map_err(|_| "解密结果不是有效的 UTF-8".to_string())
    }

    /// 加密明文，返回 base64 编码的 "nonce + ciphertext"
    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let cipher = Aes256Gcm::new(&self.key.into());
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| format!("加密失败: {}", e))?;

        // nonce(12 bytes) + ciphertext 拼接后 base64
        let mut combined = Vec::with_capacity(12 + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        Ok(general_purpose::STANDARD.encode(&combined))
    }

    /// 解密 base64 编码的 "nonce + ciphertext"
    pub fn decrypt(&self, encoded: &str) -> Result<String, String> {
        let combined = general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("Base64 解码失败: {}", e))?;

        if combined.len() < 12 {
            return Err("数据格式错误：长度不足".to_string());
        }

        let (nonce_bytes, ciphertext) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let cipher = Aes256Gcm::new(&self.key.into());

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("解密失败: {}", e))?;

        String::from_utf8(plaintext).map_err(|e| format!("UTF-8 解码失败: {}", e))
    }

    /// 克隆密钥（用于数据迁移时多个存储实例共享同一密钥）
    pub fn clone_key(&self) -> Self {
        Crypto { key: self.key }
    }
}

/// 生成随机密码
pub fn generate_password(length: usize) -> String {
    let chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789!@#$%&*";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..chars.len());
            chars.as_bytes()[idx] as char
        })
        .collect()
}
