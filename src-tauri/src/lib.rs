mod shared;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

use shared::models::*;
use shared::sync::auth::{baidu::BaiduAuth, quark::QuarkAuth, QrAuthenticator, QrLoginSession, QrLoginStatus};
use shared::sync::SyncProvider;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

/// 应用全局状态
pub struct AppState {
    /// JSON 存储实例（每次操作都重新读取文件，无需 close/reopen）
    pub database: Mutex<shared::storage::json_store::JsonStore>,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    /// 夸克 Cookie 即将过期提醒是否已发送（避免重复提醒）
    pub quark_cookie_warn_sent: std::sync::atomic::AtomicBool,
}

/// 当前本地时间戳字符串
fn now_ts() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

// ========== Tauri Commands ==========

/// 获取所有密码
#[tauri::command]
fn get_all_passwords(state: State<'_, Arc<AppState>>) -> Result<Vec<PasswordDto>, String> {
    state.database.lock().unwrap().get_all()
}

/// 搜索密码（名字不完全匹配）
#[tauri::command]
fn search_passwords(query: String, state: State<'_, Arc<AppState>>) -> Result<Vec<PasswordDto>, String> {
    if query.is_empty() {
        return state.database.lock().unwrap().get_all();
    }
    state.database.lock().unwrap().search(&query)
}

/// 按标签筛选
#[tauri::command]
fn get_passwords_by_tag(tag: String, state: State<'_, Arc<AppState>>) -> Result<Vec<PasswordDto>, String> {
    state.database.lock().unwrap().get_by_tag(&tag)
}

/// 获取收藏
#[tauri::command]
fn get_favorites(state: State<'_, Arc<AppState>>) -> Result<Vec<PasswordDto>, String> {
    state.database.lock().unwrap().get_favorites()
}

/// 获取弱密码
#[tauri::command]
fn get_weak_passwords(state: State<'_, Arc<AppState>>) -> Result<Vec<PasswordDto>, String> {
    state.database.lock().unwrap().get_weak()
}

/// 新增密码
#[tauri::command]
fn add_password(data: PasswordInput, state: State<'_, Arc<AppState>>) -> Result<PasswordDto, String> {
    state.database.lock().unwrap().insert(&data)
}

/// 更新密码
#[tauri::command]
fn update_password(id: i64, data: PasswordInput, state: State<'_, Arc<AppState>>) -> Result<PasswordDto, String> {
    state.database.lock().unwrap().update(id, &data)
}

/// 删除密码
#[tauri::command]
fn delete_password(id: i64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.database.lock().unwrap().delete(id)
}

/// 切换收藏
#[tauri::command]
fn toggle_favorite(id: i64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.database.lock().unwrap().toggle_favorite(id)
}

/// 更新最后使用时间
#[tauri::command]
fn update_last_used(id: i64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.database.lock().unwrap().update_last_used(id)
}

/// 获取配置
#[tauri::command]
fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig, String> {
    state.database.lock().unwrap().load_config()
}

/// 保存配置
#[tauri::command]
fn save_config(config: AppConfig, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.database.lock().unwrap().save_config(&config)
}

/// 立即同步（上传或下载）
#[tauri::command]
async fn sync_now(
    provider: String,
    direction: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<SyncResult, String> {
    let config = state.database.lock().unwrap().load_config()?;
    let db_path = state.db_path.clone();

    // 取出对应提供者的 Cookie 与远程路径
    let (cookie, remote_path) = match provider.as_str() {
        "baidu" => (config.baidu_cookie.clone(), config.baidu_remote_path.clone()),
        "quark" => (config.quark_cookie.clone(), config.quark_remote_path.clone()),
        _ => {
            return Ok(SyncResult {
                success: false,
                message: format!("未知的同步提供者: {}", provider),
                synced_at: now_ts(),
            });
        }
    };

    if cookie.is_empty() {
        return Ok(SyncResult {
            success: false,
            message: "尚未扫码登录，请先在设置中扫码绑定".to_string(),
            synced_at: now_ts(),
        });
    }

    // 诊断日志：打印 Cookie 长度和是否包含 HttpOnly 标志（__pus 通常是 HttpOnly）
    eprintln!("[sync] {} cookie.length={}, hasPuus={}, hasPus={}",
        provider, cookie.len(),
        cookie.contains("__puus="),
        cookie.contains("__pus="));

    // 上传前先校验 Cookie 是否仍然有效（非官方 Cookie 可能随时失效）
    if direction == "upload" {
        let p2 = provider.clone();
        let c2 = cookie.clone();
        let valid = tokio::task::spawn_blocking(move || match p2.as_str() {
            "baidu" => BaiduAuth::new().validate(&c2),
            "quark" => QuarkAuth::new().validate(&c2),
            _ => Ok(false),
        })
        .await
        .map_err(|e| format!("校验失败: {}", e))?;
        match valid {
            Ok(true) => {}
            Ok(false) => {
                return Ok(SyncResult {
                    success: false,
                    message: "登录已失效，请重新扫码".to_string(),
                    synced_at: now_ts(),
                });
            }
            Err(e) => {
                return Ok(SyncResult {
                    success: false,
                    message: format!("校验失败: {}", e),
                    synced_at: now_ts(),
                });
            }
        }
    }

    let provider_for_task = provider.clone();
    let cookie_for_task = cookie;
    let remote_for_task = remote_path;
    let direction_for_task = direction.clone();

    let result = tokio::task::spawn_blocking(move || {
        match provider_for_task.as_str() {
            "baidu" => {
                let p = shared::sync::baidu::BaiduProvider::new(cookie_for_task);
                if direction_for_task == "download" {
                    shared::sync::sync_download(&p, &remote_for_task, &db_path)
                } else {
                    shared::sync::sync_upload(&p, &db_path, &remote_for_task)
                }
            }
            "quark" => {
                // 本地 vault.json 有效性检测（仅日志，不改变行为）
                let local_valid = shared::sync::quark::QuarkProvider::local_vault_valid(&db_path);
                println!("[sync] quark local_vault_valid = {}", local_valid);
                let p = shared::sync::quark::QuarkProvider::new(cookie_for_task);
                if direction_for_task == "download" {
                    shared::sync::sync_download(&p, &remote_for_task, &db_path)
                } else {
                    shared::sync::sync_upload(&p, &db_path, &remote_for_task)
                }
            }
            _ => SyncResult {
                success: false,
                message: "未知的同步提供者".to_string(),
                synced_at: now_ts(),
            },
        }
    })
    .await
    .map_err(|e| format!("同步任务失败: {}", e))?;

    // 同步成功则更新 last_sync
    if result.success {
        let mut cfg = state.database.lock().unwrap().load_config().unwrap_or_default();
        match provider.as_str() {
            "baidu" => cfg.baidu_last_sync = now_ts(),
            "quark" => cfg.quark_last_sync = now_ts(),
            _ => {}
        }
        let _ = state.database.lock().unwrap().save_config(&cfg);

        // 下载成功后通知前端刷新（JSON 文件无需 reopen，下次 load() 自动读取新内容）
        if direction == "download" {
            let _ = app.emit("sync://restored", ());
            eprintln!("[sync] vault.json downloaded, emit sync://restored");
        }
    }

    Ok(result)
}

/// 检查同步连接（基于扫码登录 Cookie 校验）
#[tauri::command]
async fn check_sync_connection(
    provider: String,
    state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    let config = state.database.lock().unwrap().load_config()?;
    let cookie = match provider.as_str() {
        "baidu" => config.baidu_cookie,
        "quark" => config.quark_cookie,
        _ => return Err("未知的同步提供者".to_string()),
    };
    if cookie.is_empty() {
        return Ok(false);
    }
    let res: Result<bool, String> = tokio::task::spawn_blocking(move || match provider.as_str() {
        "baidu" => BaiduAuth::new().validate(&cookie),
        "quark" => QuarkAuth::new().validate(&cookie),
        _ => Ok(false),
    })
    .await
    .map_err(|e| format!("检查失败: {}", e))?;
    Ok(res.unwrap_or(false))
}

/// 启动扫码登录，返回二维码图片与轮询票据
#[tauri::command]
async fn qr_start(provider: String) -> Result<QrLoginSession, String> {
    tokio::task::spawn_blocking(move || match provider.as_str() {
        "baidu" => BaiduAuth::new().start_login(),
        "quark" => QuarkAuth::new().start_login(),
        _ => Err("未知的提供者".to_string()),
    })
    .await
    .map_err(|e| format!("启动扫码失败: {}", e))?
}

/// 轮询扫码登录状态；确认登录成功时会自动把 Cookie 与失效时间写入配置
#[tauri::command]
async fn qr_poll(
    provider: String,
    login_token: String,
    state: State<'_, Arc<AppState>>,
) -> Result<QrPollResult, String> {
    let provider_key = provider.clone();
    let status = tokio::task::spawn_blocking(move || match provider.as_str() {
        "baidu" => BaiduAuth::new().poll_login(&login_token),
        "quark" => QuarkAuth::new().poll_login(&login_token),
        _ => Err("未知的提供者".to_string()),
    })
    .await
    .map_err(|e| format!("轮询失败: {}", e))??;

    match status {
        QrLoginStatus::Waiting => Ok(QrPollResult {
            status: "waiting".to_string(),
            message: "等待扫码".to_string(),
        }),
        QrLoginStatus::Scanned => Ok(QrPollResult {
            status: "scanned".to_string(),
            message: "已扫码，请在手机上确认登录".to_string(),
        }),
        QrLoginStatus::Expired => Ok(QrPollResult {
            status: "expired".to_string(),
            message: "二维码已过期，请重新生成".to_string(),
        }),
        QrLoginStatus::Failed { message } => Ok(QrPollResult {
            status: "failed".to_string(),
            message,
        }),
        QrLoginStatus::Confirmed { cookie, expires_at } => {
            let mut config = state.database.lock().unwrap().load_config()?;
            match provider_key.as_str() {
                "baidu" => {
                    config.baidu_cookie = cookie;
                    config.baidu_cookie_expires_at = expires_at;
                }
                "quark" => {
                    config.quark_cookie = cookie;
                    config.quark_cookie_expires_at = expires_at;
                    // 重新登录，重置过期提醒标记
                    state.quark_cookie_warn_sent.store(false, std::sync::atomic::Ordering::SeqCst);
                }
                _ => {}
            }
            state.database.lock().unwrap().save_config(&config)?;
            Ok(QrPollResult {
                status: "confirmed".to_string(),
                message: format!("{} 登录成功", provider_key),
            })
        }
    }
}

/// 退出登录：清空对应提供者的 Cookie 与失效时间，并关闭自动同步
#[tauri::command]
fn logout(
    provider: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut config = state.database.lock().unwrap().load_config()?;
    match provider.as_str() {
        "baidu" => {
            config.baidu_cookie = String::new();
            config.baidu_cookie_expires_at = 0;
            config.baidu_sync_enabled = false;
        }
        "quark" => {
            config.quark_cookie = String::new();
            config.quark_cookie_expires_at = 0;
            config.quark_sync_enabled = false;
        }
        _ => return Err("未知的提供者".to_string()),
    }
    state.database.lock().unwrap().save_config(&config)?;

    // 移动端：若所有同步均已停用，取消常驻通知
    #[cfg(mobile)]
    {
        let cfg = state.database.lock().unwrap().load_config().unwrap_or_default();
        let any_sync = (cfg.baidu_sync_enabled && !cfg.baidu_cookie.is_empty())
            || (cfg.quark_sync_enabled && !cfg.quark_cookie.is_empty());
        if !any_sync {
            mobile::notification::cancel_all(&app);
        }
    }

    // 桌面端无通知模块，显式标记参数已使用以避免警告
    #[cfg(not(mobile))]
    let _ = &app;

    Ok(())
}

/// 从外部 JSON 文件导入数据（直接覆盖当前 vault.json）
#[tauri::command]
fn import_db(file_path: String, state: State<'_, Arc<AppState>>) -> Result<ImportResult, String> {
    let path = PathBuf::from(&file_path);
    if !path.exists() {
        return Ok(ImportResult {
            success: false,
            message: format!("文件不存在: {}", file_path),
            imported_count: 0,
        });
    }

    // 简化方案：直接复制外部 JSON 文件覆盖当前 vault.json
    // 注意：这会替换当前所有数据，外部文件必须是本应用导出的 vault.json 格式
    match std::fs::copy(&path, &state.db_path) {
        Ok(_) => {
            // 验证复制后的文件可解密
            match state.database.lock().unwrap().load() {
                Ok(data) => Ok(ImportResult {
                    success: true,
                    message: format!("成功导入，共 {} 条密码记录", data.passwords.len()),
                    imported_count: data.passwords.len(),
                }),
                Err(e) => Ok(ImportResult {
                    success: false,
                    message: format!("导入后验证失败（文件可能已损坏或不是本应用导出）: {}", e),
                    imported_count: 0,
                }),
            }
        }
        Err(e) => Ok(ImportResult {
            success: false,
            message: format!("复制文件失败: {}", e),
            imported_count: 0,
        }),
    }
}

/// 导出数据库文件到指定路径
#[tauri::command]
fn export_db(file_path: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    std::fs::copy(&state.db_path, &file_path).map_err(|e| format!("导出失败: {}", e))?;
    Ok(())
}

/// 生成随机密码
#[tauri::command]
fn generate_password(length: Option<usize>) -> Result<String, String> {
    let len = length.unwrap_or(16);
    Ok(shared::crypto::generate_password(len))
}

/// 获取数据库文件路径（供前端显示）
#[tauri::command]
fn get_db_info(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let size = std::fs::metadata(&state.db_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let count = state.database.lock().unwrap().get_all().map(|v| v.len()).unwrap_or(0);
    Ok(serde_json::json!({
        "path": state.db_path.to_string_lossy(),
        "size": size,
        "count": count,
    }))
}

// ========== 定时同步调度 ==========

/// 调度循环里每个提供者的同步结果处理：
/// - 成功：更新 last_sync
/// - Cookie 失效：关闭自动同步，发 sync://expired 事件通知前端重新扫码
/// - 其它失败：发 sync://error 事件
fn handle_sync_result(
    res: Result<Result<(), String>, String>,
    provider: &str,
    state: Arc<AppState>,
    app: tauri::AppHandle,
) {
    let inner = match res {
        Ok(r) => r,
        Err(e) => {
            let _ = app.emit(
                "sync://error",
                serde_json::json!({ "provider": provider, "message": format!("任务异常: {}", e) }),
            );
            return;
        }
    };

    match inner {
        Ok(()) => {
            let mut cfg = state.database.lock().unwrap().load_config().unwrap_or_default();
            match provider {
                "baidu" => cfg.baidu_last_sync = now_ts(),
                "quark" => cfg.quark_last_sync = now_ts(),
                _ => {}
            }
            let _ = state.database.lock().unwrap().save_config(&cfg);
            // 移动端：同步完成通知
            #[cfg(mobile)]
            {
                if let Err(e) = mobile::notification::show_sync_complete(&app) {
                    eprintln!("[mobile] 同步完成通知失败: {}", e);
                }
            }
        }
        Err(msg) if msg == "cookie_expired" => {
            let mut cfg = state.database.lock().unwrap().load_config().unwrap_or_default();
            match provider {
                "baidu" => cfg.baidu_sync_enabled = false,
                "quark" => cfg.quark_sync_enabled = false,
                _ => {}
            }
            let _ = state.database.lock().unwrap().save_config(&cfg);
            let _ = app.emit(
                "sync://expired",
                serde_json::json!({
                    "provider": provider,
                    "message": format!("{} 登录已失效，请重新扫码", provider)
                }),
            );
            // 移动端：Cookie 失效，取消常驻通知（同步调度器仍运行，但实际无任务可做）
            #[cfg(mobile)]
            {
                mobile::notification::cancel_running(&app);
            }
        }
        Err(msg) => {
            let _ = app.emit(
                "sync://error",
                serde_json::json!({ "provider": provider, "message": msg }),
            );
            // 移动端：同步失败通知
            #[cfg(mobile)]
            {
                if let Err(e) = mobile::notification::show_sync_error(&app, &msg) {
                    eprintln!("[mobile] 同步错误通知失败: {}", e);
                }
            }
        }
    }
}

/// 启动后台定时同步：每次上传前先 validate Cookie，失效则停用并通知前端
fn start_sync_scheduler(state: Arc<AppState>, app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let config = state.database.lock().unwrap().load_config().unwrap_or_default();

            // 百度网盘同步
            if config.baidu_sync_enabled && !config.baidu_cookie.is_empty() {
                let cookie = config.baidu_cookie.clone();
                let db_path = state.db_path.clone();
                let remote = config.baidu_remote_path.clone();
                let st = state.clone();
                let ap = app.clone();
                let res = tauri::async_runtime::spawn_blocking(move || match BaiduAuth::new().validate(&cookie) {
                    Ok(true) => {
                        let provider = shared::sync::baidu::BaiduProvider::new(cookie);
                        provider.upload(&db_path, &remote)
                    }
                    Ok(false) => Err("cookie_expired".to_string()),
                    Err(e) => Err(format!("validate:{}", e)),
                })
                .await
                .map_err(|e| format!("任务异常: {}", e));
                handle_sync_result(res, "baidu", st, ap);
            }

            // 夸克网盘同步（双向：本地无效→下载；本地有效→比较 mtime，新者覆盖旧者）
            if config.quark_sync_enabled && !config.quark_cookie.is_empty() {
                // Cookie 即将过期检测（距过期 < 7 天时提醒前端）
                let now_ts = chrono::Local::now().timestamp();
                let expires_at = config.quark_cookie_expires_at;
                if expires_at > 0 {
                    let secs_left = expires_at - now_ts;
                    if secs_left <= 0 {
                        // 已过期，走原有 cookie_expired 逻辑（由 validate 返回 false 触发）
                    } else if secs_left < 7 * 86400 {
                        // 即将过期（< 7 天）
                        if !state.quark_cookie_warn_sent.load(std::sync::atomic::Ordering::SeqCst) {
                            let days_left = secs_left / 86400;
                            let _ = app.emit(
                                "sync://cookie_expiring",
                                serde_json::json!({
                                    "provider": "quark",
                                    "days_left": days_left,
                                    "message": format!("夸克登录将在 {} 天后过期，请及时重新扫码", days_left)
                                }),
                            );
                            state.quark_cookie_warn_sent.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    } else {
                        // 距过期还远，重置提醒标记（便于下次过期前再次提醒）
                        state.quark_cookie_warn_sent.store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                }

                let cookie = config.quark_cookie.clone();
                let db_path = state.db_path.clone();
                let remote = config.quark_remote_path.clone();
                let st = state.clone();
                let st_inner = state.clone();
                let ap = app.clone();
                let res = tauri::async_runtime::spawn_blocking(move || match QuarkAuth::new().validate(&cookie) {
                    Ok(true) => {
                        let provider = shared::sync::quark::QuarkProvider::new(cookie);

                        // 1. 检测本地 vault.json 有效性
                        let local_valid = shared::sync::quark::QuarkProvider::local_vault_valid(&db_path);

                        if !local_valid {
                            // 2. 本地无效 → 查云端是否有文件
                            match provider.remote_file_mtime(&remote) {
                                Ok(Some(remote_mtime)) => {
                                    // 云端有文件 → 下载（JSON 文件无需 close/reopen）
                                    provider.download(&remote, &db_path)?;
                                    // 下载成功，记录云端 mtime
                                    let mut cfg = st_inner.database.lock().unwrap().load_config().unwrap_or_default();
                                    cfg.quark_last_remote_mtime = remote_mtime;
                                    let _ = st_inner.database.lock().unwrap().save_config(&cfg);
                                    Ok(())
                                }
                                Ok(None) => {
                                    // 云端也无文件 → 跳过
                                    println!("[sync] quark: 本地与云端均无有效数据，跳过");
                                    Ok(())
                                }
                                Err(e) => Err(format!("获取远端文件信息失败: {}", e)),
                            }
                        } else {
                            // 3. 本地有效 → 获取云端 mtime
                            match provider.remote_file_mtime(&remote) {
                                Ok(None) => {
                                    // 4. 云端不存在 → 上传
                                    provider.upload(&db_path, &remote)?;
                                    // 上传成功后查询云端 mtime
                                    let remote_mtime = provider.remote_file_mtime(&remote)
                                        .ok().flatten().unwrap_or(0);
                                    let mut cfg = st_inner.database.lock().unwrap().load_config().unwrap_or_default();
                                    cfg.quark_last_remote_mtime = remote_mtime;
                                    let _ = st_inner.database.lock().unwrap().save_config(&cfg);
                                    Ok(())
                                }
                                Ok(Some(remote_mtime)) => {
                                    // 5. 云端存在 → 比较时间戳
                                    let local_mtime = std::fs::metadata(&db_path)
                                        .ok()
                                        .and_then(|m| m.modified().ok())
                                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                        .map(|d| d.as_secs() as i64)
                                        .unwrap_or(0);

                                    if local_mtime > remote_mtime {
                                        // 本地更新 → 上传
                                        provider.upload(&db_path, &remote)?;
                                        let mut cfg = st_inner.database.lock().unwrap().load_config().unwrap_or_default();
                                        cfg.quark_last_remote_mtime = local_mtime;
                                        let _ = st_inner.database.lock().unwrap().save_config(&cfg);
                                        Ok(())
                                    } else if local_mtime < remote_mtime {
                                        // 云端更新 → 下载（JSON 文件无需 close/reopen）
                                        provider.download(&remote, &db_path)?;
                                        let mut cfg = st_inner.database.lock().unwrap().load_config().unwrap_or_default();
                                        cfg.quark_last_remote_mtime = remote_mtime;
                                        let _ = st_inner.database.lock().unwrap().save_config(&cfg);
                                        Ok(())
                                    } else {
                                        // 时间戳相等 → 跳过
                                        println!("[sync] quark: 本地与云端时间戳一致，跳过同步");
                                        let mut cfg = st_inner.database.lock().unwrap().load_config().unwrap_or_default();
                                        cfg.quark_last_remote_mtime = remote_mtime;
                                        let _ = st_inner.database.lock().unwrap().save_config(&cfg);
                                        Ok(())
                                    }
                                }
                                Err(e) => Err(format!("获取远端文件信息失败: {}", e)),
                            }
                        }
                    }
                    Ok(false) => Err("cookie_expired".to_string()),
                    Err(e) => Err(format!("validate:{}", e)),
                })
                .await
                .map_err(|e| format!("任务异常: {}", e));
                handle_sync_result(res, "quark", st, ap);
            }

            // 取较短的启用间隔作为轮询周期
            let interval = if config.baidu_sync_enabled {
                config.baidu_sync_interval
            } else if config.quark_sync_enabled {
                config.quark_sync_interval
            } else {
                300 // 默认 5 分钟检查一次
            };

            tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
        }
    });
}

// ========== 应用入口 ==========

/// 桌面端数据目录迁移：从旧路径（dirs::data_dir()/password-safer）迁移到
/// Tauri 标准 app_data_dir（%APPDATA%\com.passwordsafer.app\）
/// 仅在新目录尚无 vault.json 且旧目录存在时执行，避免数据丢失
#[cfg(desktop)]
fn migrate_from_old_data_dir(new_data_dir: &std::path::Path) {
    let old_data_dir = match dirs::data_dir() {
        Some(d) => d.join("password-safer"),
        None => return,
    };

    if !old_data_dir.exists() {
        return;
    }

    // 新目录已有 vault.json，说明已迁移过，跳过
    if new_data_dir.join("vault.json").exists() {
        return;
    }

    eprintln!(
        "[migrate] 检测到旧数据目录: {:?}，迁移到 {:?}",
        old_data_dir, new_data_dir
    );

    // 复制旧目录下所有文件到新目录
    if let Ok(entries) = std::fs::read_dir(&old_data_dir) {
        for entry in entries.flatten() {
            let from = entry.path();
            if from.is_file() {
                let to = new_data_dir.join(entry.file_name());
                match std::fs::copy(&from, &to) {
                    Ok(_) => eprintln!("[migrate] 已迁移文件: {:?}", entry.file_name()),
                    Err(e) => eprintln!("[migrate] 复制 {:?} 失败: {}", from, e),
                }
            }
        }
    }

    eprintln!("[migrate] 数据目录迁移完成（旧目录保留，可手动删除）");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 创建 Builder，应用平台专属配置（URI scheme、窗口事件等）
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = desktop::configure_builder(builder);
    #[cfg(mobile)]
    let builder = mobile::configure_builder(builder);

    // 注册命令处理器（共享命令 + 平台专属命令）
    #[cfg(desktop)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_all_passwords,
        search_passwords,
        get_passwords_by_tag,
        get_favorites,
        get_weak_passwords,
        add_password,
        update_password,
        delete_password,
        toggle_favorite,
        update_last_used,
        get_config,
        save_config,
        sync_now,
        check_sync_connection,
        qr_start,
        qr_poll,
        logout,
        import_db,
        export_db,
        generate_password,
        get_db_info,
        desktop::auth::quark::open_quark_login,
        desktop::auth::quark::save_quark_cookie,
        desktop::auth::baidu::open_baidu_login,
    ]);

    #[cfg(mobile)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        get_all_passwords,
        search_passwords,
        get_passwords_by_tag,
        get_favorites,
        get_weak_passwords,
        add_password,
        update_password,
        delete_password,
        toggle_favorite,
        update_last_used,
        get_config,
        save_config,
        sync_now,
        check_sync_connection,
        qr_start,
        qr_poll,
        logout,
        import_db,
        export_db,
        generate_password,
        get_db_info,
        mobile::auth::quark::open_quark_login,
        mobile::auth::baidu::open_baidu_login,
    ]);

    builder
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            // 初始化数据目录（通过 Tauri AppHandle 获取跨平台路径）
            // 仅保留快速路径计算与目录创建，避免阻塞 Android 主线程触发 ANR
            let data_dir = shared::storage::get_app_data_dir(app.handle());
            std::fs::create_dir_all(&data_dir).ok();

            let app_handle = app.handle().clone();

            // 同步 I/O 与解密操作放入异步任务，setup 立即返回 Ok(())
            tauri::async_runtime::spawn(async move {
                // spawn_blocking 在独立线程池执行同步 I/O，不阻塞主线程
                let blocking_result = tauri::async_runtime::spawn_blocking(move || -> Result<Arc<AppState>, String> {
                    // 桌面端：从旧路径（password-safer）迁移到标准 app_data_dir
                    #[cfg(desktop)]
                    migrate_from_old_data_dir(&data_dir);

                    let db_path = data_dir.join("vault.json");
                    let key_path = data_dir.join("master.key");

                    // 初始化加密器
                    let crypto = shared::crypto::Crypto::from_key_file(&key_path)?;

                    // 初始化 JSON 存储（双重加密：密码字段加密 + 文件整体加密）
                    let database =
                        shared::storage::json_store::JsonStore::open(db_path.clone(), crypto)?;

                    let state = Arc::new(AppState {
                        database: Mutex::new(database),
                        db_path,
                        data_dir,
                        quark_cookie_warn_sent: std::sync::atomic::AtomicBool::new(false),
                    });

                    Ok(state)
                })
                .await;

                match blocking_result {
                    Ok(Ok(state)) => {
                        // AppState 注入（必须在平台 init 与 sync 调度之前）
                        app_handle.manage(state.clone());

                        // 平台专属初始化（系统托盘等）
                        #[cfg(desktop)]
                        {
                            if let Err(e) = desktop::init(&app_handle) {
                                let _ = app_handle
                                    .emit("app-error", format!("桌面端初始化失败: {:?}", e));
                                return;
                            }
                        }
                        #[cfg(mobile)]
                        {
                            if let Err(e) = mobile::init(&app_handle) {
                                let _ = app_handle
                                    .emit("app-error", format!("移动端初始化失败: {:?}", e));
                                return;
                            }
                        }

                        start_sync_scheduler(state, app_handle.clone());

                        // 通知前端：应用初始化完成，可以开始调用 invoke
                        let _ = app_handle.emit("app-ready", ());
                    }
                    Ok(Err(e)) => {
                        // spawn_blocking 内部返回的 Err（加密器/存储初始化失败）
                        let _ = app_handle.emit("app-error", e);
                    }
                    Err(e) => {
                        // spawn_blocking 任务本身 panic 或被取消
                        let _ = app_handle
                            .emit("app-error", format!("初始化任务异常: {:?}", e));
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
