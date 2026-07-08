// ============================================
// 移动端百度网盘 WebView 登录
// 与桌面端等价：注入初始化脚本（删除 __TAURI__、设置 Android Chrome UA）
// probe 线程轮询 cookies_for_url() 获取 HttpOnly BDUSS
// 登录成功后 emit `baidu-login-success` 事件
// ============================================

use tauri::{Emitter, Manager};

use crate::AppState;

/// baidu-login WebView 注入的初始化脚本
/// 与桌面端逻辑一致：捕获 UA/错误日志、删除 window.__TAURI__ 避免反爬检测
/// Cookie 轮询由 Rust 端 probe 线程通过 cookies_for_url() 完成（BDUSS 为 HttpOnly）
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

        console.log("[baidu-login/mobile] init script executed, location=" + location.href);
        console.log("[baidu-login/mobile] navigator.userAgent=" + navigator.userAgent);
        window.addEventListener('error', function(e) {
            console.log("[baidu-login/mobile] window error: " + (e.message || 'unknown') +
                " at " + (e.filename || '') + ":" + (e.lineno || 0));
        });
        window.addEventListener('unhandledrejection', function(e) {
            console.log("[baidu-login/mobile] unhandled rejection: " +
                (e.reason && e.reason.message ? e.reason.message : e.reason));
        });

        if (window.__baiduLoginWatch) return;
        window.__baiduLoginWatch = true;

        // 保留 __TAURI__ 引用到局部变量，并从 window 上删除以避免被百度反爬虫检测
        const __TAURI__ = window.__TAURI__;
        if (__TAURI__) {
            delete window.__TAURI__;
            console.log("[baidu-login/mobile] __TAURI__ removed from window (local ref kept)");
        }

        console.log("[baidu-login/mobile] init done, cookie probe handled by Rust thread");
    })();
"#;

/// 打开百度网盘官方登录页（移动端 WebView 方式）
/// Android Chrome UA 用于规避反爬检测；probe 线程获取完整 Cookie（含 HttpOnly 的 BDUSS）
#[tauri::command]
pub async fn open_baidu_login(app: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[baidu-login/mobile] === open_baidu_login START ===");

    // 若已存在则先关闭
    if let Some(existing) = app.get_webview_window("baidu-login") {
        eprintln!("[baidu-login/mobile] closing existing window...");
        let _ = existing.close();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        eprintln!("[baidu-login/mobile] existing window closed");
    }

    let url = "https://pan.baidu.com/login";
    eprintln!("[baidu-login/mobile] building webview window for: {}", url);

    // 移动端使用 Android Chrome UA
    let user_agent = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Mobile Safari/537.36";

    let app_clone = app.clone();
    let build_result = tokio::task::spawn_blocking(move || {
        let builder = tauri::WebviewWindowBuilder::new(
            &app_clone,
            "baidu-login",
            tauri::WebviewUrl::External(url.parse().unwrap()),
        )
        .title("百度网盘 · 登录")
        .inner_size(420.0, 720.0)
        .resizable(true)
        .min_inner_size(360.0, 600.0)
        .user_agent(user_agent)
        .initialization_script(INIT_SCRIPT)
        .on_navigation(|url| {
            eprintln!("[baidu-login/mobile] navigating to: {}", url);
            true
        })
        .on_page_load(|_window, payload| {
            eprintln!("[baidu-login/mobile] page load event: {:?} url={}", payload.event(), payload.url());
        });
        builder.build()
    })
    .await;

    let webview_window = match build_result {
        Ok(Ok(w)) => {
            eprintln!("[baidu-login/mobile] build() OK, window created");
            w
        }
        Ok(Err(e)) => {
            eprintln!("[baidu-login/mobile] build() returned Err: {:?}", e);
            return Err(format!("打开百度登录窗口失败: {}", e));
        }
        Err(join_err) => {
            eprintln!("[baidu-login/mobile] spawn_blocking join error: {:?}", join_err);
            if join_err.is_panic() {
                return Err(format!("打开百度登录窗口时线程 panic: {}", join_err));
            }
            return Err(format!("打开百度登录窗口时线程异常: {}", join_err));
        }
    };

    // 启动后台 probe 线程，定期通过 cookies_for_url() 获取完整 Cookie（含 HttpOnly BDUSS）
    let wv = webview_window.clone();
    let app_for_thread = app.clone();
    std::thread::spawn(move || {
        for i in 1..=30 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if app_for_thread.get_webview_window("baidu-login").is_none() {
                eprintln!("[baidu-login/mobile] probe stopped, window closed");
                break;
            }

            let url: tauri::Url = match "https://pan.baidu.com".parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let cookies = match wv.cookies_for_url(url.clone()) {
                Ok(c) => {
                    eprintln!("[baidu-login/mobile] probe #{}: cookies_for_url() returned {} cookies", i, c.len());
                    c
                }
                Err(e) => {
                    eprintln!("[baidu-login/mobile] probe #{}: cookies_for_url() FAILED: {:?}, will retry", i, e);
                    continue;
                }
            };

            let cookie_str: String = cookies
                .iter()
                .map(|c| format!("{}={}", c.name(), c.value()))
                .collect::<Vec<_>>()
                .join("; ");

            // 检查是否包含 BDUSS（登录成功的标志）
            let has_bduss = cookie_str.contains("BDUSS=");
            eprintln!("[baidu-login/mobile] probe #{}: cookie_str.length={}, hasBduss={}",
                i, cookie_str.len(), has_bduss);

            if has_bduss && cookie_str.len() > 50 {
                eprintln!("[baidu-login/mobile] 登录成功，保存完整 Cookie, length={}", cookie_str.len());

                let app_state = app_for_thread.state::<std::sync::Arc<AppState>>();
                let mut config = app_state.database.lock().unwrap().load_config().unwrap_or_default();
                config.baidu_cookie = cookie_str.clone();
                config.baidu_cookie_expires_at = chrono::Local::now().timestamp() + 60 * 86400;
                match app_state.database.lock().unwrap().save_config(&config) {
                    Ok(_) => eprintln!("[baidu-login/mobile] config saved successfully"),
                    Err(e) => eprintln!("[baidu-login/mobile] config save failed: {}", e),
                }

                let _ = app_for_thread.emit("baidu-login-success", &cookie_str);
                eprintln!("[baidu-login/mobile] emitted baidu-login-success event");

                if let Some(w) = app_for_thread.get_webview_window("baidu-login") {
                    let _ = w.close();
                    eprintln!("[baidu-login/mobile] baidu-login window closed");
                }
                break;
            }
        }
    });

    let _ = webview_window;
    eprintln!("[baidu-login/mobile] === open_baidu_login END ===");
    Ok(())
}
