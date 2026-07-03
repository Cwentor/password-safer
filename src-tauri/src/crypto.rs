use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose, Engine};
use rand::{Rng, RngCore};

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
