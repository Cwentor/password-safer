use super::{browser_client, extract_cookies_by_prefixes, QrAuthenticator, QrLoginSession, QrLoginStatus};
use rand::Rng;

/// 百度网盘扫码登录提供者
///
/// 基于百度 passport 二维码协议（非官方）：
///   getqrcode → unicast 轮询 → login 换 BDUSS
/// 接口若随版本变动失效，validate 会返回 false，前端提示重新扫码。
pub struct BaiduAuth {
    client: reqwest::blocking::Client,
}

impl BaiduAuth {
    pub fn new() -> Self {
        BaiduAuth {
            client: browser_client(),
        }
    }
}

impl Default for BaiduAuth {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

fn now_plus_days(days: i64) -> i64 {
    chrono::Local::now().timestamp() + days * 86400
}

fn gen_gid() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| format!("{:x}", rng.gen_range(0u8..16)))
        .collect()
}

impl QrAuthenticator for BaiduAuth {
    fn name(&self) -> &str {
        "百度网盘"
    }

    fn start_login(&self) -> Result<QrLoginSession, String> {
        let ts = now_ms();
        let gid = gen_gid();
        let url = format!(
            "https://passport.baidu.com/v2/api/getqrcode?lp=pc&qrloginfrom=pc&gid={gid}&apiver=v3&tt={ts}&tpl=mn&_={ts}",
            gid = gid,
            ts = ts
        );

        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("获取二维码失败: {}", e))?
            .json()
            .map_err(|e| format!("解析二维码响应失败: {}", e))?;

        let imgurl = resp["imgurl"]
            .as_str()
            .ok_or_else(|| format!("未获取到二维码图片: {:?}", resp))?
            .to_string();
        let sign = resp["sign"]
            .as_str()
            .ok_or_else(|| format!("未获取到二维码 sign: {:?}", resp))?
            .to_string();

        // login_token 编码 sign 与 gid，轮询时拆出
        let token = format!("{}|{}", sign, gid);
        Ok(QrLoginSession {
            qr_image: imgurl,
            login_token: token,
            provider: "baidu".to_string(),
        })
    }

    fn poll_login(&self, login_token: &str) -> Result<QrLoginStatus, String> {
        let parts: Vec<&str> = login_token.split('|').collect();
        if parts.len() < 2 {
            return Ok(QrLoginStatus::Failed {
                message: "无效的登录票据".to_string(),
            });
        }
        let sign = parts[0];
        let gid = parts[1];
        let ts = now_ms();
        let url = format!(
            "https://passport.baidu.com/v2/channel/unicast?channel_id={sign}&gid={gid}&apiver=v3&tt={ts}&tpl=mn&_={ts}",
            sign = sign,
            gid = gid,
            ts = ts
        );

        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("轮询失败: {}", e))?
            .json()
            .map_err(|e| format!("解析轮询响应失败: {}", e))?;

        let no = resp["no"].as_str().unwrap_or("");
        if no.is_empty() {
            return Ok(QrLoginStatus::Waiting);
        }

        // no 为 JSON 字符串，含 loginId / authSid
        let no_json: serde_json::Value = serde_json::from_str(no)
            .map_err(|e| format!("解析扫码确认信息失败: {}", e))?;
        let login_id = no_json["loginId"]
            .as_str()
            .ok_or("未获取到 loginId")?;
        let auth_sid = no_json["authSid"]
            .as_str()
            .ok_or("未获取到 authSid")?;

        // 用 loginId 完成登录，换取 BDUSS Cookie
        let ts = now_ms();
        let login_url = format!(
            "https://passport.baidu.com/v2/api/?login?tpl=mn&staticpage=https%3A%2F%2Fwww.baidu.com%2Fcache%2Fuser%2Fhtml%2Fv3Jump.html&loginId={lid}&authSid={sid}&gid={gid}&apiver=v3&logLogin=true&tt={ts}&_={ts}",
            lid = login_id,
            sid = auth_sid,
            gid = gid,
            ts = ts
        );

        let resp = self
            .client
            .get(&login_url)
            .send()
            .map_err(|e| format!("登录换 Cookie 失败: {}", e))?;

        let cookie = extract_cookies_by_prefixes(&resp, &["BDUSS=", "STOKEN=", "PTOKEN="]);
        if !cookie.contains("BDUSS=") {
            return Ok(QrLoginStatus::Failed {
                message: "未获取到 BDUSS，登录失败".to_string(),
            });
        }

        Ok(QrLoginStatus::Confirmed {
            cookie,
            expires_at: now_plus_days(60),
        })
    }

    fn validate(&self, cookie: &str) -> Result<bool, String> {
        if !cookie.contains("BDUSS=") {
            return Ok(false);
        }
        let url = "https://pan.baidu.com/api/list?dir=%2F&num=1&order=time&clienttype=0&web=5";
        let resp: serde_json::Value = self
            .client
            .get(url)
            .header("Cookie", cookie)
            .send()
            .map_err(|e| format!("校验失败: {}", e))?
            .json()
            .map_err(|e| format!("解析校验响应失败: {}", e))?;

        // errno=2 表示未登录；0 或 -9 等表示登录有效
        let errno = resp["errno"].as_i64().unwrap_or(2);
        Ok(errno == 0 || errno == -9)
    }
}
