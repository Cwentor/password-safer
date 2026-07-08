use tauri::{Manager, Runtime};

/// 创建系统托盘：菜单、图标、左键切换窗口、菜单事件
pub fn create_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), tauri::Error> {
    let show_item = tauri::menu::MenuItem::with_id(app, "tray_show", "显示主窗口", true, None::<&str>)?;
    let hide_item = tauri::menu::MenuItem::with_id(app, "tray_hide", "隐藏主窗口", true, None::<&str>)?;
    let quit_item = tauri::menu::MenuItem::with_id(app, "tray_quit", "退出", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&show_item, &hide_item, &quit_item])?;

    tauri::tray::TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Password Safer - 密码保管箱")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    match window.is_visible() {
                        Ok(true) => { let _ = window.hide(); }
                        _ => {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                }
            }
        })
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "tray_show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "tray_hide" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
                "tray_quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}
