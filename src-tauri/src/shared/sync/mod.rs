pub mod auth;
pub mod baidu;
pub mod engine;
pub mod quark;

use std::path::Path;

/// 同步提供者 trait
/// 持有 Cookie 鉴权信息，只负责网盘上的文件传输，不关心 Cookie 来源、
/// 不关心密码簿内容：下载返回字节，写入一律由调用方经单一入口完成
pub trait SyncProvider: Send + Sync {
    /// 上传本地文件到云端，覆盖远程
    fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), String>;
    /// 从云端下载文件内容并返回字节（不落盘，由调用方决定如何写入）
    fn download(&self, remote_path: &str) -> Result<Vec<u8>, String>;
    /// 获取远端文件最后修改时间（unix 秒），文件不存在返回 Ok(None)
    /// 默认实现返回 Err，表示该 provider 不支持
    fn remote_file_mtime(&self, _remote_path: &str) -> Result<Option<i64>, String> {
        Err("remote_file_mtime not supported".to_string())
    }
}

/// 按名称构造同步提供者与其 Cookie 校验器（按网盘分派的唯一位置）
pub fn build_provider(
    name: &str,
    cookie: String,
) -> Result<(Box<dyn SyncProvider>, Box<dyn auth::QrAuthenticator>), String> {
    match name {
        "baidu" => Ok((
            Box::new(baidu::BaiduProvider::new(cookie)),
            Box::new(auth::baidu::BaiduAuth::new()),
        )),
        "quark" => Ok((
            Box::new(quark::QuarkProvider::new(cookie)),
            Box::new(auth::quark::QuarkAuth::new()),
        )),
        _ => Err(format!("未知的同步提供者: {}", name)),
    }
}

/// 本地密码簿文件有效性：文件存在且非空
pub fn local_vault_valid(local_path: &Path) -> bool {
    match std::fs::metadata(local_path) {
        Ok(m) => m.len() > 0,
        Err(_) => false,
    }
}
