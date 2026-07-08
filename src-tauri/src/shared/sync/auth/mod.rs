pub mod baidu;
pub mod quark;

use serde::{Deserialize, Serialize};

/// 扫码登录会话：前端据此展示二维码，并轮询后端
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrLoginSession {
    /// 二维码图片：data:image/png;base64,... 或 https 图片地址
    pub qr_image: String,
    /// 轮询票据（可能编码了内部上下文，对各提供者不透明）
    pub login_token: String,
    /// 提供者标识，便于前端区分
    pub provider: String,
}

/// 扫码轮询状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum QrLoginStatus {
    /// 等待扫码
    Waiting,
    /// 已扫码，等待用户在手机端确认
    Scanned,
    /// 已确认，得到 Cookie 与预估失效时间（unix 秒，0 表示未知）
    Confirmed {
        cookie: String,
        expires_at: i64,
    },
    /// 二维码已过期，需重新生成
    Expired,
    /// 流程失败
    Failed {
        message: String,
    },
}

/// 扫码登录提供者 trait
/// 只关心"如何拿到一个有效 Cookie"，与文件同步完全解耦
pub trait QrAuthenticator: Send + Sync {
    /// 发起扫码登录，返回二维码与轮询票据
    fn start_login(&self) -> Result<QrLoginSession, String>;
    /// 轮询登录状态
    fn poll_login(&self, login_token: &str) -> Result<QrLoginStatus, String>;
    /// 校验已保存的 Cookie 是否仍有效
    fn validate(&self, cookie: &str) -> Result<bool, String>;
    /// 提供者名称
    #[allow(dead_code)]
    fn name(&self) -> &str;
}

/// 从响应头中提取指定前缀的 Cookie 键值对，用 "; " 拼接
pub fn extract_cookies_by_prefixes(
    resp: &reqwest::blocking::Response,
    prefixes: &[&str],
) -> String {
    let mut pairs: Vec<String> = Vec::new();
    for hv in resp.headers().get_all("set-cookie") {
        if let Ok(s) = hv.to_str() {
            if let Some(kv) = s.split(';').next() {
                let kv = kv.trim();
                if prefixes.iter().any(|p| kv.starts_with(p)) {
                    pairs.push(kv.to_string());
                }
            }
        }
    }
    pairs.join("; ")
}

/// 把若干 "k=v" 合并成 Cookie 头值
#[allow(dead_code)]
pub fn join_cookies(items: &[&str]) -> String {
    items.join("; ")
}

/// 浏览器风格的 reqwest blocking client（不跟随重定向，便于取 Set-Cookie）
pub fn browser_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}
