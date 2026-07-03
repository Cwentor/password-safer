use super::SyncProvider;
use serde_json::Value;
use std::path::PathBuf;

/// 百度网盘同步提供者（基于 BDUSS Cookie 走 web 端接口，非 OAuth）
pub struct BaiduProvider {
    cookie: String,
    client: reqwest::blocking::Client,
}

const BLOCK_SIZE: usize = 4 * 1024 * 1024;

impl BaiduProvider {
    pub fn new(cookie: String) -> Self {
        BaiduProvider {
            cookie,
            client: reqwest::blocking::Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new()),
        }
    }

    fn header(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&self.cookie) {
            h.insert(reqwest::header::COOKIE, v);
        }
        h
    }

    /// 获取 bdstoken（上传/创建需要）
    fn bdstoken(&self) -> Result<String, String> {
        let url = "https://pan.baidu.com/api/gettemplatevariable?clienttype=0&app_id=250528&web=5&fields=%5B%22bdstoken%22%5D";
        let resp: Value = self
            .client
            .get(url)
            .headers(self.header())
            .send()
            .map_err(|e| format!("获取 bdstoken 失败: {}", e))?
            .json()
            .map_err(|e| format!("解析 bdstoken 响应失败: {}", e))?;

        let token = resp["result"]["bdstoken"]
            .as_str()
            .or_else(|| resp["bdstoken"].as_str())
            .ok_or_else(|| format!("未获取到 bdstoken: {:?}", resp))?
            .to_string();
        Ok(token)
    }

    /// 把 /a/b/vault.db 拆为 目录(/a/b) 与 文件路径（完整）
    fn parent_dir(remote_path: &str) -> String {
        let trimmed = remote_path.trim_start_matches('/');
        match trimmed.rfind('/') {
            Some(i) => "/".to_string() + &trimmed[..i],
            None => "/".to_string(),
        }
    }

    /// 计算整文件 md5、分片 md5 列表、首片 md5
    fn file_hashes(path: &PathBuf) -> Result<(String, Vec<String>, String), String> {
        use md5::{Digest, Md5};
        let data = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
        let mut full = Md5::new();
        full.update(&data);
        let full_md5 = hex::encode(full.finalize());

        let mut block_list = Vec::new();
        for chunk in data.chunks(BLOCK_SIZE) {
            let mut h = Md5::new();
            h.update(chunk);
            block_list.push(hex::encode(h.finalize()));
        }
        let slice_md5 = block_list.first().cloned().unwrap_or_default();
        Ok((full_md5, block_list, slice_md5))
    }
}

impl SyncProvider for BaiduProvider {
    fn name(&self) -> &str {
        "百度网盘"
    }

    fn upload(&self, local_path: &PathBuf, remote_path: &str) -> Result<(), String> {
        let bdstoken = self.bdstoken()?;
        let dir = Self::parent_dir(remote_path);

        let file_size = std::fs::metadata(local_path)
            .map_err(|e| format!("读取本地文件失败: {}", e))?
            .len() as i64;
        let (full_md5, block_list, slice_md5) = Self::file_hashes(local_path)?;
        let block_list_json = serde_json::to_string(&block_list).unwrap_or_default();

        let pre_url = format!(
            "https://pan.baidu.com/api/precreate?access_token=&bdstoken={}&clienttype=0&app_id=250528&web=5",
            bdstoken
        );
        let pre_body = format!(
            "path={}&size={}&isdir=0&autoinit=1&target_path={}&block_list={}&content-md5={}&slice-md5={}",
            urlenc(remote_path),
            file_size,
            urlenc(&dir),
            urlenc(&block_list_json),
            full_md5,
            slice_md5
        );

        let pre_resp: Value = self
            .client
            .post(&pre_url)
            .headers(self.header())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(pre_body)
            .send()
            .map_err(|e| format!("预上传失败: {}", e))?
            .json()
            .map_err(|e| format!("解析预上传响应失败: {}", e))?;

        if pre_resp["errno"].as_i64().is_some() && pre_resp["errno"].as_i64() != Some(0) {
            return Err(format!("预上传返回错误: {:?}", pre_resp));
        }
        let uploadid = pre_resp["uploadid"]
            .as_str()
            .ok_or_else(|| format!("未获取到 uploadid: {:?}", pre_resp))?
            .to_string();

        // 分片上传
        let data = std::fs::read(local_path)
            .map_err(|e| format!("读取本地文件失败: {}", e))?;
        let mut partseq = 0i64;
        for chunk in data.chunks(BLOCK_SIZE) {
            let part_url = format!(
                "https://d.pcs.baidu.com/rest/2.0/pcs/superfile2?access_token=&method=upload&type=tmpfile&path={}&uploadid={}&partseq={}&app_id=250528&clienttype=0&web=5&bdstoken={}",
                urlenc(remote_path),
                uploadid,
                partseq,
                bdstoken
            );
            let form = reqwest::blocking::multipart::Form::new().part(
                "file",
                reqwest::blocking::multipart::Part::bytes(chunk.to_vec()).file_name("file"),
            );
            let resp: Value = self
                .client
                .post(&part_url)
                .headers(self.header())
                .multipart(form)
                .send()
                .map_err(|e| format!("分片上传失败: {}", e))?
                .json()
                .map_err(|e| format!("解析分片响应失败: {}", e))?;
            if resp["error_code"].as_i64().is_some() {
                return Err(format!("分片上传错误: {:?}", resp));
            }
            partseq += 1;
        }

        // 创建文件
        let create_url = format!(
            "https://pan.baidu.com/api/create?access_token=&bdstoken={}&clienttype=0&app_id=250528&web=5",
            bdstoken
        );
        let create_body = format!(
            "path={}&size={}&isdir=0&rtype=3&uploadid={}&block_list={}",
            urlenc(remote_path),
            file_size,
            uploadid,
            urlenc(&block_list_json)
        );
        let create_resp: Value = self
            .client
            .post(&create_url)
            .headers(self.header())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(create_body)
            .send()
            .map_err(|e| format!("创建文件失败: {}", e))?
            .json()
            .map_err(|e| format!("解析创建响应失败: {}", e))?;

        if create_resp["errno"].as_i64() != Some(0) {
            return Err(format!("创建文件错误: {:?}", create_resp));
        }
        Ok(())
    }

    fn download(&self, remote_path: &str, local_path: &PathBuf) -> Result<(), String> {
        let url = format!(
            "https://d.pcs.baidu.com/rest/2.0/pcs/file?method=download&path={}",
            urlenc(remote_path)
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.header())
            .send()
            .map_err(|e| format!("下载失败: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("下载失败: HTTP {}", resp.status()));
        }
        let bytes = resp
            .bytes()
            .map_err(|e| format!("读取下载内容失败: {}", e))?;
        std::fs::write(local_path, &bytes).map_err(|e| format!("写入本地文件失败: {}", e))?;
        Ok(())
    }
}

/// 简易 URL 编码
fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
