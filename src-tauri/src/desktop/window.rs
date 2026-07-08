use tauri::Runtime;

/// 配置窗口事件：拦截主窗口的 CloseRequested，改为隐藏窗口
pub fn configure_window_events<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            if window.label() == "main" {
                api.prevent_close();
                let _ = window.hide();
            }
        }
    })
}
