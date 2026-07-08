// ============================================
// 移动端夸克网盘 WebView 登录
// 与桌面端等价：注入初始化脚本（删除 __TAURI__、设置 Android Chrome UA）
// probe 线程轮询 cookies_for_url() 获取 HttpOnly cookie
// 登录成功后 emit `quark-login-success` 事件
// ============================================

use tauri::{Emitter, Manager};

use crate::AppState;

/// quark-login WebView 注入的初始化脚本
/// 与桌面端逻辑一致：捕获 UA/错误日志、删除 window.__TAURI__ 避免反爬检测
/// Cookie 轮询由 Rust 端 probe 线程通过 cookies_for_url() 完成（HttpOnly 不可见）
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

        console.log("[quark-login/mobile] init script executed, location=" + location.href);
        console.log("[quark-login/mobile] navigator.userAgent=" + navigator.userAgent);
        window.addEventListener('error', function(e) {
            console.log("[quark-login/mobile] window error: " + (e.message || 'unknown') +
                " at " + (e.filename || '') + ":" + (e.lineno || 0));
        });
        window.addEventListener('unhandledrejection', function(e) {
            console.log("[quark-login/mobile] unhandled rejection: " +
                (e.reason && e.reason.message ? e.reason.message : e.reason));
        });

        if (window.__quarkLoginWatch) return;
        window.__quarkLoginWatch = true;

        // 保留 __TAURI__ 引用到局部变量，并从 window 上删除以避免被夸克反爬虫检测
        // 注意：Tauri 2 的 __TAURI_INTERNALS__.invoke 是更底层的 IPC 通道，删除 __TAURI__ 不影响它
        const __TAURI__ = window.__TAURI__;
        if (__TAURI__) {
            delete window.__TAURI__;
            console.log("[quark-login/mobile] __TAURI__ removed from window (local ref kept)");
        }

        console.log("[quark-login/mobile] init done, cookie probe handled by Rust thread");
    })();
"#;

/// 打开夸克网盘官方登录页（移动端 WebView 方式）
/// Android Chrome UA 用于规避反爬检测；probe 线程获取完整 Cookie（含 HttpOnly）
#[tauri::command]
pub async fn open_quark_login(app: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[quark-login/mobile] === open_quark_login START ===");

    // 若已存在则先关闭，避免 WebviewWindowBuilder 重复创建报错
    if let Some(existing) = app.get_webview_window("quark-login") {
        eprintln!("[quark-login/mobile] closing existing window...");
        let _ = existing.close();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        eprintln!("[quark-login/mobile] existing window closed");
    }

    let url = "https://pan.quark.cn/account/login";
    eprintln!("[quark-login/mobile] building webview window for: {}", url);

    // 移动端使用 Android Chrome UA（与桌面端 Chrome 138 等价）
    let user_agent = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Mobile Safari/537.36";

    let app_clone = app.clone();
    let build_result = tokio::task::spawn_blocking(move || {
        let builder = tauri::WebviewWindowBuilder::new(
            &app_clone,
            "quark-login",
            tauri::WebviewUrl::External(url.parse().unwrap()),
        )
        .title("夸克网盘 · 登录")
        .inner_size(420.0, 720.0)
        .resizable(true)
        .min_inner_size(360.0, 600.0)
        .user_agent(user_agent)
        .initialization_script(INIT_SCRIPT)
        .on_navigation(|url| {
            eprintln!("[quark-login/mobile] navigating to: {}", url);
            true
        })
        .on_page_load(|_window, payload| {
            eprintln!("[quark-login/mobile] page load event: {:?} url={}", payload.event(), payload.url());
        });
        builder.build()
    })
    .await;

    let webview_window = match build_result {
        Ok(Ok(w)) => {
            eprintln!("[quark-login/mobile] build() OK, window created");
            w
        }
        Ok(Err(e)) => {
            eprintln!("[quark-login/mobile] build() returned Err: {:?}", e);
            return Err(format!("打开夸克登录窗口失败: {}", e));
        }
        Err(join_err) => {
            eprintln!("[quark-login/mobile] spawn_blocking join error: {:?}", join_err);
            if join_err.is_panic() {
                return Err(format!("打开夸克登录窗口时线程 panic: {}", join_err));
            }
            return Err(format!("打开夸克登录窗口时线程异常: {}", join_err));
        }
    };

    // 启动后台 probe 线程，定期通过 cookies_for_url() 获取完整 Cookie（含 HttpOnly）
    let wv = webview_window.clone();
    let app_for_thread = app.clone();
    std::thread::spawn(move || {
        for i in 1..=30 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // 窗口已关闭则停止 probe
            if app_for_thread.get_webview_window("quark-login").is_none() {
                eprintln!("[quark-login/mobile] probe stopped, window closed");
                break;
            }

            // 用 Tauri 2 官方 API 获取完整 Cookie（含 HttpOnly）
            let url: tauri::Url = match "https://pan.quark.cn".parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let cookies = match wv.cookies_for_url(url.clone()) {
                Ok(c) => {
                    eprintln!("[quark-login/mobile] probe #{}: cookies_for_url() returned {} cookies", i, c.len());
                    c
                }
                Err(e) => {
                    eprintln!("[quark-login/mobile] probe #{}: cookies_for_url() FAILED: {:?}, will retry", i, e);
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
            eprintln!("[quark-login/mobile] probe #{}: cookie_str.length={}, hasPuus={}",
                i, cookie_str.len(), has_puus);

            if has_puus && cookie_str.len() > 50 {
                // 登录成功，保存完整 Cookie（含 HttpOnly）
                eprintln!("[quark-login/mobile] 登录成功，保存完整 Cookie, length={}", cookie_str.len());

                let app_state = app_for_thread.state::<std::sync::Arc<AppState>>();
                let mut config = app_state.database.lock().unwrap().load_config().unwrap_or_default();
                config.quark_cookie = cookie_str.clone();
                config.quark_cookie_expires_at = chrono::Local::now().timestamp() + 45 * 86400;
                match app_state.database.lock().unwrap().save_config(&config) {
                    Ok(_) => eprintln!("[quark-login/mobile] config saved successfully"),
                    Err(e) => eprintln!("[quark-login/mobile] config save failed: {}", e),
                }
                // 重置过期提醒标记
                app_state.quark_cookie_warn_sent.store(false, std::sync::atomic::Ordering::SeqCst);

                // emit 事件通知前端
                let _ = app_for_thread.emit("quark-login-success", &cookie_str);
                eprintln!("[quark-login/mobile] emitted quark-login-success event");

                // 关闭登录窗口
                if let Some(w) = app_for_thread.get_webview_window("quark-login") {
                    let _ = w.close();
                    eprintln!("[quark-login/mobile] quark-login window closed");
                }
                break;
            }
        }
    });

    let _ = webview_window;
    eprintln!("[quark-login/mobile] === open_quark_login END ===");
    Ok(())
}
