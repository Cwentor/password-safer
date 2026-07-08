// ============================================
// 移动端通知模块（Task 4）
//
// 设计说明：
// - Android 同步调度器在后台运行时，需要常驻通知维持进程存活
// - 同步状态变化（完成/失败）时显示状态通知
// - 用户退出登录时取消所有通知
// - 使用 tauri-plugin-notification 提供的 NotificationExt API
// - 通知使用默认频道（不指定 channel_id），避免需要 create_channel 设置
// - ongoing() 仅 Android 生效：让用户无法滑动清除
// ============================================

use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

/// 同步服务运行中通知 ID（常驻）
const NOTIF_ID_RUNNING: i32 = 1001;

/// 同步状态通知 ID（完成/失败，复用同一 ID 以覆盖前一条）
const NOTIF_ID_STATUS: i32 = 1002;

/// 显示常驻通知：「同步服务运行中」
///
/// - 使用 `ongoing()` 让用户无法滑动清除（Android 行为）
/// - 调用场景：移动端启动同步调度器时；同步配置启用时
/// - 若通知已存在（同 ID），将被覆盖更新
pub fn show_sync_running<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.notification()
        .builder()
        .id(NOTIF_ID_RUNNING)
        .title("密码保险箱")
        .body("同步服务运行中")
        .ongoing()
        .show()
        .map_err(|e| format!("显示运行中通知失败: {}", e))
}

/// 显示同步完成通知（自动取消型，点击即消失）
///
/// - 调用场景：移动端同步调度器单次同步成功后
/// - 复用 NOTIF_ID_STATUS，覆盖前一条状态通知
pub fn show_sync_complete<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.notification()
        .builder()
        .id(NOTIF_ID_STATUS)
        .title("密码保险箱")
        .body("云同步完成")
        .auto_cancel()
        .show()
        .map_err(|e| format!("显示完成通知失败: {}", e))
}

/// 显示同步失败通知（自动取消型）
///
/// - 调用场景：移动端同步调度器单次同步失败后
/// - 复用 NOTIF_ID_STATUS，覆盖前一条状态通知
pub fn show_sync_error<R: Runtime>(app: &AppHandle<R>, msg: &str) -> Result<(), String> {
    app.notification()
        .builder()
        .id(NOTIF_ID_STATUS)
        .title("密码保险箱")
        .body(format!("同步失败: {}", msg))
        .auto_cancel()
        .show()
        .map_err(|e| format!("显示错误通知失败: {}", e))
}

/// 取消常驻「运行中」通知（保留状态通知）
///
/// - 调用场景：移动端用户关闭自动同步开关，但同步调度器仍运行
pub fn cancel_running<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.notification().remove_active(vec![NOTIF_ID_RUNNING]);
}

/// 取消所有通知（常驻 + 状态）
///
/// - 调用场景：移动端用户退出登录，或应用退出时
pub fn cancel_all<R: Runtime>(app: &AppHandle<R>) {
    let _ = app
        .notification()
        .remove_active(vec![NOTIF_ID_RUNNING, NOTIF_ID_STATUS]);
}
