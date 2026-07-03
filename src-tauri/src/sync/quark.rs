use super::SyncProvider;
use md5::Md5;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::path::PathBuf;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";
const PART_SIZE: usize = 4 * 1024 * 1024;

/// 上传断点续传元数据
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct UploadMeta {
    /// 文件完整 SHA1（用于验证是同一文件）
    pub sha1: String,
    /// 上传任务 ID
    pub task_id: String,
    /// 目标对象 fid
    pub obj_fid: String,
    /// 父目录 fid
    pub parent_fid: String,
    /// 文件名
    pub fname: String,
    /// 已完成分片索引列表
    pub completed_parts: Vec<usize>,
}

/// 夸克网盘同步提供者（基于 Cookie 走 drive-pc.quark.cn，非官方）
pub struct QuarkProvider {
    cookie: String,
    client: reqwest::blocking::Client,
}

impl QuarkProvider {
    pub fn new(cookie: String) -> Self {
        eprintln!("[quark] QuarkProvider::new cookie.length={}, hasPus={}",
            cookie.len(), cookie.contains("__pus="));
        QuarkProvider {
            cookie,
            client: reqwest::blocking::Client::builder()
                .user_agent(UA)
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new()),
        }
    }

    /// 检测本地 vault.db 文件有效性（三重判据）
    /// 1. 文件存在
    /// 2. 文件大小 > 0
    /// 3. SQLite PRAGMA integrity_check 返回 ok
    pub fn local_vault_valid(local_path: &std::path::Path) -> bool {
        if !local_path.exists() {
            return false;
        }
        let metadata = match std::fs::metadata(local_path) {
            Ok(m) => m,
            Err(_) => return false,
        };
        if metadata.len() == 0 {
            return false;
        }
        crate::db::integrity_check(local_path)
    }

    /// 获取上传断点续传 meta 文件路径
    /// 与本地文件同目录，文件名 = 原文件名 + ".uploadmeta"
    fn upload_meta_path(local_path: &std::path::Path) -> std::path::PathBuf {
        let mut p = local_path.to_path_buf();
        let mut name = p.file_name().unwrap_or_default().to_os_string();
        name.push(".uploadmeta");
        p.set_file_name(name);
        p
    }

    fn hdr(&self) -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&self.cookie) {
        h.insert(reqwest::header::COOKIE, v);
    } else {
        eprintln!("[quark] cookie header insert FAILED, len={}", self.cookie.len());
    }
    if let Ok(v) = reqwest::header::HeaderValue::from_str("https://pan.quark.cn/") {
        h.insert(reqwest::header::REFERER, v);
    }
    if let Ok(v) = reqwest::header::HeaderValue::from_str("https://pan.quark.cn") {
        h.insert(reqwest::header::ORIGIN, v);
    }
    if let Ok(v) = reqwest::header::HeaderValue::from_str("zh-CN,zh;q=0.9") {
        h.insert(reqwest::header::ACCEPT_LANGUAGE, v);
    }
    if let Ok(v) = reqwest::header::HeaderValue::from_str(UA) {
        h.insert(reqwest::header::USER_AGENT, v);
    }
    h.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    h
}

    /// drive-pc 业务接口 POST
    fn api_post(&self, action: &str, body: &Value) -> Result<Value, String> {
        let url = format!(
            "https://drive-pc.quark.cn/1/clouddrive/{}?pr=ucpro&fr=pc&{}",
            action,
            now_quark_query()
        );
        let resp: Value = self
            .client
            .post(&url)
            .headers(self.hdr())
            .json(body)
            .send()
            .map_err(|e| format!("请求 {} 失败: {}", action, e))?
            .json()
            .map_err(|e| format!("解析 {} 响应失败: {}", action, e))?;
        let code = resp["code"].as_i64().unwrap_or(0);
        if code != 0 {
            eprintln!("[quark] api_post {} FAILED: code={}, msg={}, body={}",
                action, code, resp["message"].as_str().unwrap_or(""), body);
            return Err(format!("{} 返回错误: {} ({})", action, resp["message"].as_str().unwrap_or(""), code));
        }
        Ok(resp)
    }

    /// drive-pc 业务接口 GET（参数通过 query string 传递）
    /// 用于 file/search 等只接受 GET 请求的接口
    fn api_get(&self, action: &str, params: &Value) -> Result<Value, String> {
        let mut url = format!(
            "https://drive-pc.quark.cn/1/clouddrive/{}?pr=ucpro&fr=pc",
            action
        );
        // 将 params 中的字段转为 query string
        if let Some(obj) = params.as_object() {
            for (k, v) in obj {
                let val_str = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => v.to_string().trim_matches('"').to_string(),
                };
                url.push_str(&format!("&{}={}", k, urlencode(&val_str)));
            }
        }
        url.push_str(&format!("&{}", now_quark_query()));
        let resp: Value = self
            .client
            .get(&url)
            .headers(self.hdr())
            .send()
            .map_err(|e| format!("请求 {} 失败: {}", action, e))?
            .json()
            .map_err(|e| format!("解析 {} 响应失败: {}", action, e))?;
        let code = resp["code"].as_i64().unwrap_or(0);
        if code != 0 {
            eprintln!("[quark] api_get {} FAILED: code={}, msg={}, params={}",
                action, code, resp["message"].as_str().unwrap_or(""), params);
            return Err(format!("{} 返回错误: {} ({})", action, resp["message"].as_str().unwrap_or(""), code));
        }
        Ok(resp)
    }

    /// 删除文件/目录（按 fid 列表批量删除）
    /// 用于处理同名冲突：上传前先删除远端旧文件
    fn delete_files(&self, fids: &[&str]) -> Result<(), String> {
        if fids.is_empty() {
            return Ok(());
        }
        let body = json!({
            "action_type": 2,
            "task_list": fids.iter().map(|f| json!({"fid": f})).collect::<Vec<_>>(),
            "exclude_fids": []
        });
        let _ = self.api_post("file/delete", &body)?;
        eprintln!("[quark] deleted {} files", fids.len());
        Ok(())
    }

    /// 取路径父目录 fid，逐级查找或创建
    fn ensure_parent_fid(&self, remote_path: &str) -> Result<String, String> {
        let trimmed = remote_path.trim_start_matches('/');
        let comps: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
        if comps.len() <= 1 {
            return Ok("0".to_string());
        }
        let mut cur_fid = "0".to_string();
        for name in &comps[..comps.len() - 1] {
            cur_fid = self.find_or_create_dir(&cur_fid, name)?;
        }
        Ok(cur_fid)
    }

    /// 在 fid 目录下查找名为 name 的子目录，找不到则创建
    /// 处理 23008 同名冲突：doloading 状态的目录不在 file/sort 列表，
    /// 需要用 file/search 查找卡死的目录并删除后重试
    /// 不降级到根目录（避免两份 vault.db 导致索引混乱）
    fn find_or_create_dir(&self, pdir_fid: &str, name: &str) -> Result<String, String> {
        let body = json!({
            "pdir_fid": pdir_fid,
            "_page": 1,
            "_size": 200,
            "_fetch_total": 1,
            "_fetch_sub_dirs": "1",
            "_sort": "file_type:asc,updated_at:desc"
        });
        let resp = self.api_post("file/sort", &body)?;
        if let Some(list) = resp["data"]["list"].as_array() {
            for item in list {
                if item["file_name"].as_str() == Some(name)
                    && item["file_type"].as_i64() == Some(0)
                {
                    return Ok(item["fid"].as_str().unwrap_or("0").to_string());
                }
            }
        }
        // 创建目录
        let create_body = json!({
            "pdir_fid": pdir_fid,
            "file_name": name,
            "dir_path": "",
            "dir_init_lock": false,
        });
        match self.api_post("file", &create_body) {
            Ok(resp) => Ok(resp["data"]["fid"].as_str().unwrap_or("0").to_string()),
            Err(e) => {
                // 23008 同名冲突：目录卡在 doloading 状态
                // doloading 状态的目录可接收文件上传，只是 file/sort 查不到
                // 因此直接复用 fid，不删除（删除可能失败且重新创建可能再次进入 doloading）
                if e.contains("(23008)") {
                    eprintln!("[quark] find_or_create_dir 23008 冲突，尝试用 file/search 查找卡死目录");
                    // search_file_by_name 已实现 GET 请求 + 多参数组合 + 完整诊断日志
                    match self.search_file_by_name(name, pdir_fid) {
                        Ok(Some(stuck_fid)) => {
                            // doloading 目录可接收文件上传，直接复用 fid
                            eprintln!("[quark] find_or_create_dir: {} 目录卡在 doloading 状态，复用 fid={}",
                                name, stuck_fid);
                            return Ok(stuck_fid);
                        }
                        Ok(None) => {
                            eprintln!("[quark] find_or_create_dir: {} 目录卡在 doloading 状态但 file/search 所有参数组合均未找到，请手动在夸克网盘 UI 中检查", name);
                            return Err(format!("目录 '{}' 卡在 doloading 状态但 file/search 所有参数组合均未找到，请手动在夸克网盘 UI 中检查并删除该目录后重试", name));
                        }
                        Err(search_err) => {
                            eprintln!("[quark] find_or_create_dir: search_file_by_name 调用异常: {}", search_err);
                            return Err(format!("查找卡死目录 '{}' 时发生异常: {}，请手动在夸克网盘 UI 中检查", name, search_err));
                        }
                    }
                }
                Err(e)
            }
        }
    }

    /// 通过 file/search 接口按名称搜索文件/目录（可查到 doloading 状态的）
    /// 尝试多种参数组合，输出完整诊断日志
    fn search_file_by_name(&self, name: &str, pdir_fid: &str) -> Result<Option<String>, String> {
        // 三种参数组合，覆盖夸克 API 可能的参数差异
        let combos: [(&str, Value); 3] = [
            // 组合 A：search_type=folder + pdir_fid 限定（原实现）
            ("A", json!({
                "pdir_fid": pdir_fid,
                "_page": 1,
                "_size": 50,
                "_fetch_total": 1,
                "keyword": name,
                "search_type": "folder"
            })),
            // 组合 B：不带 search_type + pdir_fid 限定（搜索所有类型）
            ("B", json!({
                "pdir_fid": pdir_fid,
                "_page": 1,
                "_size": 50,
                "_fetch_total": 1,
                "keyword": name
            })),
            // 组合 C：不带 search_type + 不带 pdir_fid（全局搜索）
            ("C", json!({
                "_page": 1,
                "_size": 50,
                "_fetch_total": 1,
                "keyword": name
            })),
        ];

        for (combo_label, body) in combos.iter() {
            let body_str = serde_json::to_string(body).unwrap_or_default();
            match self.api_get("file/search", body) {
                Ok(resp) => {
                    let resp_preview = serde_json::to_string(&resp)
                        .unwrap_or_default()
                        .chars()
                        .take(500)
                        .collect::<String>();
                    // 检查多种可能的响应结构
                    let list = resp["data"]["list"]
                        .as_array()
                        .or_else(|| resp["data"].as_array())
                        .or_else(|| resp["list"].as_array());
                    if let Some(list) = list {
                        for item in list {
                            if item["file_name"].as_str() == Some(name) {
                                if let Some(fid) = item["fid"].as_str() {
                                    eprintln!("[quark] search_file_by_name: 组合 {} 找到 {}, fid={}",
                                        combo_label, name, fid);
                                    return Ok(Some(fid.to_string()));
                                }
                            }
                        }
                    }
                    eprintln!("[quark] search_file_by_name: 组合 {} 未找到 {}，响应: {}",
                        combo_label, name, resp_preview);
                }
                Err(e) => {
                    eprintln!("[quark] search_file_by_name 组合 {} FAILED: {}, body={}",
                        combo_label, e, body_str);
                }
            }
        }
        Ok(None)
    }

    /// 递归查找文件 fid（最多 3 层）
    fn find_file_fid(&self, pdir_fid: &str, fname: &str, depth: u8) -> Result<Option<String>, String> {
        if depth > 3 {
            return Ok(None);
        }
        let body = json!({
            "pdir_fid": pdir_fid,
            "_page": 1,
            "_size": 200,
            "_fetch_total": 1,
            "_fetch_sub_dirs": "1",
            "_sort": "file_type:asc,updated_at:desc"
        });
        let resp = self.api_post("file/sort", &body)?;
        if let Some(list) = resp["data"]["list"].as_array() {
            for item in list {
                let ft = item["file_type"].as_i64().unwrap_or(0);
                if ft == 1 && item["file_name"].as_str() == Some(fname) {
                    return Ok(Some(item["fid"].as_str().unwrap_or("").to_string()));
                }
                // file_type==0 为目录，递归
                if ft == 0 {
                    if let Some(sub_fid) = item["fid"].as_str() {
                        if let Ok(Some(found)) = self.find_file_fid(sub_fid, fname, depth + 1) {
                            return Ok(Some(found));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// 递归查找文件，返回 (fid, update_time)（最多 3 层）
    fn find_file_meta(&self, pdir_fid: &str, fname: &str, depth: u8) -> Result<Option<(String, i64)>, String> {
        if depth > 3 {
            return Ok(None);
        }
        let body = json!({
            "pdir_fid": pdir_fid,
            "_page": 1,
            "_size": 200,
            "_fetch_total": 1,
            "_fetch_sub_dirs": "1",
            "_sort": "file_type:asc,updated_at:desc"
        });
        let resp = self.api_post("file/sort", &body)?;
        if let Some(list) = resp["data"]["list"].as_array() {
            for item in list {
                let ft = item["file_type"].as_i64().unwrap_or(0);
                if ft == 1 && item["file_name"].as_str() == Some(fname) {
                    let fid = item["fid"].as_str().unwrap_or("").to_string();
                    let mtime = item["update_time"].as_i64().unwrap_or(0);
                    return Ok(Some((fid, mtime)));
                }
                if ft == 0 {
                    if let Some(sub_fid) = item["fid"].as_str() {
                        if let Ok(Some(found)) = self.find_file_meta(sub_fid, fname, depth + 1) {
                            return Ok(Some(found));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// 获取远端文件的最后修改时间（unix 秒）
    /// 文件不存在时返回 Ok(None)
    pub fn remote_file_mtime(&self, remote_path: &str) -> Result<Option<i64>, String> {
        let trimmed = remote_path.trim_start_matches('/');
        let fname = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();
        match self.find_file_meta("0", &fname, 0)? {
            Some((_fid, mtime)) => Ok(Some(mtime)),
            None => Ok(None),
        }
    }

    /// 请求文件下载地址，针对错误码 23018 自动重试（最多 2 次）
    fn request_download_url(&self, fid: &str) -> Result<String, String> {
        let url = format!(
            "https://drive-pc.quark.cn/1/clouddrive/file/download?pr=ucpro&fr=pc&sys=win32&ve=2.5.56&ut=&guid=&{}",
            now_quark_query()
        );
        let body = json!({ "fids": [fid] });
        for attempt in 0..3u8 {
            let resp: Value = self
                .client
                .post(&url)
                .headers(self.hdr())
                .json(&body)
                .send()
                .map_err(|e| format!("请求下载地址失败: {}", e))?
                .json()
                .map_err(|e| format!("解析下载地址响应失败: {}", e))?;
            let code = resp["code"].as_i64().unwrap_or(0);
            if code == 23018 && attempt < 2 {
                // 下载请求过于频繁，稍后重试
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
            if code != 0 {
                return Err(format!("file/download 返回错误: {} ({})", resp["message"].as_str().unwrap_or(""), code));
            }
            return resp["data"][0]["download_url"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("未获取到下载地址: {:?}", resp));
        }
        Err("下载地址请求重试次数耗尽".to_string())
    }
}

impl SyncProvider for QuarkProvider {
    fn name(&self) -> &str {
        "夸克网盘"
    }

    fn upload(&self, local_path: &PathBuf, remote_path: &str) -> Result<(), String> {
        let trimmed = remote_path.trim_start_matches('/');
        let fname = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();
        let parent_fid = self.ensure_parent_fid(remote_path)?;

        // 上传前检查远端同名文件，若存在则删除（避免 23008 同名冲突）
        if let Ok(Some(existing_fid)) = self.find_file_fid(&parent_fid, &fname, 0) {
            eprintln!("[quark] 远端已存在同名文件 fid={}, 先删除", existing_fid);
            if let Err(e) = self.delete_files(&[existing_fid.as_str()]) {
                eprintln!("[quark] 删除远端旧文件失败（继续尝试上传）: {}", e);
            } else {
                // 删除后等待夸克服务端处理完成，避免 23008
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }

        let data = std::fs::read(local_path).map_err(|e| format!("读取本地文件失败: {}", e))?;
        let fsize = data.len() as i64;

        // 计算 MD5 和 SHA1（新 API 需要两个哈希）
        let full_md5 = {
            let mut h = Md5::new();
            h.update(&data);
            hex::encode(h.finalize())
        };
        let full_sha1 = {
            let mut h = Sha1::new();
            h.update(&data);
            hex::encode(h.finalize())
        };
        eprintln!("[quark] upload: fname={}, fsize={}, md5={}..., sha1={}...",
            fname, fsize, &full_md5[..12], &full_sha1[..12]);

        // MIME 类型（夸克需要 format_type 字段）
        let mime_type = "application/octet-stream";

        // 检查断点续传 meta
        let meta_path = Self::upload_meta_path(local_path);
        let mut meta: Option<UploadMeta> = if meta_path.exists() {
            match std::fs::read_to_string(&meta_path) {
                Ok(s) => match serde_json::from_str::<UploadMeta>(&s) {
                    Ok(m) if m.sha1 == full_sha1 && m.fname == fname && m.parent_fid == parent_fid => {
                        // SHA1、文件名、父目录均匹配，可恢复
                        Some(m)
                    }
                    _ => {
                        // 不匹配，删除旧 meta
                        let _ = std::fs::remove_file(&meta_path);
                        None
                    }
                },
                Err(_) => {
                    let _ = std::fs::remove_file(&meta_path);
                    None
                }
            }
        } else {
            None
        };

        // 若无 meta，走完整预上传流程（新 API）
        let (task_id, obj_fid) = if let Some(ref m) = meta {
            (m.task_id.clone(), m.obj_fid.clone())
        } else {
            let now_ts = chrono::Local::now().timestamp_millis();
            // 预上传（新 API：不传 hash，只传基础信息）
            let pre_body = json!({
                "ccp_hash_update": true,
                "dir_name": "",
                "file_name": fname,
                "format_type": mime_type,
                "l_created_at": now_ts,
                "l_updated_at": now_ts,
                "pdir_fid": parent_fid,
                "size": fsize
            });
            let pre = self.api_post("file/upload/pre", &pre_body)?;
            eprintln!("[quark] pre response: {}",
                serde_json::to_string(&pre).unwrap_or_default().chars().take(300).collect::<String>());

            // 获取 task_id
            let tid = pre["data"]["task_id"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| pre["data"]["task_id"].as_i64().map(|v| v.to_string()))
                .ok_or_else(|| format!("未获取到 task_id: {:?}", pre))?;

            // 提交哈希验证（秒传判断）
            let hash_body = json!({
                "md5": full_md5,
                "sha1": full_sha1,
                "task_id": tid
            });
            let hash_resp = self.api_post("file/update/hash", &hash_body)?;
            // 秒传成功：finish 为 true
            if hash_resp["data"]["finish"].as_bool() == Some(true) {
                eprintln!("[quark] 秒传成功");
                let _ = std::fs::remove_file(&meta_path);
                return Ok(());
            }

            let ofid = pre["data"]["obj"]["fid"]
                .as_str()
                .or_else(|| pre["data"]["fid"].as_str())
                .unwrap_or("")
                .to_string();
            (tid, ofid)
        };

        // 创建/更新 meta
        if meta.is_none() {
            meta = Some(UploadMeta {
                sha1: full_sha1.clone(),
                task_id: task_id.clone(),
                obj_fid: obj_fid.clone(),
                parent_fid: parent_fid.clone(),
                fname: fname.clone(),
                completed_parts: Vec::new(),
            });
            // 立即写入 meta（记录 task_id，便于断点恢复）
            if let Some(ref m) = meta {
                let _ = std::fs::write(&meta_path, serde_json::to_string(m).unwrap_or_default());
            }
        }

        // 计算分片
        let mut parts: Vec<(i64, String)> = Vec::new();
        let mut offset = 0i64;
        for chunk in data.chunks(PART_SIZE) {
            let mut h = Sha1::new();
            h.update(chunk);
            parts.push((offset, hex::encode(h.finalize())));
            offset += chunk.len() as i64;
        }

        // 获取上传地址（auth）
        // 克隆已完成分片快照，避免与后续对 meta 的可变借用冲突
        let done_parts: Vec<usize> = meta.as_ref().unwrap().completed_parts.clone();
        let mut part_offs: Vec<Value> = Vec::new();
        for (i, (off, hash)) in parts.iter().enumerate() {
            // 跳过已完成分片
            if done_parts.contains(&i) {
                continue;
            }
            part_offs.push(json!({ "part_offset": off, "part_size": PART_SIZE as i64, "part_sha1": hash }));
        }

        if !part_offs.is_empty() {
            let auth_body = json!({
                "task_id": task_id,
                "part_offs": part_offs,
                "hash_enc": full_sha1,
            });
            let auth = self.api_post("file/upload/auth", &auth_body)?;

            let auth_parts = auth["data"]["part_list"]
                .as_array()
                .or_else(|| auth["data"]["parts"].as_array())
                .ok_or_else(|| format!("未获取到分片上传地址: {:?}", auth))?;

            // auth 返回的是未完成分片的上传地址，需要映射回原始分片索引
            let mut auth_idx = 0;
            for (i, chunk) in data.chunks(PART_SIZE).enumerate() {
                // 跳过已完成分片
                if done_parts.contains(&i) {
                    continue;
                }
                let upload_url = auth_parts
                    .get(auth_idx)
                    .and_then(|p| {
                        p["upload_url"]
                            .as_str()
                            .or_else(|| p["url"].as_str())
                            .or_else(|| p["raw"].as_str())
                    })
                    .ok_or_else(|| format!("缺失第 {} 片上传地址", i))?;
                let resp = self
                    .client
                    .put(upload_url)
                    .body(chunk.to_vec())
                    .send()
                    .map_err(|e| format!("分片 {} 上传失败: {}", i, e))?;
                if !resp.status().is_success() {
                    return Err(format!("分片 {} 上传 HTTP {}", i, resp.status()));
                }
                // 更新 meta
                if let Some(ref mut m) = meta {
                    if !m.completed_parts.contains(&i) {
                        m.completed_parts.push(i);
                    }
                    let _ = std::fs::write(&meta_path, serde_json::to_string(m).unwrap_or_default());
                }
                auth_idx += 1;
            }
        }

        // 提交
        let commit_body = json!({ "task_id": task_id, "fid": obj_fid, "hash_enc": full_sha1 });
        let _ = self.api_post("file/upload/commit", &commit_body)?;

        // 完成
        let finish_body = json!({ "task_id": task_id, "fid": obj_fid });
        let _ = self.api_post("file/upload/finish", &finish_body)?;

        // 上传完成，删除 meta 文件
        let _ = std::fs::remove_file(&meta_path);

        Ok(())
    }

    fn download(&self, remote_path: &str, local_path: &PathBuf) -> Result<(), String> {
        let trimmed = remote_path.trim_start_matches('/');
        let fname = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();

        let fid = self
            .find_file_fid("0", &fname, 0)?
            .ok_or("云端未找到该文件")?;

        let dl_url = self.request_download_url(&fid)?;

        // 下载前备份本地文件（若存在），用于失败回滚
        let bak_path = local_path.with_extension("db.bak");
        let had_backup = if local_path.exists() {
            std::fs::copy(local_path, &bak_path)
                .map(|_| true)
                .unwrap_or(false)
        } else {
            false
        };

        // 执行下载，失败时回滚
        let download_result = (|| -> Result<(), String> {
            let resp = self
                .client
                .get(&dl_url)
                .send()
                .map_err(|e| format!("下载失败: {}", e))?;
            if !resp.status().is_success() {
                return Err(format!("下载失败: HTTP {}", resp.status()));
            }
            let bytes = resp
                .bytes()
                .map_err(|e| format!("读取下载内容失败: {}", e))?;
            std::fs::write(local_path, &bytes)
                .map_err(|e| format!("写入本地文件失败: {}", e))?;
            Ok(())
        })();

        if download_result.is_err() && had_backup {
            // 下载失败，回滚本地文件
            let _ = std::fs::copy(&bak_path, local_path);
        }

        download_result
    }

    fn remote_file_mtime(&self, remote_path: &str) -> Result<Option<i64>, String> {
        QuarkProvider::remote_file_mtime(self, remote_path)
    }
}

fn now_quark_query() -> String {
    let __dt = rand::random::<u32>() % 9900 + 100;
    format!(
        "uc_param_str=&__dt={}&__t={}",
        __dt,
        chrono::Local::now().timestamp_millis()
    )
}

/// URL 编码（用于 GET 请求的 query string 参数）
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}
