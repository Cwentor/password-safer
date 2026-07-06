mod config;
mod storage;
mod crypto;
mod db;
mod models;
mod sync;

use models::*;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use sync::auth::{baidu::BaiduAuth, quark::QuarkAuth, QrAuthenticator, QrLoginSession, QrLoginStatus};
use sync::SyncProvider;
use tauri::{Emitter, Manager, State};

/// 应用全局状态
pub struct AppState {
    /// JSON 存储实例（每次操作都重新读取文件，无需 close/reopen）
    pub database: Mutex<storage::json_store::JsonStore>,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
    /// 夸克 Cookie 即将过期提醒是否已发送（避免重复提醒）
    pub quark_cookie_warn_sent: std::sync::atomic::AtomicBool,
}

/// 获取应用数据目录
pub fn get_data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("password-safer");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// 当前本地时间戳字符串
fn now_ts() -> String {
    chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// URL 解码：手动处理 %XX 十六进制序列与 + → 空格
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(b' ');
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
            } else {
                out.push(b);
                i += 1;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 夸克登录回调页面：显示“登录成功，正在关闭...”
const FULL_HTML_BODY: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>登录成功</title>
<style>
  html, body { margin: 0; padding: 0; height: 100%; }
  body {
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
    font-size: 18px;
    color: #333;
    background: #fafafa;
  }
</style>
</head>
<body>
<div>登录成功，正在关闭...</div>
</body>
</html>"#;

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
                let p = sync::baidu::BaiduProvider::new(cookie_for_task);
                if direction_for_task == "download" {
                    sync::sync_download(&p, &remote_for_task, &db_path)
                } else {
                    sync::sync_upload(&p, &db_path, &remote_for_task)
                }
            }
            "quark" => {
                // 本地 vault.json 有效性检测（仅日志，不改变行为）
                let local_valid = sync::quark::QuarkProvider::local_vault_valid(&db_path);
                println!("[sync] quark local_vault_valid = {}", local_valid);
                let p = sync::quark::QuarkProvider::new(cookie_for_task);
                if direction_for_task == "download" {
                    sync::sync_download(&p, &remote_for_task, &db_path)
                } else {
                    sync::sync_upload(&p, &db_path, &remote_for_task)
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

/// quark-login 窗口注入的初始化脚本（捕获 UA、错误、隐藏 __TAURI__、轮询 Cookie）
const INIT_SCRIPT: &str = r#"
    (function() {
        // 第一时间把 UA 和位置写到 window.__probe，供 Rust 端 eval 读取
        try {
            window.__probe = {
                ua: navigator.userAgent,
                href: location.href,
                ts: Date.now()
            };
        } catch(e) {}

        console.log("[quark-login] init script executed, location=" + location.href);
        console.log("[quark-login] navigator.userAgent=" + navigator.userAgent);
        window.addEventListener('error', function(e) {
            console.log("[quark-login] window error: " + (e.message || 'unknown') +
                " at " + (e.filename || '') + ":" + (e.lineno || 0));
        });
        window.addEventListener('unhandledrejection', function(e) {
            console.log("[quark-login] unhandled rejection: " +
                (e.reason && e.reason.message ? e.reason.message : e.reason));
        });

        if (window.__quarkLoginWatch) return;
        window.__quarkLoginWatch = true;

        // 保留 __TAURI__ 引用到局部变量，并从 window 上删除以避免被夸克反爬虫检测
        // 注意：Tauri 2 的 __TAURI_INTERNALS__.invoke 是更底层的 IPC 通道，删除 __TAURI__ 不影响它
        const __TAURI__ = window.__TAURI__;
        if (__TAURI__) {
            delete window.__TAURI__;
            console.log("[quark-login] __TAURI__ removed from window (local ref kept)");
        }

        // invoke 辅助函数：优先用 __TAURI__.core.invoke，回退到 __TAURI_INTERNALS__.invoke
        const tauriInvoke = (cmd, args) => {
            if (__TAURI__ && __TAURI__.core && __TAURI__.core.invoke) {
                return __TAURI__.core.invoke(cmd, args);
            }
            if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {
                return window.__TAURI_INTERNALS__.invoke(cmd, args);
            }
            return Promise.reject(new Error("no tauri invoke available"));
        };

        const check = async () => {
            try {
                const isLoginPage = /\/account\/login/.test(location.pathname);
                const hasPuus = /(?:^|;\s*)__puus=([^;]+)/.test(document.cookie);
                console.log("[quark-login] check: isLoginPage=" + isLoginPage +
                    " hasPuus=" + hasPuus + " path=" + location.pathname);
                // 双重判据：URL 已离开登录页 + __puus 存在，防止匿名 Cookie 误触发
                if (!isLoginPage && hasPuus) {
                    const cookie = document.cookie;
                    console.log("[quark-login] 登录成功，调用 save_quark_cookie, cookie.length=" + cookie.length);
                    try {
                        await tauriInvoke('save_quark_cookie', { cookie });
                        console.log("[quark-login] save_quark_cookie 调用成功");
                    } catch (e) {
                        console.log("[quark-login] save_quark_cookie 调用失败: " + (e.message || e));
                    }
                    // 关闭窗口（双保险：Rust 命令内也会 close）
                    try {
                        if (__TAURI__ && __TAURI__.window && __TAURI__.window.getCurrentWindow) {
                            const win = __TAURI__.window.getCurrentWindow();
                            if (win) await win.close();
                        }
                    } catch (e) {}
                    return;
                }
            } catch (e) {
                console.log("[quark-login] check 异常: " + (e.message || e));
            }
            window.setTimeout(check, 1500);
        };
        window.setTimeout(check, 2000);
        console.log("[quark-login] check loop scheduled");
    })();
"#;

/// baidu-login 窗口注入的初始化脚本（捕获 UA、错误、隐藏 __TAURI__）。
/// 不在 JS 端轮询 Cookie：百度 BDUSS 为 HttpOnly，document.cookie 拿不到，
/// 改由 Rust 端 probe 线程通过 cookies_for_url() 获取完整 Cookie。
const INIT_SCRIPT_BAIDU: &str = r#"
    (function() {
        // 第一时间把 UA 和位置写到 window.__probe，供 Rust 端 eval 读取
        try {
            window.__probe = {
                ua: navigator.userAgent,
                href: location.href,
                ts: Date.now()
            };
        } catch(e) {}

        console.log("[baidu-login] init script executed, location=" + location.href);
        console.log("[baidu-login] navigator.userAgent=" + navigator.userAgent);
        window.addEventListener('error', function(e) {
            console.log("[baidu-login] window error: " + (e.message || 'unknown') +
                " at " + (e.filename || '') + ":" + (e.lineno || 0));
        });
        window.addEventListener('unhandledrejection', function(e) {
            console.log("[baidu-login] unhandled rejection: " +
                (e.reason && e.reason.message ? e.reason.message : e.reason));
        });

        if (window.__baiduLoginWatch) return;
        window.__baiduLoginWatch = true;

        // 保留 __TAURI__ 引用到局部变量，并从 window 上删除以避免被百度反爬虫检测
        // 注意：Tauri 2 的 __TAURI_INTERNALS__.invoke 是更底层的 IPC 通道，删除 __TAURI__ 不影响它
        const __TAURI__ = window.__TAURI__;
        if (__TAURI__) {
            delete window.__TAURI__;
            console.log("[baidu-login] __TAURI__ removed from window (local ref kept)");
        }

        console.log("[baidu-login] init done, cookie probe handled by Rust thread");
    })();
"#;

/// 打开夸克网盘官方登录页（WebView 方式）：
/// 由夸克官方前端管理二维码生命周期，扫码成功后通过 Tauri IPC 将 Cookie 回传后端。
#[tauri::command]
async fn open_quark_login(app: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[quark-login] === open_quark_login START (async) ===");

    // 若已存在则先关闭，避免 WebviewWindowBuilder 重复创建报错
    if let Some(existing) = app.get_webview_window("quark-login") {
        eprintln!("[quark-login] closing existing window...");
        let _ = existing.close();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        eprintln!("[quark-login] existing window closed");
    }

    let url = "https://pan.quark.cn/account/login";
    eprintln!("[quark-login] building webview window for: {}", url);

    // 用 catch_unwind 捕获 build() 内部可能的 panic
    let app_clone = app.clone();
    let build_result = tokio::task::spawn_blocking(move || {
        eprintln!("[quark-login] entering WebviewWindowBuilder::new...");
        let builder = tauri::WebviewWindowBuilder::new(
            &app_clone,
            "quark-login",
            tauri::WebviewUrl::External(url.parse().unwrap()),
        )
        .title("夸克网盘 · 扫码登录")
        .inner_size(420.0, 640.0)
        .center()
        .resizable(true)
        .min_inner_size(360.0, 540.0)
        .decorations(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36")
        .initialization_script(INIT_SCRIPT)
        .on_navigation(|url| {
            eprintln!("[quark-login] navigating to: {}", url);
            true
        })
        .on_page_load(|_window, payload| {
            eprintln!("[quark-login] page load event: {:?} url={}", payload.event(), payload.url());
        });
        eprintln!("[quark-login] builder constructed, calling .build()...");
        builder.build()
    })
    .await;

    eprintln!("[quark-login] spawn_blocking returned");

    let webview_window = match build_result {
        Ok(Ok(w)) => {
            eprintln!("[quark-login] build() OK, window created");
            w
        }
        Ok(Err(e)) => {
            eprintln!("[quark-login] build() returned Err: {:?}", e);
            return Err(format!("打开夸克登录窗口失败: {}", e));
        }
        Err(join_err) => {
            eprintln!("[quark-login] spawn_blocking join error: {:?}", join_err);
            if join_err.is_panic() {
                return Err(format!("打开夸克登录窗口时线程 panic: {}", join_err));
            }
            return Err(format!("打开夸克登录窗口时线程异常: {}", join_err));
        }
    };

    // 始终打开 DevTools（不限于 debug 模式），便于调试空白问题
    eprintln!("[quark-login] opening devtools...");
    webview_window.open_devtools();
    eprintln!("[quark-login] devtools opened");

    // 启动后台线程，定期检测登录状态
    // 登录成功后用 Tauri 2 官方 cookies_for_url() 获取完整 Cookie（含 HttpOnly）
    // 这是修复 31001 [guest] 错误的关键：document.cookie 拿不到 HttpOnly Cookie
    let wv = webview_window.clone();
    let app_for_thread = app.clone();
    std::thread::spawn(move || {
        for i in 1..=30 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // 窗口已关闭则停止 probe
            if app_for_thread.get_webview_window("quark-login").is_none() {
                eprintln!("[quark-login] probe stopped, window closed");
                break;
            }

            // 用 eval 输出调试信息（eval 不返回值，用 console.log 输出）
            let probe_js = format!(
                r#"(function(){{
                    try {{
                        var path = location.pathname || "";
                        var isLoginPage = /\/account\/login/.test(path);
                        var hasPuus = /(?:^|;\s*)__puus=/.test(document.cookie);
                        console.log("[quark-login] probe #{}: isLoginPage=" + isLoginPage +
                            " hasPuus=" + hasPuus + " path=" + path);
                    }} catch(e) {{}}
                }})();"#,
                i
            );
            if wv.eval(&probe_js).is_err() {
                eprintln!("[quark-login] probe #{} eval FAILED, stopping", i);
                break;
            }

            // 短暂等待 eval 执行
            std::thread::sleep(std::time::Duration::from_millis(300));

            // 再次检查窗口状态
            if app_for_thread.get_webview_window("quark-login").is_none() {
                eprintln!("[quark-login] probe stopped after eval, window closed");
                break;
            }

            // 用 Tauri 2 官方 API 获取完整 Cookie（含 HttpOnly）
            // probe 是独立线程，不在 Tauri 主消息循环中，可安全调用
            let url: tauri::Url = match "https://pan.quark.cn".parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let cookies = match wv.cookies_for_url(url.clone()) {
                Ok(c) => {
                    eprintln!("[quark-login] probe #{}: cookies_for_url() returned {} cookies", i, c.len());
                    c
                }
                Err(e) => {
                    eprintln!("[quark-login] probe #{}: cookies_for_url() FAILED: {:?}, will retry", i, e);
                    continue;
                }
            };

            // 拼接 Cookie 字符串：key=value; key=value
            let cookie_str: String = cookies
                .iter()
                .map(|c| format!("{}={}", c.name(), c.value()))
                .collect::<Vec<_>>()
                .join("; ");

            // 检查是否包含 __puus（登录成功的标志）
            let has_puus = cookie_str.contains("__puus=");
            // 检查 URL 是否已离开登录页（通过 eval 读取的 path 无法直接拿到，用 cookie 判断）
            eprintln!("[quark-login] probe #{}: cookie_str.length={}, hasPuus={}",
                i, cookie_str.len(), has_puus);

            if has_puus && cookie_str.len() > 50 {
                // 登录成功，保存完整 Cookie（含 HttpOnly）
                eprintln!("[quark-login] 登录成功，保存完整 Cookie（含 HttpOnly）, length={}", cookie_str.len());

                // 复用 save_quark_cookie 的保存逻辑
                let app_state = app_for_thread.state::<std::sync::Arc<AppState>>();
                let mut config = app_state.database.lock().unwrap().load_config().unwrap_or_default();
                config.quark_cookie = cookie_str.clone();
                config.quark_cookie_expires_at = chrono::Local::now().timestamp() + 45 * 86400;
                match app_state.database.lock().unwrap().save_config(&config) {
                    Ok(_) => eprintln!("[quark-login] config saved successfully (cookies_for_url path)"),
                    Err(e) => eprintln!("[quark-login] config save failed: {}", e),
                }
                // 重置过期提醒标记
                app_state.quark_cookie_warn_sent.store(false, std::sync::atomic::Ordering::SeqCst);

                // emit 事件通知前端
                let _ = app_for_thread.emit("quark-login-success", &cookie_str);
                eprintln!("[quark-login] emitted quark-login-success event");

                // 关闭登录窗口
                if let Some(w) = app_for_thread.get_webview_window("quark-login") {
                    let _ = w.close();
                    eprintln!("[quark-login] quark-login window closed");
                }
                break;
            }
        }
    });

    let _ = webview_window;
    eprintln!("[quark-login] === open_quark_login END ===");
    Ok(())
}

/// 保存夸克网盘登录 Cookie（由 quark-login 窗口的 init script 通过 invoke 调用）：
/// 校验 Cookie → 写入配置 → emit 事件通知主窗口 → 关闭登录窗口。
#[tauri::command]
fn save_quark_cookie(
    cookie: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    eprintln!("[quark-login] save_quark_cookie called, cookie.length={}", cookie.len());
    if !cookie.contains("__puus=") {
        eprintln!("[quark-login] save_quark_cookie: cookie 缺少 __puus");
        return Err("Cookie 中缺少 __puus，登录未成功".to_string());
    }
    let mut config = state.database.lock().unwrap().load_config()?;
    config.quark_cookie = cookie;
    config.quark_cookie_expires_at = chrono::Local::now().timestamp() + 45 * 86400;
    state.database.lock().unwrap().save_config(&config)?;
    eprintln!("[quark-login] save_quark_cookie: config saved, expires_at={}", config.quark_cookie_expires_at);
    // 重新登录，重置过期提醒标记
    state.quark_cookie_warn_sent.store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = app.emit("quark-login-success", &config.quark_cookie);
    eprintln!("[quark-login] save_quark_cookie: emitted quark-login-success event");
    if let Some(w) = app.get_webview_window("quark-login") {
        let _ = w.close();
        eprintln!("[quark-login] save_quark_cookie: quark-login window closed");
    }
    Ok(())
}

/// 打开百度网盘官方登录页（WebView 方式）：
/// 由百度官方前端管理登录流程，扫码/登录成功后由 Rust 端 probe 线程
/// 通过 cookies_for_url() 获取完整 Cookie（含 HttpOnly 的 BDUSS）并保存。
#[tauri::command]
async fn open_baidu_login(app: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[baidu-login] === open_baidu_login START (async) ===");

    // 若已存在则先关闭，避免 WebviewWindowBuilder 重复创建报错
    if let Some(existing) = app.get_webview_window("baidu-login") {
        eprintln!("[baidu-login] closing existing window...");
        let _ = existing.close();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        eprintln!("[baidu-login] existing window closed");
    }

    let url = "https://pan.baidu.com/login";
    eprintln!("[baidu-login] building webview window for: {}", url);

    // 用 catch_unwind 捕获 build() 内部可能的 panic
    let app_clone = app.clone();
    let build_result = tokio::task::spawn_blocking(move || {
        eprintln!("[baidu-login] entering WebviewWindowBuilder::new...");
        let builder = tauri::WebviewWindowBuilder::new(
            &app_clone,
            "baidu-login",
            tauri::WebviewUrl::External(url.parse().unwrap()),
        )
        .title("百度网盘 · 扫码登录")
        .inner_size(420.0, 640.0)
        .center()
        .resizable(true)
        .min_inner_size(360.0, 540.0)
        .decorations(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36")
        .initialization_script(INIT_SCRIPT_BAIDU)
        .on_navigation(|url| {
            eprintln!("[baidu-login] navigating to: {}", url);
            true
        })
        .on_page_load(|_window, payload| {
            eprintln!("[baidu-login] page load event: {:?} url={}", payload.event(), payload.url());
        });
        eprintln!("[baidu-login] builder constructed, calling .build()...");
        builder.build()
    })
    .await;

    eprintln!("[baidu-login] spawn_blocking returned");

    let webview_window = match build_result {
        Ok(Ok(w)) => {
            eprintln!("[baidu-login] build() OK, window created");
            w
        }
        Ok(Err(e)) => {
            eprintln!("[baidu-login] build() returned Err: {:?}", e);
            return Err(format!("打开百度登录窗口失败: {}", e));
        }
        Err(join_err) => {
            eprintln!("[baidu-login] spawn_blocking join error: {:?}", join_err);
            if join_err.is_panic() {
                return Err(format!("打开百度登录窗口时线程 panic: {}", join_err));
            }
            return Err(format!("打开百度登录窗口时线程异常: {}", join_err));
        }
    };

    // 始终打开 DevTools（不限于 debug 模式），便于调试空白问题
    eprintln!("[baidu-login] opening devtools...");
    webview_window.open_devtools();
    eprintln!("[baidu-login] devtools opened");

    // 启动后台线程，定期检测登录状态
    // 登录成功后用 Tauri 2 官方 cookies_for_url() 获取完整 Cookie（含 HttpOnly）
    // 百度 BDUSS 为 HttpOnly，document.cookie 拿不到，必须用此 API
    let wv = webview_window.clone();
    let app_for_thread = app.clone();
    std::thread::spawn(move || {
        for i in 1..=30 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // 窗口已关闭则停止 probe
            if app_for_thread.get_webview_window("baidu-login").is_none() {
                eprintln!("[baidu-login] probe stopped, window closed");
                break;
            }

            // 用 Tauri 2 官方 API 获取完整 Cookie（含 HttpOnly）
            // probe 是独立线程，不在 Tauri 主消息循环中，可安全调用
            let url: tauri::Url = match "https://pan.baidu.com".parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let cookies = match wv.cookies_for_url(url.clone()) {
                Ok(c) => {
                    eprintln!("[baidu-login] probe #{}: cookies_for_url() returned {} cookies", i, c.len());
                    c
                }
                Err(e) => {
                    eprintln!("[baidu-login] probe #{}: cookies_for_url() FAILED: {:?}, will retry", i, e);
                    continue;
                }
            };

            // 拼接 Cookie 字符串：key=value; key=value
            let cookie_str: String = cookies
                .iter()
                .map(|c| format!("{}={}", c.name(), c.value()))
                .collect::<Vec<_>>()
                .join("; ");

            // 检查是否包含 BDUSS（登录成功的标志）
            let has_bduss = cookie_str.contains("BDUSS=");
            eprintln!("[baidu-login] probe #{}: cookie_str.length={}, hasBduss={}",
                i, cookie_str.len(), has_bduss);

            if has_bduss && cookie_str.len() > 50 {
                // 登录成功，保存完整 Cookie（含 HttpOnly）
                eprintln!("[baidu-login] 登录成功，保存完整 Cookie（含 HttpOnly）, length={}", cookie_str.len());

                // 保存到配置
                let app_state = app_for_thread.state::<std::sync::Arc<AppState>>();
                let mut config = app_state.database.lock().unwrap().load_config().unwrap_or_default();
                config.baidu_cookie = cookie_str.clone();
                config.baidu_cookie_expires_at = chrono::Local::now().timestamp() + 60 * 86400;
                match app_state.database.lock().unwrap().save_config(&config) {
                    Ok(_) => eprintln!("[baidu-login] config saved successfully (cookies_for_url path)"),
                    Err(e) => eprintln!("[baidu-login] config save failed: {}", e),
                }
                // 注意：AppState 上没有 baidu_cookie_warn_sent 字段，无需重置

                // emit 事件通知前端
                let _ = app_for_thread.emit("baidu-login-success", &cookie_str);
                eprintln!("[baidu-login] emitted baidu-login-success event");

                // 关闭登录窗口
                if let Some(w) = app_for_thread.get_webview_window("baidu-login") {
                    let _ = w.close();
                    eprintln!("[baidu-login] baidu-login window closed");
                }
                break;
            }
        }
    });

    let _ = webview_window;
    eprintln!("[baidu-login] === open_baidu_login END ===");
    Ok(())
}

/// 退出登录：清空对应提供者的 Cookie 与失效时间，并关闭自动同步
#[tauri::command]
fn logout(provider: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
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
    state.database.lock().unwrap().save_config(&config)
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
    Ok(crypto::generate_password(len))
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
        }
        Err(msg) => {
            let _ = app.emit(
                "sync://error",
                serde_json::json!({ "provider": provider, "message": msg }),
            );
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
                        let provider = sync::baidu::BaiduProvider::new(cookie);
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
                        let provider = sync::quark::QuarkProvider::new(cookie);

                        // 1. 检测本地 vault.json 有效性
                        let local_valid = sync::quark::QuarkProvider::local_vault_valid(&db_path);

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

/// 数据迁移：如果存在旧 vault.db（SQLite），读取数据并迁移到 vault.json
/// 迁移完成后将 vault.db 重命名为 vault.db.old（避免重复迁移）
/// 仅在 vault.json 不存在时执行迁移
fn migrate_from_sqlite_if_needed(data_dir: &std::path::Path, crypto: &crypto::Crypto) {
    let json_path = data_dir.join("vault.json");
    let db_path = data_dir.join("vault.db");
    let old_db_path = data_dir.join("vault.db.old");

    // vault.json 已存在，无需迁移
    if json_path.exists() {
        return;
    }
    // 旧 vault.db 不存在，无需迁移
    if !db_path.exists() {
        return;
    }
    // vault.db.old 已存在说明之前迁移过，不再重复迁移
    if old_db_path.exists() {
        return;
    }

    eprintln!("[migrate] 检测到旧 vault.db，开始迁移到 vault.json");

    // 打开旧 SQLite 数据库
    let database = match db::Database::open(&db_path, crypto.clone_key()) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("[migrate] 打开旧 vault.db 失败: {}，跳过迁移", e);
            return;
        }
    };

    // 读取所有密码记录
    let passwords = match database.get_all() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[migrate] 读取旧密码数据失败: {}，跳过迁移", e);
            return;
        }
    };

    // 读取旧配置（使用 ConfigManager）
    let config_manager = match config::ConfigManager::from_db_path(&db_path) {
        Ok(cm) => cm,
        Err(e) => {
            eprintln!("[migrate] 打开旧配置失败: {}，跳过迁移", e);
            return;
        }
    };
    let config = config_manager.load();

    // 构造 StoreData
    let mut store_data = storage::json_store::StoreData::default();
    store_data.config = config;
    store_data.next_id = passwords.iter().map(|p| p.id).max().unwrap_or(0) + 1;

    for dto in passwords {
        // 重新加密密码字段（用同一密钥，加密结果不同但解密一致）
        let password_encrypted = match crypto.encrypt(&dto.password) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[migrate] 加密密码字段失败: {}，跳过该记录", e);
                continue;
            }
        };
        let record = storage::json_store::PasswordRecord {
            id: dto.id,
            name: dto.name,
            icon: dto.icon,
            url: dto.url,
            username: dto.username,
            password_encrypted,
            tags: dto.tags,
            notes: dto.notes,
            favorite: dto.favorite,
            strength: dto.strength,
            created: dto.created,
            last_used: dto.last_used,
        };
        store_data.passwords.push(record);
    }

    eprintln!("[migrate] 迁移 {} 条密码记录", store_data.passwords.len());

    // 创建 JsonStore 并保存数据
    let json_store = match storage::json_store::JsonStore::open(json_path.clone(), crypto.clone_key()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[migrate] 创建 vault.json 失败: {}，迁移中止", e);
            return;
        }
    };
    if let Err(e) = json_store.save(&store_data) {
        eprintln!("[migrate] 保存 vault.json 失败: {}，迁移中止", e);
        return;
    }

    // 迁移成功，将旧 vault.db 重命名为 vault.db.old
    if let Err(e) = std::fs::rename(&db_path, &old_db_path) {
        eprintln!("[migrate] 重命名 vault.db 为 vault.db.old 失败: {}（数据已迁移但旧文件未重命名）", e);
    } else {
        eprintln!("[migrate] 旧 vault.db 已重命名为 vault.db.old");
    }

    // 清理 WAL/SHM 残留文件
    let _ = std::fs::remove_file(data_dir.join("vault.db-wal"));
    let _ = std::fs::remove_file(data_dir.join("vault.db-shm"));

    eprintln!("[migrate] 迁移完成");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 初始化数据目录
    let data_dir = get_data_dir();
    let db_path = data_dir.join("vault.json");
    let key_path = data_dir.join("master.key");

    // 初始化加密器
    let crypto = crypto::Crypto::from_key_file(&key_path).expect("无法初始化加密器");

    // 数据迁移：如果存在旧 vault.db（SQLite），迁移数据到 vault.json
    migrate_from_sqlite_if_needed(&data_dir, &crypto);

    // 初始化 JSON 存储（双重加密：密码字段加密 + 文件整体加密）
    let database = storage::json_store::JsonStore::open(db_path.clone(), crypto)
        .expect("无法打开 vault.json");

    let state = Arc::new(AppState {
        database: Mutex::new(database),
        db_path,
        data_dir,
        quark_cookie_warn_sent: std::sync::atomic::AtomicBool::new(false),
    });

    // 启动定时同步
    let scheduler_state = state.clone();
    tauri::Builder::default()
        .manage(state.clone())
        .register_uri_scheme_protocol("quark-cb", move |app, request| -> tauri::http::Response<Vec<u8>> {
            // 请求形如：http://quark-cb.localhost/success?c=ENCODED_COOKIE
            let uri = request.uri().to_string();
            eprintln!("[quark-login] quark-cb received request: {}", uri.chars().take(100).collect::<String>());
            let cookie = uri
                .find("c=")
                .and_then(|pos| {
                    let start = pos + 2;
                    let rest = &uri[start..];
                    let end = rest.find('&').unwrap_or(rest.len());
                    Some(url_decode(&rest[..end]))
                })
                .unwrap_or_default();
            eprintln!("[quark-login] quark-cb decoded cookie.length={}, hasPuus={}", cookie.len(), cookie.contains("__puus="));
            let handle = app.app_handle();

            // 直接在 Rust 端保存 Cookie 到配置（不依赖前端 invoke）
            let app_state = handle.state::<std::sync::Arc<AppState>>();
            if !cookie.contains("__puus=") {
                eprintln!("[quark-login] quark-cb: cookie 缺少 __puus，不保存");
            } else {
                let mut config = app_state.database.lock().unwrap().load_config().unwrap_or_default();
                config.quark_cookie = cookie.clone();
                config.quark_cookie_expires_at = chrono::Local::now().timestamp() + 45 * 86400;
                match app_state.database.lock().unwrap().save_config(&config) {
                    Ok(_) => eprintln!("[quark-login] quark-cb: config saved successfully"),
                    Err(e) => eprintln!("[quark-login] quark-cb: config save failed: {}", e),
                }
                // 重置过期提醒标记
                app_state.quark_cookie_warn_sent.store(false, std::sync::atomic::Ordering::SeqCst);
            }

            let _ = handle.emit("quark-login-success", &cookie);
            eprintln!("[quark-login] quark-cb: emitted quark-login-success event");
            if let Some(w) = handle.get_webview_window("quark-login") {
                let _ = w.close();
                eprintln!("[quark-login] quark-cb: quark-login window closed");
            }
            tauri::http::Response::builder()
                .status(200)
                .header("Content-Type", "text/html; charset=utf-8")
                .body(FULL_HTML_BODY.as_bytes().to_vec())
                .unwrap()
        })
        .invoke_handler(tauri::generate_handler![
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
            open_quark_login,
            save_quark_cookie,
            open_baidu_login,
            logout,
            import_db,
            export_db,
            generate_password,
            get_db_info,
        ])
        .setup(move |app| {
            // 系统托盘菜单
            let show_item = tauri::menu::MenuItem::with_id(app, "tray_show", "显示主窗口", true, None::<&str>)?;
            let hide_item = tauri::menu::MenuItem::with_id(app, "tray_hide", "隐藏主窗口", true, None::<&str>)?;
            let quit_item = tauri::menu::MenuItem::with_id(app, "tray_quit", "退出", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app, &[&show_item, &hide_item, &quit_item])?;

            // 系统托盘图标
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

            start_sync_scheduler(scheduler_state, app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
