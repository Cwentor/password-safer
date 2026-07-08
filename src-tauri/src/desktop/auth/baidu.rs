use tauri::{Emitter, Manager};

use crate::AppState;

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

/// 打开百度网盘官方登录页（WebView 方式）：
/// 由百度官方前端管理登录流程，扫码/登录成功后由 Rust 端 probe 线程
/// 通过 cookies_for_url() 获取完整 Cookie（含 HttpOnly 的 BDUSS）并保存。
#[tauri::command]
pub async fn open_baidu_login(app: tauri::AppHandle) -> Result<(), String> {
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
