use std::sync::Arc;
use tauri::{Emitter, Manager, Runtime, State};

use crate::AppState;

/// 夸克登录回调页面：显示"登录成功，正在关闭..."
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

/// 打开夸克网盘官方登录页（WebView 方式）：
/// 由夸克官方前端管理二维码生命周期，扫码成功后通过 Tauri IPC 将 Cookie 回传后端。
#[tauri::command]
pub async fn open_quark_login(app: tauri::AppHandle) -> Result<(), String> {
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
pub fn save_quark_cookie(
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

/// 注册 quark-cb URI scheme 协议
/// 处理夸克登录回调：从 URL 提取 Cookie 并保存到配置
pub fn register_uri_scheme<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_uri_scheme_protocol("quark-cb", move |app, request| -> tauri::http::Response<Vec<u8>> {
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
}
