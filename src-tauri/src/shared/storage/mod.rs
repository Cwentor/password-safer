pub mod json_store;

use tauri::{AppHandle, Manager};

/// 获取应用数据目录，用于存储 vault.json 和 master.key
/// Windows: %APPDATA%\com.passwordsafer.app\
/// Android: App 私有目录
pub fn get_app_data_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to resolve app data dir")
}
