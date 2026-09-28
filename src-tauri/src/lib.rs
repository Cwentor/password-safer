mod shared;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

use shared::models::*;
use shared::sync::auth::{baidu::BaiduAuth, quark::QuarkAuth, QrAuthenticator, QrLoginSession, QrLoginStatus};
use shared::sync::engine;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

/// 应用全局状态
pub struct AppState {
    /// JSON 存储实例（每次操作都重新读取文件，无需 close/reopen）；
    /// 套 Arc 是为了让云同步引擎的 VaultAccess 能持引用走单一写入口
    pub database: Arc<Mutex<shared::storage::json_store::JsonStore>>,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    /// Cookie 即将过期提醒是否已发送（避免重复提醒，按网盘各记一个）
    pub quark_cookie_warn_sent: std::sync::atomic::AtomicBool,
    pub baidu_cookie_warn_sent: std::sync::atomic::AtomicBool,
}

/// 密码簿文件访问的生产实现：写入一律经 Mutex<JsonStore> 单一入口，
/// 供云同步引擎下载后替换本地密码簿使用
struct VaultAccess {
    db_path: PathBuf,
    database: Arc<Mutex<shared::storage::json_store::JsonStore>>,
}

impl engine::VaultIo for VaultAccess {
    fn path(&self) -> &Path {
        &self.db_path
    }

    fn local_mtime(&self) -> Option<i64> {
        std::fs::metadata(&self.db_path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
    }

    fn replace(&self, bytes: &[u8]) -> Result<(), String> {
        self.database.lock().unwrap().replace_raw(bytes)
    }
}

/// 当前本地时间戳字符串
fn now_ts() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// 网盘显示名（提示信息与事件用）
fn provider_display(provider: &str) -> &'static str {
    match provider {
        "baidu" => "百度网盘",
        "quark" => "夸克网盘",
        _ => "未知网盘",
    }
}

// ========== Tauri Commands ==========

/// 获取所有密码
#[tauri::command]
fn get_all_passwords(state: State<'_, Arc<AppState>>) -> Result<Vec<PasswordDto>, String> {
    state.database.lock().unwrap().get_all()
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

/// 立即同步（上传或下载）；方向由用户在界面上指定。
/// 策略与执行都在云同步引擎里，这里只做输入组装与结果落地。
#[tauri::command]
async fn sync_now(
    provider: String,
    direction: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<SyncResult, String> {
    let config = state.database.lock().unwrap().load_config()?;
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

    let db_path = state.db_path.clone();
    let database = state.database.clone();
    let provider_key = provider.clone();
    let is_download = direction == "download";

    let report = tauri::async_runtime::spawn_blocking(move || {
        let (provider, validator) = match shared::sync::build_provider(&provider_key, cookie.clone()) {
            Ok(x) => x,
            Err(e) => return engine::SyncReport::new(engine::Outcome::Failed(e)),
        };
        let vault = VaultAccess { db_path, database };
        engine::run_manual(engine::ManualInputs {
            provider: provider.as_ref(),
            validator: Some(validator.as_ref()),
            cookie: &cookie,
            remote_path: &remote_path,
            direction: if is_download {
                engine::Direction::Download
            } else {
                engine::Direction::Upload
            },
            vault: &vault,
        })
    })
    .await
    .map_err(|e| format!("同步任务失败: {}", e))?;

    // 同步成功则更新 last_sync
    if report.record_sync {
        let mut cfg = state.database.lock().unwrap().load_config().unwrap_or_default();
        match provider.as_str() {
            "baidu" => cfg.baidu_last_sync = now_ts(),
            "quark" => cfg.quark_last_sync = now_ts(),
            _ => {}
        }
        let _ = state.database.lock().unwrap().save_config(&cfg);
    }

    // 下载成功后通知前端刷新（JSON 文件无需 reopen，下次 load() 自动读取新内容）
    if report.downloaded {
        let _ = app.emit("sync://restored", ());
        eprintln!("[sync] vault.json downloaded, emit sync://restored");
    }

    let (success, message) = match &report.outcome {
        engine::Outcome::Uploaded => (true, format!("已上传到 {}", provider_display(&provider))),
        engine::Outcome::Downloaded => (true, format!("已从 {} 下载", provider_display(&provider))),
        engine::Outcome::CookieExpired => (false, "登录已失效，请重新扫码".to_string()),
        engine::Outcome::Failed(e) => (false, e.clone()),
        engine::Outcome::Skipped | engine::Outcome::NotLoggedIn => (false, "无需同步".to_string()),
    };
    Ok(SyncResult {
        success,
        message,
        synced_at: now_ts(),
    })
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

/// 从外部文件导入数据（整体替换当前密码簿）
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

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            return Ok(ImportResult {
                success: false,
                message: format!("读取文件失败: {}", e),
                imported_count: 0,
            });
        }
    };

    // 单一写入口：先验证可解密、可解析，再原子替换；验证不过不会破坏现有数据
    match state.database.lock().unwrap().replace_raw(&bytes) {
        Ok(()) => {
            let imported_count = state
                .database
                .lock()
                .unwrap()
                .get_all()
                .map(|v| v.len())
                .unwrap_or(0);
            Ok(ImportResult {
                success: true,
                message: format!("成功导入，共 {} 条密码记录", imported_count),
                imported_count,
            })
        }
        Err(e) => Ok(ImportResult {
            success: false,
            message: format!("导入失败: {}", e),
            imported_count: 0,
        }),
    }
}

/// 导出数据库文件到指定路径（复制在锁内进行，不与写入并发）
#[tauri::command]
fn export_db(file_path: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let _guard = state.database.lock().unwrap();
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

/// 把云同步引擎的报告落地：所有同步副作用的唯一出口。
/// 配置只做一次读-改-写；事件与移动端通知按结局分发。
fn apply_sync_report(
    provider: &str,
    report: engine::SyncReport,
    state: &Arc<AppState>,
    app: &tauri::AppHandle,
) {
    // 过期预警标记与事件
    if let Some(v) = report.set_warn_sent {
        match provider {
            "baidu" => state
                .baidu_cookie_warn_sent
                .store(v, std::sync::atomic::Ordering::SeqCst),
            "quark" => state
                .quark_cookie_warn_sent
                .store(v, std::sync::atomic::Ordering::SeqCst),
            _ => {}
        }
    }
    if let Some(w) = report.expiry_warning {
        let _ = app.emit(
            "sync://cookie_expiring",
            serde_json::json!({
                "provider": provider,
                "days_left": w.days_left,
                "message": format!("{} 登录将在 {} 天后过期，请及时重新扫码", provider_display(provider), w.days_left)
            }),
        );
    }

    match report.outcome {
        engine::Outcome::Uploaded | engine::Outcome::Downloaded | engine::Outcome::Skipped => {
            if report.record_sync {
                let mut cfg = state.database.lock().unwrap().load_config().unwrap_or_default();
                match provider {
                    "baidu" => cfg.baidu_last_sync = now_ts(),
                    "quark" => cfg.quark_last_sync = now_ts(),
                    _ => {}
                }
                let _ = state.database.lock().unwrap().save_config(&cfg);
            }
            if report.downloaded {
                let _ = app.emit("sync://restored", ());
            }
            // 移动端：同步完成通知
            #[cfg(mobile)]
            {
                if let Err(e) = mobile::notification::show_sync_complete(app) {
                    eprintln!("[mobile] 同步完成通知失败: {}", e);
                }
            }
        }
        engine::Outcome::NotLoggedIn => {}
        engine::Outcome::CookieExpired => {
            // Cookie 失效：关闭自动同步，通知前端重新扫码
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
                mobile::notification::cancel_running(app);
            }
        }
        engine::Outcome::Failed(msg) => {
            let _ = app.emit(
                "sync://error",
                serde_json::json!({ "provider": provider, "message": msg }),
            );
            // 移动端：同步失败通知
            #[cfg(mobile)]
            {
                if let Err(e) = mobile::notification::show_sync_error(app, &msg) {
                    eprintln!("[mobile] 同步错误通知失败: {}", e);
                }
            }
        }
    }
}

/// 启动后台定时同步。调度器只负责到点触发；方向判定、过期预警、记账
/// 都在云同步引擎（shared::sync::engine）里，结果经 apply_sync_report 落地。
fn start_sync_scheduler(state: Arc<AppState>, app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let config = state.database.lock().unwrap().load_config().unwrap_or_default();
            let now_secs = chrono::Local::now().timestamp();

            for name in ["baidu", "quark"] {
                let (enabled, cookie, remote, expires_at, warn_sent) = match name {
                    "baidu" => (
                        config.baidu_sync_enabled,
                        config.baidu_cookie.clone(),
                        config.baidu_remote_path.clone(),
                        config.baidu_cookie_expires_at,
                        state
                            .baidu_cookie_warn_sent
                            .load(std::sync::atomic::Ordering::SeqCst),
                    ),
                    "quark" => (
                        config.quark_sync_enabled,
                        config.quark_cookie.clone(),
                        config.quark_remote_path.clone(),
                        config.quark_cookie_expires_at,
                        state
                            .quark_cookie_warn_sent
                            .load(std::sync::atomic::Ordering::SeqCst),
                    ),
                    _ => continue,
                };
                if !enabled || cookie.is_empty() {
                    continue;
                }

                let st = state.clone();
                let res = tauri::async_runtime::spawn_blocking(move || {
                    let (provider, validator) = match shared::sync::build_provider(name, cookie.clone()) {
                        Ok(x) => x,
                        Err(e) => return engine::SyncReport::new(engine::Outcome::Failed(e)),
                    };
                    let vault = VaultAccess {
                        db_path: st.db_path.clone(),
                        database: st.database.clone(),
                    };
                    engine::run_cycle(engine::CycleInputs {
                        provider: provider.as_ref(),
                        validator: validator.as_ref(),
                        cookie: &cookie,
                        remote_path: &remote,
                        cookie_expires_at: expires_at,
                        warn_sent,
                        now_secs,
                        vault: &vault,
                    })
                })
                .await
                .map_err(|e| format!("任务异常: {}", e));

                match res {
                    Ok(report) => apply_sync_report(name, report, &state, &app),
                    Err(e) => {
                        let _ = app.emit(
                            "sync://error",
                            serde_json::json!({ "provider": name, "message": e }),
                        );
                    }
                }
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
        add_password,
        update_password,
        delete_password,
        toggle_favorite,
        update_last_used,
        get_config,
        save_config,
        sync_now,
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
        add_password,
        update_password,
        delete_password,
        toggle_favorite,
        update_last_used,
        get_config,
        save_config,
        sync_now,
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
                        database: Arc::new(Mutex::new(database)),
                        db_path,
                        data_dir,
                        quark_cookie_warn_sent: std::sync::atomic::AtomicBool::new(false),
                        baidu_cookie_warn_sent: std::sync::atomic::AtomicBool::new(false),
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
