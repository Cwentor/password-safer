//! 扫码登录（WebView 方式）：一份实现 + 四份配置。
//!
//! 统一「开窗 → 注入反检测脚本 → 轮询 Cookie → 校验 → 写配置 → 发事件 → 关窗」
//! 这条链，桌面与移动的真实差异（UA、窗口尺寸）由模块内 cfg 常量吸收。
//! 每个网盘只描述一份 [`WebviewLoginSpec`]；保存 Cookie 的唯一入口是
//! [`save_login_cookie`]，探询线程、手动保存命令、URI 回调共用它。
//! 无头二维码协议栈（`shared::sync::auth`）是另一种登录方式，不在此列。

use crate::AppState;
use std::sync::Arc;
use tauri::{Emitter, Manager, Runtime};

/// 登录窗口参数：每个网盘一份，四个平台（桌面/移动 × 夸克/百度）共用
pub struct WebviewLoginSpec {
    pub provider: &'static str,
    /// 窗口标签，如 "quark-login"
    pub window_label: &'static str,
    /// 登录页地址
    pub login_url: &'static str,
    /// Cookie 中出现即认为登录成功的标记（如 "__puus=" / "BDUSS="）
    pub cookie_marker: &'static str,
    /// 读取 Cookie 的域名，如 "https://pan.quark.cn"
    pub cookie_domain: &'static str,
    /// Cookie 预估有效天数
    pub expiry_days: i64,
    pub window_title: &'static str,
    /// 登录成功后发给前端的事件名
    pub success_event: &'static str,
    /// 日志前缀
    pub log_tag: &'static str,
    /// JS 轮询调用的保存命令名；None 表示不在 JS 端轮询
    /// （百度 BDUSS 为 HttpOnly，document.cookie 拿不到，只能靠 Rust 探询线程）
    pub js_poll_command: Option<&'static str>,
    /// JS 轮询判定"已离开登录页"的正则字面量（仅 js_poll_command 为 Some 时使用）
    pub login_path_regex: &'static str,
}

/// 夸克网盘扫码登录配置
pub fn quark_spec() -> WebviewLoginSpec {
    WebviewLoginSpec {
        provider: "quark",
        window_label: "quark-login",
        login_url: "https://pan.quark.cn/account/login",
        cookie_marker: "__puus=",
        cookie_domain: "https://pan.quark.cn",
        expiry_days: 45,
        window_title: "夸克网盘 · 扫码登录",
        success_event: "quark-login-success",
        log_tag: "quark-login",
        js_poll_command: Some("save_quark_cookie"),
        login_path_regex: r"/\/account\/login/",
    }
}

/// 百度网盘扫码登录配置
pub fn baidu_spec() -> WebviewLoginSpec {
    WebviewLoginSpec {
        provider: "baidu",
        window_label: "baidu-login",
        login_url: "https://pan.baidu.com/login",
        cookie_marker: "BDUSS=",
        cookie_domain: "https://pan.baidu.com",
        expiry_days: 60,
        window_title: "百度网盘 · 扫码登录",
        success_event: "baidu-login-success",
        log_tag: "baidu-login",
        js_poll_command: None,
        login_path_regex: "",
    }
}

// 平台差异：UA 与窗口尺寸（桌面带 center/decorations，移动不带）
#[cfg(desktop)]
const PLATFORM_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";
#[cfg(mobile)]
const PLATFORM_UA: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Mobile Safari/537.36";

#[cfg(desktop)]
const INNER_SIZE: (f64, f64) = (420.0, 640.0);
#[cfg(desktop)]
const MIN_SIZE: (f64, f64) = (360.0, 540.0);
#[cfg(mobile)]
const INNER_SIZE: (f64, f64) = (420.0, 720.0);
#[cfg(mobile)]
const MIN_SIZE: (f64, f64) = (360.0, 600.0);

/// init 脚本模板：捕获 UA/错误日志、隐藏 __TAURI__；__POLL_SECTION__ 处
/// 按 spec 注入 JS 轮询段（仅桌面夸克）或"探询交给 Rust 线程"的收尾日志
const INIT_SCRIPT_TEMPLATE: &str = r#"
    (function() {
        // 第一时间把 UA 和位置写到 window.__probe，供 Rust 端 eval 读取
        try {
            window.__probe = {
                ua: navigator.userAgent,
                href: location.href,
                ts: Date.now()
            };
        } catch(e) {}

        console.log("[__TAG__] init script executed, location=" + location.href);
        console.log("[__TAG__] navigator.userAgent=" + navigator.userAgent);
        window.addEventListener('error', function(e) {
            console.log("[__TAG__] window error: " + (e.message || 'unknown') +
                " at " + (e.filename || '') + ":" + (e.lineno || 0));
        });
        window.addEventListener('unhandledrejection', function(e) {
            console.log("[__TAG__] unhandled rejection: " +
                (e.reason && e.reason.message ? e.reason.message : e.reason));
        });

        if (window.__GUARD__) return;
        window.__GUARD__ = true;

        // 保留 __TAURI__ 引用到局部变量，并从 window 上删除以避免被网盘反爬虫检测
        // 注意：Tauri 2 的 __TAURI_INTERNALS__.invoke 是更底层的 IPC 通道，删除 __TAURI__ 不影响它
        const __TAURI__ = window.__TAURI__;
        if (__TAURI__) {
            delete window.__TAURI__;
            console.log("[__TAG__] __TAURI__ removed from window (local ref kept)");
        }
__POLL_SECTION__
    })();
"#;

/// JS 轮询段：双重判据（离开登录页 + 标记 Cookie 出现）后调用保存命令并自关窗口
const POLL_SECTION: &str = r#"
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
                const isLoginPage = __PATH_RE__.test(location.pathname);
                const hasMarker = __MARKER_RE__.test(document.cookie);
                console.log("[__TAG__] check: isLoginPage=" + isLoginPage +
                    " hasMarker=" + hasMarker + " path=" + location.pathname);
                // 双重判据：URL 已离开登录页 + 标记 Cookie 存在，防止匿名 Cookie 误触发
                if (!isLoginPage && hasMarker) {
                    const cookie = document.cookie;
                    console.log("[__TAG__] 登录成功，调用 __CMD__, cookie.length=" + cookie.length);
                    try {
                        await tauriInvoke('__CMD__', { cookie });
                        console.log("[__TAG__] __CMD__ 调用成功");
                    } catch (e) {
                        console.log("[__TAG__] __CMD__ 调用失败: " + (e.message || e));
                    }
                    // 关闭窗口（双保险：Rust 保存入口内也会 close）
                    try {
                        if (__TAURI__ && __TAURI__.window && __TAURI__.window.getCurrentWindow) {
                            const win = __TAURI__.window.getCurrentWindow();
                            if (win) await win.close();
                        }
                    } catch (e) {}
                    return;
                }
            } catch (e) {
                console.log("[__TAG__] check 异常: " + (e.message || e));
            }
            window.setTimeout(check, 1500);
        };
        window.setTimeout(check, 2000);
        console.log("[__TAG__] check loop scheduled");
"#;

/// 无 JS 轮询段的收尾：Cookie（含 HttpOnly）由 Rust 探询线程获取
const NO_POLL_SECTION: &str = r#"
        console.log("[__TAG__] init done, cookie probe handled by Rust thread");
"#;

/// 按网盘配置生成注入 WebView 的初始化脚本
fn init_script(spec: &WebviewLoginSpec) -> String {
    let (poll_section, cmd) = match spec.js_poll_command {
        Some(cmd) => (POLL_SECTION, cmd),
        None => (NO_POLL_SECTION, ""),
    };
    let marker_re = format!(r"/(?:^|;\s*){}([^;]+)/", spec.cookie_marker);
    INIT_SCRIPT_TEMPLATE
        .replace("__POLL_SECTION__", poll_section)
        .replace("__CMD__", cmd)
        .replace("__PATH_RE__", spec.login_path_regex)
        .replace("__MARKER_RE__", &marker_re)
        .replace("__GUARD__", &format!("__{}LoginWatch", spec.provider))
        .replace("__TAG__", spec.log_tag)
}

/// 打开网盘扫码登录窗口并启动 Cookie 探询线程
pub async fn open_login_window(app: tauri::AppHandle, spec: WebviewLoginSpec) -> Result<(), String> {
    let tag = spec.log_tag;
    eprintln!("[{}] === open_login_window START ===", tag);

    // 若已存在则先关闭，避免 WebviewWindowBuilder 重复创建报错
    if let Some(existing) = app.get_webview_window(spec.window_label) {
        eprintln!("[{}] closing existing window...", tag);
        let _ = existing.close();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        eprintln!("[{}] existing window closed", tag);
    }

    eprintln!("[{}] building webview window for: {}", tag, spec.login_url);

    let label = spec.window_label.to_string();
    let title = spec.window_title.to_string();
    let url = spec.login_url.to_string();
    let script = init_script(&spec);
    let tag_nav = tag.to_string();
    let tag_page = tag.to_string();

    // build() 可能 panic，放入 spawn_blocking 并区分 join 错误
    let app_for_build = app.clone();
    let build_result = tokio::task::spawn_blocking(move || {
        let builder = tauri::WebviewWindowBuilder::new(
            &app_for_build,
            label.as_str(),
            tauri::WebviewUrl::External(url.parse().unwrap()),
        )
        .title(title)
        .inner_size(INNER_SIZE.0, INNER_SIZE.1)
        .resizable(true)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .user_agent(PLATFORM_UA)
        .initialization_script(&script)
        .on_navigation(move |url| {
            eprintln!("[{}] navigating to: {}", tag_nav, url);
            true
        })
        .on_page_load(move |_window, payload| {
            eprintln!("[{}] page load event: {:?} url={}", tag_page, payload.event(), payload.url());
        });
        #[cfg(desktop)]
        let builder = builder.center().decorations(true);
        builder.build()
    })
    .await;

    let webview_window = match build_result {
        Ok(Ok(w)) => {
            eprintln!("[{}] build() OK, window created", tag);
            w
        }
        Ok(Err(e)) => {
            eprintln!("[{}] build() returned Err: {:?}", tag, e);
            return Err(format!("打开登录窗口失败: {}", e));
        }
        Err(join_err) => {
            eprintln!("[{}] spawn_blocking join error: {:?}", tag, join_err);
            if join_err.is_panic() {
                return Err(format!("打开登录窗口时线程 panic: {}", join_err));
            }
            return Err(format!("打开登录窗口时线程异常: {}", join_err));
        }
    };

    // 始终打开 DevTools（不限于 debug 模式），便于调试空白问题
    #[cfg(desktop)]
    webview_window.open_devtools();

    // 探询线程：定期用 Tauri 2 官方 cookies_for_url() 获取完整 Cookie（含 HttpOnly）；
    // probe 是独立线程，不在 Tauri 主消息循环中，可安全调用
    let wv = webview_window.clone();
    let app_for_thread = app.clone();
    let tag_probe = tag.to_string();
    std::thread::spawn(move || {
        for i in 1..=30 {
            std::thread::sleep(std::time::Duration::from_secs(2));
            // 窗口已关闭则停止 probe
            if app_for_thread.get_webview_window(spec.window_label).is_none() {
                eprintln!("[{}] probe stopped, window closed", tag_probe);
                break;
            }

            let url: tauri::Url = match spec.cookie_domain.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let cookies = match wv.cookies_for_url(url.clone()) {
                Ok(c) => {
                    eprintln!("[{}] probe #{}: cookies_for_url() returned {} cookies", tag_probe, i, c.len());
                    c
                }
                Err(e) => {
                    eprintln!("[{}] probe #{}: cookies_for_url() FAILED: {:?}, will retry", tag_probe, i, e);
                    continue;
                }
            };

            // 拼接 Cookie 字符串：key=value; key=value
            let cookie_str: String = cookies
                .iter()
                .map(|c| format!("{}={}", c.name(), c.value()))
                .collect::<Vec<_>>()
                .join("; ");

            let has_marker = cookie_str.contains(spec.cookie_marker);
            eprintln!("[{}] probe #{}: cookie_str.length={}, hasMarker={}",
                tag_probe, i, cookie_str.len(), has_marker);

            if has_marker && cookie_str.len() > 50 {
                // 登录成功，保存完整 Cookie（含 HttpOnly）
                eprintln!("[{}] 登录成功，保存完整 Cookie（含 HttpOnly）, length={}",
                    tag_probe, cookie_str.len());
                if let Err(e) = save_login_cookie(&app_for_thread, &spec, &cookie_str) {
                    eprintln!("[{}] 保存 Cookie 失败: {}", tag_probe, e);
                }
                break;
            }
        }
    });

    Ok(())
}

/// 保存登录 Cookie 的唯一入口：校验标记 → 写入对应配置字段 →
/// 重置该网盘的过期提醒标记 → emit 事件 → 关闭登录窗口。
/// 探询线程、save_quark_cookie 命令、quark-cb 回调三处共用。
pub fn save_login_cookie<R: Runtime>(
    app: &tauri::AppHandle<R>,
    spec: &WebviewLoginSpec,
    cookie: &str,
) -> Result<(), String> {
    let tag = spec.log_tag;
    if !cookie.contains(spec.cookie_marker) {
        eprintln!("[{}] cookie 缺少标记 {}，登录未成功", tag, spec.cookie_marker);
        return Err(format!("Cookie 中缺少 {}，登录未成功", spec.cookie_marker));
    }

    let state = app.state::<Arc<AppState>>();
    let expires_at = chrono::Local::now().timestamp() + spec.expiry_days * 86400;
    {
        let mut config = state.database.lock().unwrap().load_config().unwrap_or_default();
        match spec.provider {
            "baidu" => {
                config.baidu_cookie = cookie.to_string();
                config.baidu_cookie_expires_at = expires_at;
            }
            "quark" => {
                config.quark_cookie = cookie.to_string();
                config.quark_cookie_expires_at = expires_at;
            }
            _ => return Err(format!("未知的登录提供者: {}", spec.provider)),
        }
        state.database.lock().unwrap().save_config(&config)?;
    }
    eprintln!("[{}] config saved, expires_at={}", tag, expires_at);

    // 重新登录，重置对应网盘的过期提醒标记
    match spec.provider {
        "quark" => state
            .quark_cookie_warn_sent
            .store(false, std::sync::atomic::Ordering::SeqCst),
        "baidu" => state
            .baidu_cookie_warn_sent
            .store(false, std::sync::atomic::Ordering::SeqCst),
        _ => {}
    }

    let _ = app.emit(spec.success_event, cookie);
    eprintln!("[{}] emitted {} event", tag, spec.success_event);

    if let Some(w) = app.get_webview_window(spec.window_label) {
        let _ = w.close();
        eprintln!("[{}] login window closed", tag);
    }
    Ok(())
}

/// quark-cb 回调页：显示"登录成功，正在关闭..."
const CALLBACK_HTML: &str = r#"<!DOCTYPE html>
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

/// 注册 quark-cb URI scheme 协议：处理夸克登录回调，
/// 从 URL 提取 Cookie 并经 save_login_cookie 唯一入口保存
#[cfg(desktop)]
pub fn register_quark_callback_scheme<R: Runtime>(
    builder: tauri::Builder<R>,
) -> tauri::Builder<R> {
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

        // 仅在 Cookie 有效时保存并发事件（无效 Cookie 不再通知前端）
        if let Err(e) = save_login_cookie(handle, &quark_spec(), &cookie) {
            eprintln!("[quark-login] quark-cb: {}", e);
        }

        tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", "text/html; charset=utf-8")
            .body(CALLBACK_HTML.as_bytes().to_vec())
            .unwrap()
    })
}
