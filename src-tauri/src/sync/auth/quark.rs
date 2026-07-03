use super::{browser_client, QrAuthenticator, QrLoginSession, QrLoginStatus};
use base64::Engine;

/// 夸克网盘扫码登录提供者
///
/// 基于夸克统一登录 uop.quark.cn CAS 二维码协议（非官方）。
/// 流程：getTokenForQrcodeLogin → 生成二维码 → 轮询 getServiceTicketByQrcodeToken
///       → 用 service_ticket 换取 pan.quark.cn 的持久 Cookie（__puus 等）。
/// 接口失效时 validate 返回 false，前端提示重新扫码或手动填 Cookie。
pub struct QuarkAuth {
    client: reqwest::blocking::Client,
}

impl QuarkAuth {
    pub fn new() -> Self {
        QuarkAuth {
            client: browser_client(),
        }
    }
}

impl Default for QuarkAuth {
    fn default() -> Self {
        Self::new()
    }
}

const UA_PLATFORM: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const SCAN_LOGIN_PAGE: &str = "https://su.quark.cn/4_eMHBJ";

fn now_plus_days(days: i64) -> i64 {
    chrono::Local::now().timestamp() + days * 86400
}

/// 把二维码内容字符串渲染成 PNG 的 data URL
fn qr_to_data_url(content: &str) -> Result<String, String> {
    let code = qrcode::QrCode::new(content.as_bytes())
        .map_err(|e| format!("生成二维码失败: {}", e))?;
    let img = code
        .render::<image::Luma<u8>>()
        .min_dimensions(256, 256)
        .build();
    let mut buf = Vec::new();
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("编码二维码图片失败: {}", e))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok(format!("data:image/png;base64,{}", b64))
}

/// 从响应头提取所有 set-cookie 的 k=v，用 "; " 拼接
fn extract_all_cookies(resp: &reqwest::blocking::Response) -> String {
    let mut pairs: Vec<String> = Vec::new();
    for hv in resp.headers().get_all("set-cookie") {
        if let Ok(s) = hv.to_str() {
            if let Some(kv) = s.split(';').next() {
                let kv = kv.trim().to_string();
                if !kv.is_empty() {
                    pairs.push(kv);
                }
            }
        }
    }
    pairs.join("; ")
}

/// 用 service_ticket 换取 pan.quark.cn 的持久 Cookie
fn exchange_service_ticket(
    client: &reqwest::blocking::Client,
    service_ticket: &str,
) -> Result<String, String> {
    let url = format!(
        "https://pan.quark.cn/account/info?st={}&lw=scan",
        service_ticket
    );
    let resp = client
        .get(&url)
        .header("User-Agent", UA_PLATFORM)
        .header("Referer", "https://pan.quark.cn/")
        .send()
        .map_err(|e| format!("换取夸克 Cookie 失败: {}", e))?;

    let cookie = extract_all_cookies(&resp);
    if cookie.contains("__puus=") {
        Ok(cookie)
    } else {
        // 如果不跟随重定向拿不到 cookie，尝试跟随重定向再试一次
        let redirect_client = reqwest::blocking::Client::builder()
            .user_agent(UA_PLATFORM)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| format!("创建重定向客户端失败: {}", e))?;

        let resp2 = redirect_client
            .get(&url)
            .header("Referer", "https://pan.quark.cn/")
            .send()
            .map_err(|e| format!("换取夸克 Cookie 失败(重试): {}", e))?;

        let cookie2 = extract_all_cookies(&resp2);
        if cookie2.contains("__puus=") {
            Ok(cookie2)
        } else {
            Err(format!(
                "未获取到夸克持久 Cookie (__puus)，请改用手动填入 Cookie"
            ))
        }
    }
}

impl QrAuthenticator for QuarkAuth {
    fn name(&self) -> &str {
        "夸克网盘"
    }

    fn start_login(&self) -> Result<QrLoginSession, String> {
        // 1. 获取 token
        let url = "https://uop.quark.cn/cas/ajax/getTokenForQrcodeLogin?client_id=532";
        let resp: serde_json::Value = self
            .client
            .get(url)
            .header("Referer", "https://pan.quark.cn/")
            .header("Origin", "https://pan.quark.cn")
            .send()
            .map_err(|e| format!("获取夸克二维码 token 失败: {}", e))?
            .json()
            .map_err(|e| format!("解析夸克 token 响应失败: {}", e))?;

        let token = resp["data"]["members"]["token"]
            .as_str()
            .ok_or_else(|| format!("未获取到夸克 token: {:?}", resp))?
            .to_string();

        // 2. 构建二维码内容 URL
        let qr_content = format!(
            "{}?token={}&client_id=532&ssb=weblogin&uc_param_str=&uc_biz_str=S:custom|OPT:SAREA@0|OPT:IMMERSIVE@1|OPT:BACK_BTN_STYLE@0",
            SCAN_LOGIN_PAGE,
            token
        );

        // 3. 渲染二维码为 PNG data URL
        let qr_image = qr_to_data_url(&qr_content)?;

        Ok(QrLoginSession {
            qr_image,
            login_token: token,
            provider: "quark".to_string(),
        })
    }

    fn poll_login(&self, login_token: &str) -> Result<QrLoginStatus, String> {
        let url = format!(
            "https://uop.quark.cn/cas/ajax/getServiceTicketByQrcodeToken?client_id=532&token={}",
            login_token
        );

        let resp: serde_json::Value = self
            .client
            .get(&url)
            .header("Referer", "https://pan.quark.cn/")
            .header("Origin", "https://pan.quark.cn")
            .send()
            .map_err(|e| format!("轮询夸克扫码失败: {}", e))?
            .json()
            .map_err(|e| format!("解析夸克轮询响应失败: {}", e))?;

        // status 字段为整数：2000000=确认, 50004002=token不存在/过期
        let status = resp["status"].as_i64().unwrap_or(-1);

        match status {
            2000000 => {
                // 登录确认，提取 service_ticket
                let service_ticket = resp["data"]["members"]["service_ticket"]
                    .as_str()
                    .ok_or_else(|| format!("未获取到 service_ticket: {:?}", resp))?;

                // 用 service_ticket 换取持久 Cookie
                match exchange_service_ticket(&self.client, service_ticket) {
                    Ok(cookie) => Ok(QrLoginStatus::Confirmed {
                        cookie,
                        expires_at: now_plus_days(45),
                    }),
                    Err(e) => Ok(QrLoginStatus::Failed { message: e }),
                }
            }
            50004002 => Ok(QrLoginStatus::Expired),
            _ => {
                // 其他状态码表示等待扫码或已扫码待确认
                Ok(QrLoginStatus::Waiting)
            }
        }
    }

    fn validate(&self, cookie: &str) -> Result<bool, String> {
        if !cookie.contains("__puus=") {
            return Ok(false);
        }
        let url = "https://pan.quark.cn/account/info?fr=pc&platform=pc";
        let resp: serde_json::Value = self
            .client
            .get(url)
            .header("Cookie", cookie)
            .header("Referer", "https://pan.quark.cn/")
            .header("Origin", "https://pan.quark.cn")
            .header("User-Agent", UA_PLATFORM)
            .send()
            .map_err(|e| format!("校验夸克失败: {}", e))?
            .json()
            .map_err(|e| format!("解析校验响应失败: {}", e))?;

        // data 字段非空表示 Cookie 有效
        Ok(resp["data"].is_object() || resp["data"].is_array())
    }
}

/// 兼容夸克响应里 code 出现在顶层或 data 内
fn body_code(v: &serde_json::Value) -> i64 {
    if let Some(c) = v["code"].as_i64() {
        return c;
    }
    if let Some(c) = v["data"]["code"].as_i64() {
        return c;
    }
    -1
}
