//! 移动端夸克扫码登录：薄壳命令，实现在 [`crate::shared::login`]。
use crate::shared::login;

/// 打开夸克网盘扫码登录窗口（移动端 WebView 方式）
#[tauri::command]
pub async fn open_quark_login(app: tauri::AppHandle) -> Result<(), String> {
    login::open_login_window(app, login::quark_spec()).await
}

/// 保存夸克网盘登录 Cookie（由登录窗口的 JS 轮询通过 invoke 调用，
/// 命令名须与 shared::login 的轮询脚本一致）
#[tauri::command]
pub fn save_quark_cookie(cookie: String, app: tauri::AppHandle) -> Result<(), String> {
    login::save_login_cookie(&app, &login::quark_spec(), &cookie)
}
