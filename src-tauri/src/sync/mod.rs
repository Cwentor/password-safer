pub mod auth;
pub mod baidu;
pub mod quark;

use crate::models::SyncResult;
use std::path::PathBuf;

/// 同步提供者 trait
/// 持有 Cookie 鉴权信息，只负责文件上传/下载，不关心 Cookie 来源
pub trait SyncProvider: Send + Sync {
    /// 上传本地文件到云端，覆盖远程
    fn upload(&self, local_path: &PathBuf, remote_path: &str) -> Result<(), String>;
    /// 从云端下载文件到本地，覆盖本地
    fn download(&self, remote_path: &str, local_path: &PathBuf) -> Result<(), String>;
    /// 提供者名称
    fn name(&self) -> &str;
    /// 获取远端文件最后修改时间（unix 秒），文件不存在返回 Ok(None)
    /// 默认实现返回 Err，表示该 provider 不支持
    #[allow(dead_code)]
    fn remote_file_mtime(&self, _remote_path: &str) -> Result<Option<i64>, String> {
        Err("remote_file_mtime not supported".to_string())
    }
}

/// 执行上传：本地 db 文件 → 网盘
pub fn sync_upload(
    provider: &dyn SyncProvider,
    local_path: &PathBuf,
    remote_path: &str,
) -> SyncResult {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    match provider.upload(local_path, remote_path) {
        Ok(()) => SyncResult {
            success: true,
            message: format!("已上传到 {}", provider.name()),
            synced_at: now,
        },
        Err(e) => SyncResult {
            success: false,
            message: format!("上传失败: {}", e),
            synced_at: now,
        },
    }
}

/// 执行下载：网盘 → 本地 db 文件
pub fn sync_download(
    provider: &dyn SyncProvider,
    remote_path: &str,
    local_path: &PathBuf,
) -> SyncResult {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    match provider.download(remote_path, local_path) {
        Ok(()) => SyncResult {
            success: true,
            message: format!("已从 {} 下载", provider.name()),
            synced_at: now,
        },
        Err(e) => SyncResult {
            success: false,
            message: format!("下载失败: {}", e),
            synced_at: now,
        },
    }
}
