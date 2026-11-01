mod shared;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

use shared::models::*;
use shared::storage::sync_state::SyncState;
use shared::sync::auth::{baidu::BaiduAuth, quark::QuarkAuth, QrAuthenticator, QrLoginSession, QrLoginStatus};
use shared::sync::{engine, session};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

/// 应用全局状态
pub struct AppState {
    /// JSON 存储实例；None = 密码簿锁定（v2 无本机缓存）或待建库（首启）。
    /// 套 Arc 是为了让云同步引擎的 VaultAccess 能持引用走单一写入口
    pub database: Arc<Mutex<Option<shared::storage::json_store::JsonStore>>>,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    /// Cookie 即将过期提醒是否已发送（避免重复提醒，按网盘各记一个）
    pub quark_cookie_warn_sent: std::sync::atomic::AtomicBool,
    pub baidu_cookie_warn_sent: std::sync::atomic::AtomicBool,
}

impl AppState {
    /// 锁住数据库槽位；`as_ref()/as_mut()` 为 None 即密码簿锁定/待建库
    fn db(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<shared::storage::json_store::JsonStore>>, String>
    {
        self.database
            .lock()
            .map_err(|e| format!("数据库锁异常: {}", e))
    }
}

const LOCKED_MSG: &str = "密码簿已锁定，请先解锁";

/// 密码簿文件访问的生产实现：写入一律经 Mutex 单一入口，
/// 供云同步引擎下载后替换本地密码簿使用
struct VaultAccess {
    db_path: PathBuf,
    database: Arc<Mutex<Option<shared::storage::json_store::JsonStore>>>,
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

    fn read(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.db_path).map_err(|e| format!("读取密码簿失败: {}", e))
    }

    fn replace(&self, bytes: &[u8]) -> Result<(), String> {
        let mut g = self.database.lock().map_err(|e| e.to_string())?;
        let store = g.as_mut().ok_or(LOCKED_MSG)?;
        store.replace_raw(bytes)
    }
}

/// 同步记账访问的生产实现（sync_state.json）
struct SyncStateAccess {
    path: PathBuf,
}

impl session::SyncStateIo for SyncStateAccess {
    fn load(&self) -> Result<SyncState, String> {
        SyncState::load_or_create(self.path.clone())
    }

    fn save(&self, st: &SyncState) -> Result<(), String> {
        st.save()
    }
}

/// 给（重新打开的）store 注册用户写回调：刷新 sync_state.local_dirty_at
fn attach_user_save_callback(store: &mut shared::storage::json_store::JsonStore, data_dir: &Path) {
    let sync_state_path = data_dir.join("sync_state.json");
    store.set_on_user_save(Box::new(move || {
        match SyncState::load_or_create(sync_state_path.clone()) {
            Ok(mut st) => {
                st.local_dirty_at = chrono::Local::now().timestamp();
                if let Err(e) = st.save() {
                    eprintln!("[sync_state] 刷新 local_dirty_at 失败: {}", e);
                }
            }
            Err(e) => eprintln!("[sync_state] 刷新 local_dirty_at 读取失败: {}", e),
        }
    }));
}

/// 库密码最低要求（拍板：≥8 位；强度提示由前端展示）
fn check_password_strength(p: &str) -> Result<(), String> {
    if p.chars().count() < 8 {
        return Err("库密码至少需要 8 位".to_string());
    }
    Ok(())
}

/// 当前本地时间戳字符串
fn now_ts() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// 把最近一次同步时间写进本地记账 sync_state.json。
/// 不再写入密码簿内的 AppConfig——那会让每轮同步都改动 vault.json，
/// 下轮就被 mtime 判为"本地较新"而重复上传。
fn record_last_sync(state: &Arc<AppState>, provider: &str) {
    let path = state.data_dir.join("sync_state.json");
    match SyncState::load_or_create(path) {
        Ok(mut st) => {
            if let Some(p) = st.provider_mut(provider) {
                p.last_sync = now_ts();
            }
            if let Err(e) = st.save() {
                eprintln!("[sync_state] 记录同步时间失败: {}", e);
            }
        }
        Err(e) => eprintln!("[sync_state] 读取失败: {}", e),
    }
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
    state
        .db()?
        .as_ref()
        .ok_or(LOCKED_MSG)?
        .get_all()
}

/// 新增密码
#[tauri::command]
fn add_password(data: PasswordInput, state: State<'_, Arc<AppState>>) -> Result<PasswordDto, String> {
    state.db()?.as_mut().ok_or(LOCKED_MSG)?.insert(&data)
}

/// 更新密码
#[tauri::command]
fn update_password(id: i64, data: PasswordInput, state: State<'_, Arc<AppState>>) -> Result<PasswordDto, String> {
    state.db()?.as_mut().ok_or(LOCKED_MSG)?.update(id, &data)
}

/// 删除密码
#[tauri::command]
fn delete_password(id: i64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.db()?.as_mut().ok_or(LOCKED_MSG)?.delete(id)
}

/// 切换收藏
#[tauri::command]
fn toggle_favorite(id: i64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.db()?.as_mut().ok_or(LOCKED_MSG)?.toggle_favorite(id)
}

/// 更新最后使用时间
#[tauri::command]
fn update_last_used(id: i64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.db()?.as_mut().ok_or(LOCKED_MSG)?.update_last_used(id)
}

/// 获取配置
#[tauri::command]
fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig, String> {
    state.db()?.as_ref().ok_or(LOCKED_MSG)?.load_config()
}

/// 保存配置
#[tauri::command]
fn save_config(config: AppConfig, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.db()?.as_mut().ok_or(LOCKED_MSG)?.save_config(&config)
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
    let config = state.db()?.as_ref().ok_or(LOCKED_MSG)?.load_config()?;
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
    let sync_state_path = state.data_dir.join("sync_state.json");

    let report = tauri::async_runtime::spawn_blocking(move || {
        let (provider, validator) = match shared::sync::build_provider(&provider_key, cookie.clone()) {
            Ok(x) => x,
            Err(e) => return engine::SyncReport::new(engine::Outcome::Failed(e)),
        };
        let vault = VaultAccess { db_path, database };
        let sync_state = SyncStateAccess {
            path: sync_state_path,
        };
        engine::run_manual(engine::ManualInputs {
            provider: provider.as_ref(),
            validator: Some(validator.as_ref()),
            cookie: &cookie,
            remote_path: &remote_path,
            provider_key: &provider_key,
            direction: if is_download {
                engine::Direction::Download
            } else {
                engine::Direction::Upload
            },
            vault: &vault,
            sync_state: &sync_state,
        })
    })
    .await
    .map_err(|e| format!("同步任务失败: {}", e))?;

    // 同步成功则更新本地记账（不再写进密码簿）
    if report.record_sync {
        record_last_sync(&state, &provider);
    }

    // 下载成功后通知前端刷新（JSON 文件无需 reopen，下次 load() 自动读取新内容）
    if report.downloaded {
        let _ = app.emit("sync://restored", ());
        eprintln!("[sync] vault.json downloaded, emit sync://restored");
    }

    let (success, message) = match &report.outcome {
        engine::Outcome::Uploaded => (true, format!("已上传到 {}", provider_display(&provider))),
        engine::Outcome::Downloaded => (true, format!("已从 {} 下载", provider_display(&provider))),
        engine::Outcome::GenesisMigrated => (true, "已完成云端旧版密码簿迁移".to_string()),
        engine::Outcome::ConflictPending => (
            false,
            "检测到同步冲突，自动同步已暂停，请在设置中处理".to_string(),
        ),
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
            let mut config = state.db()?.as_mut().ok_or(LOCKED_MSG)?.load_config()?;
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
            state.db()?.as_mut().ok_or(LOCKED_MSG)?.save_config(&config)?;
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
    let mut config = state.db()?.as_mut().ok_or(LOCKED_MSG)?.load_config()?;
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
    state.db()?.as_mut().ok_or(LOCKED_MSG)?.save_config(&config)?;

    // 移动端：若所有同步均已停用，取消常驻通知
    #[cfg(mobile)]
    {
        let cfg = state
            .db()?
            .as_ref()
            .map(|s| s.load_config().unwrap_or_default())
            .unwrap_or_default();
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
    let import_result = {
        let g = state.db()?;
        match g.as_ref().ok_or(LOCKED_MSG)?.replace_raw(&bytes) {
            Ok(()) => {
                let imported_count = g
                    .as_ref()
                    .ok_or(LOCKED_MSG)?
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
    };
    import_result
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
    let count = state
        .db()?
        .as_ref()
        .ok_or(LOCKED_MSG)?
        .get_all()
        .map(|v| v.len())
        .unwrap_or(0);
    Ok(serde_json::json!({
        "path": state.db_path.to_string_lossy(),
        "size": size,
        "count": count,
    }))
}

// ========== 密码簿锁定 / 库密码（v2） ==========

/// 密码簿状态：format = none（待设库密码）| v1（master.key 模式）| v2（库密码模式）；
/// unlocked = 存储槽位是否有库（v2 无缓存时为 false，前端弹解锁框）
#[tauri::command]
fn get_vault_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let format = shared::storage::json_store::vault_format(&state.db_path);
    let unlocked = state.db()?.as_ref().is_some();
    Ok(serde_json::json!({ "format": format, "unlocked": unlocked }))
}

/// 解锁 v2 密码簿（新设备 / 无本机缓存时）：派生 → 验证 → 缓存 → 入槽
#[tauri::command]
fn unlock_vault(password: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let (mut store, key) =
        shared::storage::json_store::JsonStore::open_with_password(state.db_path.clone(), &password)?;
    attach_user_save_callback(&mut store, &state.data_dir);
    // 缓存失败非致命：本次会话已解锁，重启后需重输密码
    if let Err(e) = shared::storage::key_cache::cache_key(&key) {
        eprintln!("[vault] 派生钥缓存失败（不影响本次解锁）: {}", e);
    }
    *state.db()? = Some(store);
    Ok(())
}

/// 首启建库：设置库密码并创建空库（仅当无密码簿文件时允许）
#[tauri::command]
fn set_vault_password(password: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    check_password_strength(&password)?;
    if shared::storage::json_store::vault_format(&state.db_path) != "none" {
        return Err("密码簿已存在，如需更换库密码请使用修改功能".to_string());
    }
    let (mut store, key) =
        shared::storage::json_store::JsonStore::create_with_password(state.db_path.clone(), &password)?;
    attach_user_save_callback(&mut store, &state.data_dir);
    if let Err(e) = shared::storage::key_cache::cache_key(&key) {
        eprintln!("[vault] 派生钥缓存失败（不影响本次会话）: {}", e);
    }
    *state.db()? = Some(store);
    Ok(())
}

/// 修改库密码：验证旧密码 → 全库换钥 → 更新本机缓存
#[tauri::command]
fn change_vault_password(
    old_password: String,
    new_password: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    check_password_strength(&new_password)?;
    let new_key = {
        let mut g = state.db()?;
        let store = g.as_mut().ok_or(LOCKED_MSG)?;
        if !store.verify_password(&old_password) {
            return Err("当前库密码不正确".to_string());
        }
        store.re_encrypt(&new_password)?
    };
    // 旧缓存已失效：先清再写，写失败则保持清空（下次启动输密码解锁，安全侧）
    shared::storage::key_cache::clear_cache();
    if let Err(e) = shared::storage::key_cache::cache_key(&new_key) {
        eprintln!("[vault] 新派生钥缓存失败（下次启动需输密码）: {}", e);
    }
    Ok(())
}

/// v1 → v2 升级迁移：为 master.key 时代的密码簿设置库密码（全库换钥），
/// 成功后 master.key 归档为 .retired 保留
#[tauri::command]
fn migrate_vault_to_password(password: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    check_password_strength(&password)?;
    let new_key = {
        let mut g = state.db()?;
        let store = g.as_mut().ok_or(LOCKED_MSG)?;
        if shared::storage::json_store::vault_format(&state.db_path) != "v1" {
            return Err("仅 master.key 模式的密码簿需要迁移".to_string());
        }
        store.re_encrypt(&password)?
    };
    // master.key 归档（不删除，保留回退排查能力）
    let key_path = state.data_dir.join("master.key");
    if key_path.exists() {
        let retired = state.data_dir.join("master.key.retired");
        if let Err(e) = std::fs::rename(&key_path, &retired) {
            eprintln!("[vault] master.key 归档失败（不影响迁移）: {}", e);
        }
    }
    if let Err(e) = shared::storage::key_cache::cache_key(&new_key) {
        eprintln!("[vault] 派生钥缓存失败（下次启动需输密码）: {}", e);
    }
    Ok(())
}

/// 显式锁定：清空内存中的库（本机缓存保留，解锁免输密码）
#[tauri::command]
fn lock_vault(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    *state.db()? = None;
    Ok(())
}

/// 读取云同步本地记账（last_sync 等展示信息；sync_state.json 不上云）
#[tauri::command]
fn get_sync_state(
    state: State<'_, Arc<AppState>>,
) -> Result<shared::storage::sync_state::SyncState, String> {
    SyncState::load_or_create(state.data_dir.join("sync_state.json"))
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
        engine::Outcome::Uploaded
        | engine::Outcome::Downloaded
        | engine::Outcome::Skipped
        | engine::Outcome::GenesisMigrated => {
            if report.record_sync {
                record_last_sync(state, provider);
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
        engine::Outcome::ConflictPending => {
            // 冲突挂起：记账照常（远端可达、Cookie 有效），通知前端挂起待裁决
            if report.record_sync {
                record_last_sync(state, provider);
            }
            let _ = app.emit(
                "sync://conflict",
                serde_json::json!({
                    "provider": provider,
                    "message": format!("{} 检测到同步冲突，自动同步已暂停", provider_display(provider))
                }),
            );
        }
        engine::Outcome::NotLoggedIn => {}
        engine::Outcome::CookieExpired => {
            // Cookie 失效：关闭自动同步，通知前端重新扫码（锁定时无从读配置，跳过）
            if let Ok(mut g) = state.database.lock() {
                if let Some(store) = g.as_mut() {
                    let mut cfg = store.load_config().unwrap_or_default();
                    match provider {
                        "baidu" => cfg.baidu_sync_enabled = false,
                        "quark" => cfg.quark_sync_enabled = false,
                        _ => {}
                    }
                    let _ = store.save_config(&cfg);
                }
            }
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
            // 锁定/待建库时配置不可用：空配置 → 两网盘均视为未启用，本轮空转。
            // MutexGuard 不能跨 await：在同步段取完配置即刻释放
            let config = match state.database.lock() {
                Ok(g) => match g.as_ref() {
                    Some(store) => store.load_config().unwrap_or_default(),
                    None => AppConfig::default(),
                },
                Err(_) => AppConfig::default(),
            };
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
                    let sync_state = SyncStateAccess {
                        path: st.data_dir.join("sync_state.json"),
                    };
                    engine::run_cycle(engine::CycleInputs {
                        provider: provider.as_ref(),
                        validator: validator.as_ref(),
                        cookie: &cookie,
                        remote_path: &remote,
                        provider_key: name,
                        cookie_expires_at: expires_at,
                        warn_sent,
                        now_secs,
                        vault: &vault,
                        sync_state: &sync_state,
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
        get_sync_state,
        get_vault_status,
        unlock_vault,
        set_vault_password,
        change_vault_password,
        migrate_vault_to_password,
        lock_vault,
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
        get_sync_state,
        get_vault_status,
        unlock_vault,
        set_vault_password,
        change_vault_password,
        migrate_vault_to_password,
        lock_vault,
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

                    // 启动分流（§2.8）：none=待建库（前端引导设库密码）；
                    // v1=master.key 模式照常打开（待迁移）；v2=本机缓存免解锁，无缓存进入锁定态
                    let database = match shared::storage::json_store::vault_format(&db_path) {
                        "none" => None,
                        "v1" => {
                            let key_path = data_dir.join("master.key");
                            let crypto = shared::crypto::Crypto::from_key_file(&key_path)?;
                            let mut store = shared::storage::json_store::JsonStore::open(db_path.clone(), crypto)?;
                            attach_user_save_callback(&mut store, &data_dir);
                            Some(store)
                        }
                        _ => match shared::storage::key_cache::load_cached_key() {
                            Some(key) => {
                                let mut store = shared::storage::json_store::JsonStore::open_with_key(
                                    db_path.clone(),
                                    key,
                                )?;
                                attach_user_save_callback(&mut store, &data_dir);
                                eprintln!("[vault] 已用本机缓存解锁密码簿");
                                Some(store)
                            }
                            None => {
                                eprintln!("[vault] 无本机缓存，密码簿进入锁定态，等待输入库密码");
                                None
                            }
                        },
                    };

                    // 初始化同步记账（不存在则创建并生成 device_id）
                    let sync_state_path = data_dir.join("sync_state.json");
                    SyncState::load_or_create(sync_state_path)?;

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
