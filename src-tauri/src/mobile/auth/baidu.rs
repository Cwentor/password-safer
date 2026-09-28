//! 移动端百度扫码登录：薄壳命令，实现在 [`crate::shared::login`]。
use crate::shared::login;

/// 打开百度网盘扫码登录窗口（移动端 WebView 方式）
#[tauri::command]
pub async fn open_baidu_login(app: tauri::AppHandle) -> Result<(), String> {
    login::open_login_window(app, login::baidu_spec()).await
}
