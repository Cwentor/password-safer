//! 追加式云同步的会话编排：push / pull / genesis / legacy 迁移 / 冲突挂起。
//!
//! 远端布局（base = legacy 密码簿 remote_path 的父目录）：
//! - `<base>/meta/meta_<UTC毫秒>_<device>.json`：提交点，唯一"最新状态"来源
//! - `<base>/blobs/<sha256>.bin`：不可变密文实体
//! - `<base>/vault.json`：legacy 整文件（迁移确认后删除）
//!
//! 两段式 push：先传 blob（中途崩溃只留孤儿 blob，无害，同内容秒传复用），
//! 再传 meta（提交点）。meta 传完而本地记账未更新的，下轮 list 会看到自己的
//! head 并收敛到 last_heads——全流程幂等。
//!
//! 冲突策略（拍板 v2）：引擎只挂起不裁决。本地有未推送修改且远端出现新 head、
//! 或远端 fork 时，本地内容先以 `kind:"backup"` 的 meta 钉进远端命名空间
//! （可被任何设备找回），`conflict_pending` 置位后自动同步周期直接跳过，
//! 等待用户在裁决 UI 里选择；手动上传/下载视为用户裁决（清挂起后执行）。

use super::engine::VaultIo;
use super::manifest::{self, FileEntry, Manifest, MetaKind};
use super::SyncProvider;
use crate::shared::storage::sync_state::SyncState;
use std::path::Path;

const LEGACY_FILE_NAME: &str = "vault.json";
const META_DIR_NAME: &str = "meta";
const BLOBS_DIR_NAME: &str = "blobs";

/// 本地同步记账接缝：生产实现走 sync_state.json，测试用内存替身
pub trait SyncStateIo: Send + Sync {
    fn load(&self) -> Result<SyncState, String>;
    fn save(&self, st: &SyncState) -> Result<(), String>;
}

/// 会话结局
#[derive(Debug, Clone, PartialEq)]
pub enum SessionOutcome {
    /// 产生了新 head
    Pushed,
    /// 应用了远端 head
    Applied,
    /// 无事可做
    Skipped,
    /// 冲突挂起，等待用户裁决
    ConflictPending,
    /// legacy 整文件远端迁移完成
    GenesisMigrated,
}

/// 会话报告（供引擎映射为 SyncReport）
#[derive(Debug, Clone)]
pub struct SessionReport {
    pub outcome: SessionOutcome,
    /// 展示细节（阶段 6 冲突事件与前端提示接入后消费）
    #[allow(dead_code)]
    pub message: String,
    /// 应用了新数据，前端需要刷新
    pub downloaded: bool,
}

/// 一次会话的输入
pub struct SessionInputs<'a> {
    pub provider: &'a dyn SyncProvider,
    pub vault: &'a dyn VaultIo,
    pub sync_state: &'a dyn SyncStateIo,
    /// legacy 密码簿远端路径（同时定义 base 目录）
    pub remote_path: &'a str,
    /// 网盘标识（"quark" / "baidu"，同步分账 key）
    pub provider_key: &'a str,
    /// 当前 unix 毫秒
    pub now_ms: i64,
}

// ---------- 路径派生 ----------

/// legacy 密码簿路径的父目录（base）
fn base_dir(remote_path: &str) -> String {
    let trimmed = remote_path.trim_start_matches('/');
    match trimmed.rfind('/') {
        Some(i) => format!("/{}", &trimmed[..i]),
        None => "/".to_string(),
    }
}

fn meta_dir(remote_path: &str) -> String {
    format!("{}/{}", base_dir(remote_path), META_DIR_NAME)
}

fn blobs_dir(remote_path: &str) -> String {
    format!("{}/{}", base_dir(remote_path), BLOBS_DIR_NAME)
}

fn meta_path(remote_path: &str, name: &str) -> String {
    format!("{}/{}", meta_dir(remote_path), name)
}

fn blob_path(remote_path: &str, sha: &str) -> String {
    format!("{}/{}.bin", blobs_dir(remote_path), sha)
}

// ---------- 远端 meta 读写 ----------

/// 拉取 /meta 目录全部 meta 并解析（跳过非 meta 命名与非法文件）
/// （会话内部与冲突详情命令共用）
pub fn fetch_metas(
    provider: &dyn SyncProvider,
    remote_path: &str,
) -> Result<Vec<(String, Manifest)>, String> {
    let mdir = meta_dir(remote_path);
    let files = provider.list(&mdir)?;
    let mut raw = Vec::new();
    for f in files {
        if manifest::parse_filename(&f.name).is_none() {
            continue;
        }
        let bytes = provider.download(&meta_path(remote_path, &f.name))?;
        raw.push((f.name, bytes));
    }
    Ok(manifest::parse_all(&raw))
}

fn head_vault_entry(metas: &[(String, Manifest)], head: &str) -> Option<FileEntry> {
    metas
        .iter()
        .find(|(n, _)| n == head)
        .and_then(|(_, m)| m.files.get(LEGACY_FILE_NAME).cloned())
}

/// 一组 head 中最新者的名字
fn newest_head_name(metas: &[(String, Manifest)], heads: &[String]) -> Option<String> {
    let head_metas: Vec<(String, Manifest)> = metas
        .iter()
        .filter(|(n, _)| heads.contains(n))
        .cloned()
        .collect();
    manifest::newest(&head_metas).map(|(n, _)| n.clone())
}

/// 一组 head 中最新者对应的 vault 条目
fn newest_head_vault_entry(metas: &[(String, Manifest)], heads: &[String]) -> Option<FileEntry> {
    let head_metas: Vec<(String, Manifest)> = metas
        .iter()
        .filter(|(n, _)| heads.contains(n))
        .cloned()
        .collect();
    manifest::newest(&head_metas)
        .and_then(|(_, m)| m.files.get(LEGACY_FILE_NAME).cloned())
}

/// 本地内容是否与任一已知 head 的 blob 一致（一致 = 本地干净）
fn local_matches_known(metas: &[(String, Manifest)], known: &[String], local_sha: &str) -> bool {
    known
        .iter()
        .any(|k| head_vault_entry(metas, k).map(|e| e.blob == local_sha).unwrap_or(false))
}

// ---------- 本地快照 ----------

/// 本地密码簿快照（读失败等价于本地无效）
struct LocalSnapshot {
    bytes: Option<Vec<u8>>,
    sha: Option<String>,
    valid: bool,
}

fn snapshot(i: &SessionInputs) -> LocalSnapshot {
    let bytes = i.vault.read().ok();
    let valid = bytes.as_ref().map(|b| !b.is_empty()).unwrap_or(false);
    let sha = bytes.as_ref().map(|b| manifest::sha256_hex(b));
    LocalSnapshot { bytes, sha, valid }
}

// ---------- 记账 ----------

fn known_heads(st: &SyncState, key: &str) -> Vec<String> {
    st.provider(key).map(|p| p.last_heads.clone()).unwrap_or_default()
}

fn set_heads(st: &mut SyncState, key: &str, heads: Vec<String>) {
    if let Some(p) = st.provider_mut(key) {
        p.last_heads = heads;
    }
}

// ---------- 远端写入 ----------

/// 上传 blob（两段式第一段；同内容重复上传由网盘秒传吸收，幂等）
fn upload_blob(i: &SessionInputs, local_path: &Path, sha: &str) -> Result<(), String> {
    i.provider.upload(local_path, &blob_path(i.remote_path, sha))
}

/// 进程内自增序号（临时文件名去重：并发会话/测试绝不共用同一个临时文件）
fn unique_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// 上传 meta（两段式第二段：提交点）。meta 字节先落临时文件再走 provider.upload
fn write_meta(i: &SessionInputs, m: &Manifest, name: &str) -> Result<(), String> {
    let bytes = m.serialize()?;
    let tmp = std::env::temp_dir().join(format!(
        "ps-meta-{}-{}-{}-{}.json",
        std::process::id(),
        m.device_id,
        m.updated_at,
        unique_seq()
    ));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写 meta 临时文件失败: {}", e))?;
    let result = i.provider.upload(&tmp, &meta_path(i.remote_path, name));
    let _ = std::fs::remove_file(&tmp);
    result
}

/// 生成并上传一个新 head（supersedes 由调用方给定）
fn push_new_head(
    i: &SessionInputs,
    st: &mut SyncState,
    heads: Vec<String>,
    local: &LocalSnapshot,
) -> Result<SessionReport, String> {
    let sha = local
        .sha
        .clone()
        .ok_or_else(|| "本地密码簿为空，无法上传".to_string())?;
    let bytes = local.bytes.as_ref().unwrap();
    // 第一段：blob（幂等，孤儿无害）
    upload_blob(i, i.vault.path(), &sha)?;
    // 第二段：meta = 提交点
    let mut m = Manifest::new_head(&st.device_id, i.now_ms);
    m.supersedes = heads.clone();
    m.files.insert(
        LEGACY_FILE_NAME.to_string(),
        FileEntry {
            blob: sha,
            size: bytes.len() as i64,
            updated_at: i.now_ms,
            deleted: false,
        },
    );
    let name = manifest::new_filename(&st.device_id, i.now_ms);
    write_meta(i, &m, &name)?;

    set_heads(st, i.provider_key, vec![name]);
    st.conflict_pending = None;
    i.sync_state.save(st)?;
    Ok(SessionReport {
        outcome: SessionOutcome::Pushed,
        message: "已上传最新版本".to_string(),
        downloaded: false,
    })
}

/// 把任意字节以 blob + `kind:"backup"` meta 钉进远端命名空间（不抢 head、可寻址）。
/// 冲突挂起的本地备份、legacy 迁移前的旧文件归档共用此入口。
/// 把任意字节以 blob + `kind:"backup"` meta 钉进远端命名空间（不抢 head、可寻址）。
/// 冲突挂起的本地备份、legacy 迁移前的旧文件归档共用此入口。
/// blob 从给定字节经临时文件上传（不假设它等于本地密码簿文件）；
/// backup meta 文件名加进程内序号，避免同一会话内与 head meta 同毫秒撞名。
fn pin_bytes_as_backup(
    i: &SessionInputs,
    st: &SyncState,
    bytes: &[u8],
) -> Result<String, String> {
    let sha = manifest::sha256_hex(bytes);
    let tmp = std::env::temp_dir().join(format!(
        "ps-pin-{}-{}-{}.bin",
        std::process::id(),
        st.device_id,
        unique_seq()
    ));
    std::fs::write(&tmp, bytes).map_err(|e| format!("写备份临时文件失败: {}", e))?;
    let up = upload_blob(i, &tmp, &sha);
    let _ = std::fs::remove_file(&tmp);
    up?;

    // backup meta 的 ms 加序号扰动：与本会话稍后写出的 head meta 永不重名
    let backup_ms = i.now_ms + unique_seq() as i64;
    let mut m = Manifest::new_head(&st.device_id, backup_ms);
    m.kind = MetaKind::Backup;
    m.files.insert(
        LEGACY_FILE_NAME.to_string(),
        FileEntry {
            blob: sha.clone(),
            size: bytes.len() as i64,
            updated_at: i.now_ms,
            deleted: false,
        },
    );
    let name = manifest::new_filename(&st.device_id, backup_ms);
    write_meta(i, &m, &name)?;
    Ok(sha)
}

/// 冲突挂起：本地内容（若有）先以 backup meta 钉进远端命名空间，再置 pending
fn park(
    i: &SessionInputs,
    st: &mut SyncState,
    heads: Vec<String>,
    local: &LocalSnapshot,
    reason: &str,
) -> Result<SessionReport, String> {
    let mut backup_blob = None;
    if local.valid {
        let sha = pin_bytes_as_backup(i, st, local.bytes.as_ref().unwrap())?;
        backup_blob = Some(sha);
    }
    st.conflict_pending = Some(serde_json::json!({
        "provider": i.provider_key,
        "remote_heads": heads,
        "backup_blob": backup_blob,
        "detected_at": i.now_ms,
        "reason": reason,
    }));
    i.sync_state.save(st)?;
    Ok(SessionReport {
        outcome: SessionOutcome::ConflictPending,
        message: format!("{}：自动同步已暂停，待处理", reason),
        downloaded: false,
    })
}

// ---------- legacy 迁移 ----------

/// 删除 legacy 整文件；失败置补删标记（下轮重试），不阻塞主流程。
/// 调用前必须已确认 genesis head 可被 list 到（提交点先行，破坏性动作在后）。
fn delete_legacy(i: &SessionInputs, st: &mut SyncState) {
    match i.provider.delete(i.remote_path) {
        Ok(()) => {
            st.legacy_pending_delete = false;
            eprintln!("[session] legacy 远端密码簿已删除");
        }
        Err(e) => {
            st.legacy_pending_delete = true;
            eprintln!("[session] legacy 删除失败（已置补删标记）: {}", e);
        }
    }
}

/// 机会性补删：上轮删除失败的 legacy，本轮再试
fn try_pending_legacy_delete(i: &SessionInputs, st: &mut SyncState) {
    if st.legacy_pending_delete {
        delete_legacy(i, st);
    }
}

/// 确认本设备刚写入的 head meta 已可被 list 到（网盘 eventual consistency 兜底，
/// 评审 C2：提交点先行，破坏性动作在后）
fn confirm_head_visible(i: &SessionInputs, name: &str) -> bool {
    match i.provider.list(&meta_dir(i.remote_path)) {
        Ok(files) => files.iter().any(|f| f.name == name),
        Err(e) => {
            eprintln!("[session] 确认 head 可见性失败（按未见处理）: {}", e);
            false
        }
    }
}

/// 远端尚无 head 时的 genesis / legacy 迁移（§2.7）
fn genesis(
    i: &SessionInputs,
    st: &mut SyncState,
    local: &LocalSnapshot,
) -> Result<SessionReport, String> {
    let base = base_dir(i.remote_path);
    let base_files = i.provider.list(&base)?;
    let legacy = base_files.iter().find(|f| f.name == LEGACY_FILE_NAME);

    match (local.valid, legacy.is_some()) {
        (false, false) => Ok(SessionReport {
            outcome: SessionOutcome::Skipped,
            message: "云端与本地均为空".to_string(),
            downloaded: false,
        }),
        (false, true) => adopt_legacy(i, st),
        (true, false) => {
            // 纯净首推
            push_new_head(i, st, Vec::new(), local)
        }
        (true, true) => {
            // 本地与 legacy 都有数据：mtime 新者先落地（规格沉默处的务实裁决）
            let legacy_mtime = legacy.unwrap().mtime;
            let local_mtime = i.vault.local_mtime().unwrap_or(0);
            if legacy_mtime > local_mtime {
                adopt_legacy(i, st)
            } else {
                // 评审 C3：本地胜出前，先把 legacy 字节归档为可找回的 backup，
                // 再 push genesis → 确认可见 → 删除 legacy（破坏性动作最后）
                let legacy_bytes = i.provider.download(i.remote_path)?;
                pin_bytes_as_backup(i, st, &legacy_bytes)?;
                let r = push_new_head(i, st, Vec::new(), local)?;
                let genesis_name = st
                    .provider(i.provider_key)
                    .and_then(|p| p.last_heads.first().cloned())
                    .unwrap_or_default();
                if confirm_head_visible(i, &genesis_name) {
                    delete_legacy(i, st);
                } else {
                    st.legacy_pending_delete = true;
                    eprintln!("[session] genesis 未确认可见，legacy 留待下轮补删");
                }
                i.sync_state.save(st)?;
                Ok(SessionReport {
                    outcome: SessionOutcome::GenesisMigrated,
                    message: "本机版本较新，已接管云端并清理旧文件".to_string(),
                    downloaded: r.downloaded,
                })
            }
        }
    }
}

/// 收养 legacy：下载 → 验证 → 应用 → genesis push → 确认可见 → 删除 legacy
fn adopt_legacy(i: &SessionInputs, st: &mut SyncState) -> Result<SessionReport, String> {
    let bytes = i.provider.download(i.remote_path)?;
    let sha = manifest::sha256_hex(&bytes);
    i.vault.replace(&bytes)?;
    let local = LocalSnapshot {
        bytes: Some(bytes.clone()),
        sha: Some(sha),
        valid: true,
    };
    push_new_head(i, st, Vec::new(), &local)?;
    let genesis_name = st
        .provider(i.provider_key)
        .and_then(|p| p.last_heads.first().cloned())
        .unwrap_or_default();
    if confirm_head_visible(i, &genesis_name) {
        delete_legacy(i, st);
    } else {
        st.legacy_pending_delete = true;
        eprintln!("[session] genesis 未确认可见，legacy 留待下轮补删");
    }
    i.sync_state.save(st)?;
    Ok(SessionReport {
        outcome: SessionOutcome::GenesisMigrated,
        message: "已从云端旧版密码簿完成迁移".to_string(),
        downloaded: true,
    })
}

// ---------- 应用远端 head ----------

/// 下载 head 的 vault blob，校验哈希后原子替换本地（两把锁：sha 校验 + replace_raw 验密）
fn apply_head(
    i: &SessionInputs,
    st: &mut SyncState,
    metas: &[(String, Manifest)],
    head: &str,
    heads: Vec<String>,
) -> Result<SessionReport, String> {
    let entry = head_vault_entry(metas, head)
        .ok_or_else(|| format!("head meta {} 缺少 {} 条目", head, LEGACY_FILE_NAME))?;
    if entry.deleted {
        set_heads(st, i.provider_key, heads);
        st.conflict_pending = None;
        i.sync_state.save(st)?;
        return Ok(SessionReport {
            outcome: SessionOutcome::Skipped,
            message: "云端最新版本为已删除状态".to_string(),
            downloaded: false,
        });
    }
    let bytes = i.provider.download(&blob_path(i.remote_path, &entry.blob))?;
    let sha = manifest::sha256_hex(&bytes);
    if sha != entry.blob {
        return Err(format!("远端 blob 校验失败（期望 {}）", entry.blob));
    }
    if let Err(e) = i.vault.replace(&bytes) {
        // 本地钥解不开云端新版本：库密码可能已在其他设备更换（评审 A1）→
        // 转冲突挂起，走"输入云端库密码接管"路径，而不是报错死循环
        if e.contains("解密失败") {
            let local = snapshot(i);
            return park(
                i,
                st,
                heads,
                &local,
                "本地无法解密云端新版本（库密码可能已在其他设备更换）",
            );
        }
        return Err(e);
    }
    set_heads(st, i.provider_key, heads);
    st.conflict_pending = None;
    i.sync_state.save(st)?;
    Ok(SessionReport {
        outcome: SessionOutcome::Applied,
        message: "已应用云端最新版本".to_string(),
        downloaded: true,
    })
}

// ---------- 三大入口 ----------

/// 自动同步（调度周期）：收敛判定 → push / apply / 挂起。
/// 判定矩阵：
/// - 挂起中 → 跳过（等裁决，不发网络请求）
/// - 远端无 head → genesis / legacy 迁移
/// - 远端 fork（>1 head）→ 挂起
/// - 本地内容 == 最新 head blob → 收敛 skip
/// - 本地相对已知 head 有未推送修改：
///   - 远端无新 head → 正常 push
///   - 远端有新 head → 冲突挂起
/// - 本地干净 → 应用远端新 head
pub fn sync_auto(i: &SessionInputs) -> Result<SessionReport, String> {
    let mut st = i.sync_state.load()?;
    try_pending_legacy_delete(i, &mut st);
    i.sync_state.save(&st)?;

    if st.conflict_pending.is_some() {
        return Ok(SessionReport {
            outcome: SessionOutcome::ConflictPending,
            message: "存在待处理的同步冲突，自动同步已暂停".to_string(),
            downloaded: false,
        });
    }

    let metas = fetch_metas(i.provider, i.remote_path)?;
    let heads = manifest::resolve_heads(&metas);
    let known = known_heads(&st, i.provider_key);
    let local = snapshot(i);

    if heads.is_empty() {
        return genesis(i, &mut st, &local);
    }
    if heads.len() > 1 {
        return park(i, &mut st, heads, &local, "检测到远端版本分叉");
    }
    let head = heads[0].clone();

    // 内容一致性：本地与最新 head 相同 → 收敛（补齐 last_heads 视角）
    let head_entry = head_vault_entry(&metas, &head)
        .ok_or_else(|| format!("head meta {} 缺少 {} 条目", head, LEGACY_FILE_NAME))?;
    if local.valid && local.sha.as_deref() == Some(head_entry.blob.as_str()) {
        set_heads(&mut st, i.provider_key, heads);
        st.conflict_pending = None;
        i.sync_state.save(&st)?;
        return Ok(SessionReport {
            outcome: SessionOutcome::Skipped,
            message: "云端与本地一致".to_string(),
            downloaded: false,
        });
    }

    // 本地是否有未推送修改：相对已知 head 而言
    let local_dirty = local.valid && !local_matches_known(&metas, &known, local.sha.as_ref().unwrap());

    if local_dirty {
        if unknown_heads(&heads, &known).is_empty() {
            // 远端无新东西 → 正常 push（supersedes 当前 head）
            return push_new_head(i, &mut st, heads, &local);
        }
        // 两边都动了 → 挂起
        return park(i, &mut st, heads, &local, "本地与云端均有未合并的修改");
    }

    // 本地干净（或为空）：应用远端最新
    apply_head(i, &mut st, &metas, &head, heads)
}

fn unknown_heads(heads: &[String], known: &[String]) -> Vec<String> {
    heads
        .iter()
        .filter(|h| !known.contains(h))
        .cloned()
        .collect()
}

/// 手动上传：内容与云端最新一致则提示无需上传；
/// 否则视为用户裁决（清挂起），强制生成新 head（supersedes 全部现 heads）
pub fn push_manual(i: &SessionInputs) -> Result<SessionReport, String> {
    let mut st = i.sync_state.load()?;
    try_pending_legacy_delete(i, &mut st);
    i.sync_state.save(&st)?;

    let metas = fetch_metas(i.provider, i.remote_path)?;
    let heads = manifest::resolve_heads(&metas);
    let local = snapshot(i);
    if !local.valid {
        return Err("本地密码簿为空，无法上传".to_string());
    }
    if let Some(entry) = newest_head_vault_entry(&metas, &heads) {
        if entry.blob == local.sha.clone().unwrap() {
            set_heads(&mut st, i.provider_key, heads);
            st.conflict_pending = None;
            i.sync_state.save(&st)?;
            return Ok(SessionReport {
                outcome: SessionOutcome::Skipped,
                message: "云端已是最新内容".to_string(),
                downloaded: false,
            });
        }
    }
    push_new_head(i, &mut st, heads, &local)
}

/// 手动下载：视为用户裁决（清挂起）。本地若有与目标不同的内容，
/// 先以 backup meta 备份再应用最新 head（"覆盖"但可找回）
pub fn pull_manual(i: &SessionInputs) -> Result<SessionReport, String> {
    let mut st = i.sync_state.load()?;
    try_pending_legacy_delete(i, &mut st);
    i.sync_state.save(&st)?;

    let metas = fetch_metas(i.provider, i.remote_path)?;
    let heads = manifest::resolve_heads(&metas);
    if heads.is_empty() {
        let local = snapshot(i);
        return genesis(i, &mut st, &local);
    }
    let head = newest_head_name(&metas, &heads).ok_or_else(|| "未找到最新 head".to_string())?;

    let local = snapshot(i);
    if local.valid {
        let target = head_vault_entry(&metas, &head);
        let differs = target
            .map(|e| e.blob != local.sha.clone().unwrap())
            .unwrap_or(true);
        if differs {
            // 本地内容先备份（不抢 head）
            park_backup_only(i, &st, &local)?;
        }
    }
    apply_head(i, &mut st, &metas, &head, heads)
}

/// 只写 backup meta（不置 pending）——手动下载前的自救
fn park_backup_only(i: &SessionInputs, st: &SyncState, local: &LocalSnapshot) -> Result<(), String> {
    let bytes = local.bytes.as_ref().ok_or("本地密码簿为空")?;
    pin_bytes_as_backup(i, st, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    const NOW_MS: i64 = 1_700_000_000_000;

    /// 测试用唯一序号（并行测试的临时文件/目录绝不重名）
    fn unique_seq() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    /// 模拟"仅新增"的远端：同名不同内容拒绝（模拟 23008），同内容幂等成功（模拟秒传）
    struct FakeCloud {
        files: Mutex<HashMap<String, (Vec<u8>, i64)>>,
    }

    impl FakeCloud {
        fn new() -> Self {
            FakeCloud {
                files: Mutex::new(HashMap::new()),
            }
        }
        fn put(&self, path: &str, bytes: &[u8]) {
            self.put_with_mtime(path, bytes, 1);
        }
        fn put_with_mtime(&self, path: &str, bytes: &[u8], mtime: i64) {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), (bytes.to_vec(), mtime));
        }
        fn get(&self, path: &str) -> Option<Vec<u8>> {
            self.files.lock().unwrap().get(path).map(|(b, _)| b.clone())
        }
        fn has(&self, path: &str) -> bool {
            self.files.lock().unwrap().contains_key(path)
        }
        fn count_in_dir(&self, dir: &str) -> usize {
            self.files
                .lock()
                .unwrap()
                .keys()
                .filter(|p| Self::parent(p) == dir)
                .count()
        }
        fn parent(path: &str) -> String {
            let t = path.trim_start_matches('/');
            match t.rfind('/') {
                Some(i) => format!("/{}", &t[..i]),
                None => "/".to_string(),
            }
        }
    }

    impl SyncProvider for FakeCloud {
        fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), String> {
            let data =
                std::fs::read(local_path).map_err(|e| format!("读取本地文件失败: {}", e))?;
            let mut files = self.files.lock().unwrap();
            if let Some((existing, _)) = files.get(remote_path) {
                if existing == &data {
                    return Ok(()); // 同内容幂等
                }
                return Err("23008 同名冲突（模拟不可覆盖）".to_string());
            }
            files.insert(remote_path.to_string(), (data, 1));
            Ok(())
        }
        fn download(&self, remote_path: &str) -> Result<Vec<u8>, String> {
            self.files
                .lock()
                .unwrap()
                .get(remote_path)
                .map(|(b, _)| b.clone())
                .ok_or_else(|| "云端未找到该文件".to_string())
        }
        fn list(&self, remote_dir: &str) -> Result<Vec<crate::shared::sync::RemoteFile>, String> {
            let files = self.files.lock().unwrap();
            let mut out = Vec::new();
            for (path, (_, mtime)) in files.iter() {
                if Self::parent(path) == remote_dir {
                    let name = path.rsplit('/').next().unwrap_or(path).to_string();
                    out.push(crate::shared::sync::RemoteFile {
                        name,
                        size: 0,
                        mtime: *mtime,
                    });
                }
            }
            Ok(out)
        }
        fn delete(&self, remote_path: &str) -> Result<(), String> {
            self.files.lock().unwrap().remove(remote_path);
            Ok(())
        }
    }

    /// 真实临时文件的密码簿替身（upload 需要读本地文件）
    struct TestVault {
        path: PathBuf,
    }

    impl TestVault {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "ps-sess-{}-{}-{}-{}",
                std::process::id(),
                tag,
                nanos,
                unique_seq()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TestVault {
                path: dir.join("vault.json"),
            }
        }
        fn write(&self, bytes: &[u8]) {
            std::fs::write(&self.path, bytes).unwrap();
        }
    }

    impl VaultIo for TestVault {
        fn path(&self) -> &Path {
            &self.path
        }
        fn local_mtime(&self) -> Option<i64> {
            std::fs::metadata(&self.path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
        }
        fn read(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.path).map_err(|e| e.to_string())
        }
        fn replace(&self, bytes: &[u8]) -> Result<(), String> {
            std::fs::write(&self.path, bytes).map_err(|e| e.to_string())
        }
    }

    /// 内存记账替身
    struct MemState(Mutex<SyncState>);

    impl MemState {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "ps-sess-state-{}-{}.json",
                std::process::id(),
                tag
            ));
            MemState(Mutex::new(SyncState::load_or_create(p).unwrap()))
        }
        fn device_id(&self) -> String {
            self.0.lock().unwrap().device_id.clone()
        }
    }

    impl SyncStateIo for MemState {
        fn load(&self) -> Result<SyncState, String> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, st: &SyncState) -> Result<(), String> {
            *self.0.lock().unwrap() = st.clone();
            Ok(())
        }
    }

    fn inputs<'a>(
        cloud: &'a FakeCloud,
        vault: &'a TestVault,
        state: &'a MemState,
        now_ms: i64,
    ) -> SessionInputs<'a> {
        SessionInputs {
            provider: cloud,
            vault,
            sync_state: state,
            remote_path: "/VAULT/vault.json",
            provider_key: "quark",
            now_ms,
        }
    }

    /// 直接以"另一设备"身份向远端写一个 head（绕过本机会话）
    fn remote_push(cloud: &FakeCloud, device: &str, ms: i64, supersedes: &[String], content: &[u8]) {
        let sha = manifest::sha256_hex(content);
        cloud.put(&blob_path("/VAULT/vault.json", &sha), content);
        let mut m = Manifest::new_head(device, ms);
        m.supersedes = supersedes.to_vec();
        m.files.insert(
            "vault.json".to_string(),
            FileEntry {
                blob: sha,
                size: content.len() as i64,
                updated_at: ms,
                deleted: false,
            },
        );
        let name = manifest::new_filename(device, ms);
        let tmp = std::env::temp_dir().join(format!(
            "ps-test-push-{}-{}-{}.json",
            device,
            ms,
            unique_seq()
        ));
        std::fs::write(&tmp, m.serialize().unwrap()).unwrap();
        cloud.upload(&tmp, &meta_path("/VAULT/vault.json", &name)).unwrap();
        let _ = std::fs::remove_file(&tmp);
    }

    fn meta_dir_of() -> String {
        meta_dir("/VAULT/vault.json")
    }

    // ---- genesis / legacy 迁移 ----

    #[test]
    fn genesis_push_fresh_remote() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("genesis");
        vault.write(b"local v1");
        let state = MemState::new("genesis");

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Pushed);

        let dev = state.device_id();
        let meta_name = format!("meta_{}_{}.json", NOW_MS, dev);
        let meta = Manifest::parse(
            &cloud.get(&format!("{}/{}", meta_dir_of(), meta_name)).unwrap(),
        )
        .unwrap();
        let sha = manifest::sha256_hex(b"local v1");
        assert_eq!(meta.files.get("vault.json").unwrap().blob, sha);
        assert!(meta.supersedes.is_empty());
        assert!(cloud.has(&format!("{}/{}.bin", blobs_dir("/VAULT/vault.json"), sha)));
        assert_eq!(state.0.lock().unwrap().quark.last_heads, vec![meta_name]);
        assert!(!state.0.lock().unwrap().legacy_pending_delete);
    }

    #[test]
    fn genesis_adopt_legacy_when_local_invalid() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("adopt"); // 本地无文件
        let state = MemState::new("adopt");
        cloud.put_with_mtime("/VAULT/vault.json", b"legacy bytes", 100);

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::GenesisMigrated);
        assert!(r.downloaded);
        assert_eq!(vault.read().unwrap(), b"legacy bytes");
        // legacy 已删除
        assert!(!cloud.has("/VAULT/vault.json"));
        // genesis head 指向 legacy 内容
        let dev = state.device_id();
        let meta = Manifest::parse(
            &cloud
                .get(&format!("{}/meta_{}_{}.json", meta_dir_of(), NOW_MS, dev))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            meta.files.get("vault.json").unwrap().blob,
            manifest::sha256_hex(b"legacy bytes")
        );
    }

    #[test]
    fn genesis_local_wins_when_legacy_older() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("localwin");
        vault.write(b"local newer");
        let state = MemState::new("localwin");
        cloud.put_with_mtime("/VAULT/vault.json", b"legacy old", 1);

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::GenesisMigrated);
        assert_eq!(vault.read().unwrap(), b"local newer");
        assert!(!cloud.has("/VAULT/vault.json"));
    }

    #[test]
    fn legacy_pending_delete_retried_next_cycle() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("relegacy");
        vault.write(b"v1");
        let state = MemState::new("relegacy");
        cloud.put("/VAULT/vault.json", b"legacy");
        state.0.lock().unwrap().legacy_pending_delete = true;

        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();
        // 补删完成
        assert!(!cloud.has("/VAULT/vault.json"));
        assert!(!state.0.lock().unwrap().legacy_pending_delete);
    }

    // ---- 常规 push / pull / skip ----

    #[test]
    fn second_cycle_skips_when_nothing_changed() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("skip2");
        vault.write(b"v1");
        let state = MemState::new("skip2");

        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();
        let metas_before = cloud.count_in_dir(&meta_dir_of());

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 1000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Skipped);
        assert_eq!(cloud.count_in_dir(&meta_dir_of()), metas_before);
    }

    #[test]
    fn pull_applies_new_remote_head_when_local_clean() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("pull");
        vault.write(b"v1");
        let state = MemState::new("pull");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let head1 = state.0.lock().unwrap().quark.last_heads[0].clone();
        remote_push(&cloud, "other", NOW_MS + 1000, &[head1], b"v2");

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 2000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Applied);
        assert!(r.downloaded);
        assert_eq!(vault.read().unwrap(), b"v2");
        assert_eq!(state.0.lock().unwrap().quark.last_heads.len(), 1);
    }

    #[test]
    fn push_new_content_when_remote_unchanged() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("push2");
        vault.write(b"v1");
        let state = MemState::new("push2");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        vault.write(b"v2 local edit");
        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 1000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Pushed);
        // 链条：head2 supersedes head1
        let dev = state.device_id();
        let meta2 = Manifest::parse(
            &cloud
                .get(&format!("{}/meta_{}_{}.json", meta_dir_of(), NOW_MS + 1000, dev))
                .unwrap(),
        )
        .unwrap();
        let head1 = format!("meta_{}_{}.json", NOW_MS, dev);
        assert_eq!(meta2.supersedes, vec![head1]);
        // 本地内容不被拉取覆盖
        assert_eq!(vault.read().unwrap(), b"v2 local edit");
    }

    #[test]
    fn crash_after_blob_before_meta_recovers() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("orphan");
        vault.write(b"v1");
        let state = MemState::new("orphan");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        vault.write(b"v2");
        // 模拟崩溃：只留下孤儿 blob，没有 meta
        let sha2 = manifest::sha256_hex(b"v2");
        cloud.put(&blob_path("/VAULT/vault.json", &sha2), b"v2");

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 1000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Pushed);
        assert!(cloud.has(&format!("{}/{}.bin", blobs_dir("/VAULT/vault.json"), sha2)));
    }

    #[test]
    fn multi_provider_isolation() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("multi");
        vault.write(b"v1");
        let state = MemState::new("multi");

        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();
        assert!(!state.0.lock().unwrap().quark.last_heads.is_empty());
        assert!(state.0.lock().unwrap().baidu.last_heads.is_empty());
    }

    // ---- 冲突挂起 ----

    #[test]
    fn conflict_parks_when_both_sides_changed() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("conflict");
        vault.write(b"v1");
        let state = MemState::new("conflict");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let head1 = state.0.lock().unwrap().quark.last_heads[0].clone();
        remote_push(&cloud, "other", NOW_MS + 1000, &[head1], b"v2");
        vault.write(b"local edit");

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 2000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::ConflictPending);
        // 本地内容未被覆盖
        assert_eq!(vault.read().unwrap(), b"local edit");
        // 备份已钉进远端（backup meta 文件名带序号，按内容查找；blob 为本地内容哈希）
        let local_sha = manifest::sha256_hex(b"local edit");
        assert!(cloud.has(&format!("{}/{}.bin", blobs_dir("/VAULT/vault.json"), local_sha)));
        let backup_found = cloud
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| FakeCloud::parent(p) == meta_dir_of())
            .filter_map(|(p, (b, _))| {
                Manifest::parse(b)
                    .ok()
                    .map(|m| (p.clone(), m))
            })
            .any(|(_, m)| m.kind == MetaKind::Backup && m.files.get("vault.json").map(|e| e.blob == local_sha).unwrap_or(false));
        assert!(backup_found, "应存在指向本地内容的 backup meta");
        // 挂起状态置位
        assert!(state.0.lock().unwrap().conflict_pending.is_some());
    }

    #[test]
    fn fork_parks_even_when_local_clean() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("fork");
        vault.write(b"v1");
        let state = MemState::new("fork");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let head1 = state.0.lock().unwrap().quark.last_heads[0].clone();
        remote_push(&cloud, "devA", NOW_MS + 1000, &[head1.clone()], b"vA");
        remote_push(&cloud, "devB", NOW_MS + 1000, &[head1], b"vB");

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 2000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::ConflictPending);
        assert_eq!(vault.read().unwrap(), b"v1");
    }

    #[test]
    fn parked_cycle_skips_without_new_writes() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("parked");
        vault.write(b"v1");
        let state = MemState::new("parked");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let head1 = state.0.lock().unwrap().quark.last_heads[0].clone();
        remote_push(&cloud, "other", NOW_MS + 1000, &[head1], b"v2");
        vault.write(b"local edit");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 2000)).unwrap();
        let metas_before = cloud.count_in_dir(&meta_dir_of());

        // 挂起中的周期：不再产生新的远端写入
        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 3000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::ConflictPending);
        assert_eq!(cloud.count_in_dir(&meta_dir_of()), metas_before);
    }

    // ---- 完整性校验 ----

    #[test]
    fn apply_rejects_corrupt_blob() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("corrupt");
        vault.write(b"v1");
        let state = MemState::new("corrupt");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let head1 = state.0.lock().unwrap().quark.last_heads[0].clone();
        // 另一设备声明 blob=sha(b"v2")，但实际存的是坏字节
        let sha2 = manifest::sha256_hex(b"v2");
        cloud.put(&blob_path("/VAULT/vault.json", &sha2), b"corrupted bytes");
        let mut m2 = Manifest::new_head("other", NOW_MS + 1000);
        m2.supersedes = vec![head1];
        m2.files.insert(
            "vault.json".to_string(),
            FileEntry {
                blob: sha2,
                size: 15,
                updated_at: NOW_MS + 1000,
                deleted: false,
            },
        );
        let name = manifest::new_filename("other", NOW_MS + 1000);
        let tmp = std::env::temp_dir().join(format!("ps-test-corrupt-meta-{}.json", unique_seq()));
        std::fs::write(&tmp, m2.serialize().unwrap()).unwrap();
        cloud
            .upload(&tmp, &meta_path("/VAULT/vault.json", &name))
            .unwrap();
        let _ = std::fs::remove_file(&tmp);

        let r = sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 2000));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("校验失败"));
        // 本地未被破坏
        assert_eq!(vault.read().unwrap(), b"v1");
    }

    // ---- 手动同步（视为用户裁决） ----

    #[test]
    fn manual_download_backs_up_then_applies() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("mdl");
        vault.write(b"v1");
        let state = MemState::new("mdl");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let head1 = state.0.lock().unwrap().quark.last_heads[0].clone();
        remote_push(&cloud, "other", NOW_MS + 1000, &[head1], b"v2");
        vault.write(b"local edit");

        let r = pull_manual(&inputs(&cloud, &vault, &state, NOW_MS + 2000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Applied);
        assert_eq!(vault.read().unwrap(), b"v2");
        // 本地修改有备份
        let local_sha = manifest::sha256_hex(b"local edit");
        assert!(cloud.has(&format!("{}/{}.bin", blobs_dir("/VAULT/vault.json"), local_sha)));
        // 挂起被清除
        assert!(state.0.lock().unwrap().conflict_pending.is_none());
    }

    #[test]
    fn manual_upload_resolves_pending_and_pushes() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("mul");
        vault.write(b"v1");
        let state = MemState::new("mul");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let head1 = state.0.lock().unwrap().quark.last_heads[0].clone();
        remote_push(&cloud, "other", NOW_MS + 1000, &[head1.clone()], b"v2");
        vault.write(b"local edit");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS + 2000)).unwrap();
        assert!(state.0.lock().unwrap().conflict_pending.is_some());

        let r = push_manual(&inputs(&cloud, &vault, &state, NOW_MS + 3000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Pushed);
        // 挂起清除，新 head supersedes 两个已知 head
        assert!(state.0.lock().unwrap().conflict_pending.is_none());
        let dev = state.device_id();
        let meta3 = Manifest::parse(
            &cloud
                .get(&format!("{}/meta_{}_{}.json", meta_dir_of(), NOW_MS + 3000, dev))
                .unwrap(),
        )
        .unwrap();
        let head2 = format!("meta_{}_{}.json", NOW_MS + 1000, "other");
        // backup 不算 head、head1 已被 head2 supersed → 集合里只剩 head2
        assert_eq!(meta3.supersedes, vec![head2]);
    }

    #[test]
    fn manual_upload_skips_when_cloud_already_newest() {
        let cloud = FakeCloud::new();
        let vault = TestVault::new("mup");
        vault.write(b"v1");
        let state = MemState::new("mup");
        sync_auto(&inputs(&cloud, &vault, &state, NOW_MS)).unwrap();

        let r = push_manual(&inputs(&cloud, &vault, &state, NOW_MS + 1000)).unwrap();
        assert_eq!(r.outcome, SessionOutcome::Skipped);
        assert_eq!(r.message, "云端已是最新内容");
    }
}
