pub mod auth;
pub mod notification;

use tauri::{AppHandle, Manager, Runtime};

/// 移动平台初始化入口（在 setup 闭包中调用）
///
/// 职责：
/// 1. 启动同步服务常驻通知（如果同步配置启用）
/// 2. 后续可在此扩展：Deep Link 回调、状态栏配置等
pub fn init<R: Runtime>(app: &AppHandle<R>) -> Result<(), tauri::Error> {
    // 启动常驻通知：若任一同步提供者已启用，显示「同步服务运行中」
    // 此处通过 AppHandle 获取 AppState，读取配置判断是否启用
    if let Some(state) = app.try_state::<std::sync::Arc<crate::AppState>>() {
        let config = state
            .database
            .lock()
            .unwrap()
            .load_config()
            .unwrap_or_default();

        let any_sync_enabled = (config.baidu_sync_enabled && !config.baidu_cookie.is_empty())
            || (config.quark_sync_enabled && !config.quark_cookie.is_empty());

        if any_sync_enabled {
            if let Err(e) = notification::show_sync_running(app) {
                eprintln!("[mobile] 启动常驻通知失败: {}", e);
            }
        }
    }

    Ok(())
}

/// 配置移动专属的 Builder 选项
/// Phase 1 无需配置，Phase 2 可在此注册 Deep Link 处理器等
pub fn configure_builder<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
}
