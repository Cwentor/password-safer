pub mod auth;
pub mod tray;
pub mod window;

use tauri::{AppHandle, Runtime};

/// 桌面平台初始化入口（在 setup 闭包中调用）
/// 创建系统托盘等桌面专属 UI 元素
pub fn init<R: Runtime>(app: &AppHandle<R>) -> Result<(), tauri::Error> {
    tray::create_tray(app)?;
    Ok(())
}

/// 配置桌面专属的 Builder 选项
/// 注册 URI scheme 协议、窗口事件拦截等
pub fn configure_builder<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    let builder = auth::quark::register_uri_scheme(builder);
    let builder = window::configure_window_events(builder);
    builder
}
