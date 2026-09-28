//! 桌面端百度扫码登录：薄壳命令，实现在 [`crate::shared::login`]。
use crate::shared::login;

/// 打开百度网盘扫码登录窗口（WebView 方式）；
/// BDUSS 为 HttpOnly，由 Rust 探询线程经 cookies_for_url() 获取完整 Cookie
#[tauri::command]
pub async fn open_baidu_login(app: tauri::AppHandle) -> Result<(), String> {
    login::open_login_window(app, login::baidu_spec()).await
}
