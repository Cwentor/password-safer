pub mod auth;
pub mod baidu;
pub mod engine;
pub mod manifest;
pub mod quark;
pub mod session;

use std::path::Path;

/// 远端目录条目（list 返回；mtime 仅展示用，方向判定不依赖它）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteFile {
    pub name: String,
    pub size: i64,
    /// unix 秒
    pub mtime: i64,
}

/// 同步提供者 trait
/// 持有 Cookie 鉴权信息，只负责网盘上的文件传输，不关心 Cookie 来源、
/// 不关心密码簿内容：下载返回字节，写入一律由调用方经单一入口完成。
///
/// 追加式同步约定（远端不可覆盖、仅允许新增）：
/// - upload 语义为"新增一个远端文件"，绝不删除/覆盖已有文件；
///   调用方保证 remote_path 全局唯一（内容哈希 / 时间戳+设备号命名）
/// - delete 仅用于 legacy 迁移补删与历史清理，同步主流程不得调用
pub trait SyncProvider: Send + Sync {
    /// 上传本地文件到云端（新增，不覆盖；同名冲突由网盘报错暴露）
    fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), String>;
    /// 从云端下载文件内容并返回字节（不落盘，由调用方决定如何写入）
    fn download(&self, remote_path: &str) -> Result<Vec<u8>, String>;
    /// 列出远端目录一层内容（分页内部消化）；目录不存在返回空列表
    fn list(&self, remote_dir: &str) -> Result<Vec<RemoteFile>, String>;
    /// 删除远端单个文件（幂等：目标不存在视为成功）
    fn delete(&self, remote_path: &str) -> Result<(), String>;
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
