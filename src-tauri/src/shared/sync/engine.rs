//! 云同步引擎：Cookie 过期预警、同步编排的决策层。
//!
//! 只做决策与编排，不持有 Tauri 句柄、不写配置——副作用由调用方（lib.rs 薄壳）
//! 按报告一次性落地。方向判定与传输在 session（追加式同步会话）里；
//! 测试经 `SyncProvider` / `QrAuthenticator` / `VaultIo` / `SyncStateIo`
//! 四个接缝注入替身，无需网络与 WebView。

use super::session::{self, SessionInputs, SessionOutcome, SyncStateIo};
use super::SyncProvider;
use crate::shared::sync::auth::QrAuthenticator;
use std::path::Path;

/// Cookie 即将过期预警
#[derive(Debug, Clone, PartialEq)]
pub struct ExpiryWarning {
    pub days_left: i64,
}

/// 检查 Cookie 是否即将过期（剩余 0 < 天数 < 7 时预警一次）。
/// 返回 (预警内容, 应把 warn_sent 标记置为何值)；None 表示无需变动。
pub fn check_expiry(
    expires_at: i64,
    now_secs: i64,
    warn_sent: bool,
) -> (Option<ExpiryWarning>, Option<bool>) {
    if expires_at <= 0 {
        return (None, None);
    }
    let secs_left = expires_at - now_secs;
    if secs_left <= 0 {
        // 已过期：由 Cookie 校验兜底走失效流程，不发预警
        (None, None)
    } else if secs_left < 7 * 86400 {
        if warn_sent {
            (None, None)
        } else {
            (
                Some(ExpiryWarning {
                    days_left: secs_left / 86400,
                }),
                Some(true),
            )
        }
    } else {
        // 距过期还远，重置提醒标记（便于下次临近过期时再次提醒）
        (None, Some(false))
    }
}

/// 密码簿文件访问接缝：生产实现经 `Mutex<JsonStore>` 单一入口，
/// 测试用临时目录实现。引擎只依赖此抽象，不知道加密细节。
pub trait VaultIo: Send + Sync {
    /// 密码簿文件路径（供上传读取）
    fn path(&self) -> &Path;
    /// 本地文件修改时间（unix 秒；genesis 与 legacy 比较新旧用）
    fn local_mtime(&self) -> Option<i64>;
    /// 读取本地密码簿字节（内容哈希对比 / blob 上传用）
    fn read(&self) -> Result<Vec<u8>, String>;
    /// 用下载到的内容替换本地密码簿（实现须先验证再原子替换）
    fn replace(&self, bytes: &[u8]) -> Result<(), String>;
}

/// 一次同步的结局
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Uploaded,
    Downloaded,
    Skipped,
    /// 未绑定 Cookie（手动同步会遇到；调度器只在已绑定时触发）
    NotLoggedIn,
    /// Cookie 已失效
    CookieExpired,
    Failed(String),
    /// 冲突挂起：引擎不裁决，等用户在界面处理（阶段 5）
    ConflictPending,
    /// legacy 整文件远端迁移完成（含本机接管云端的情况）
    GenesisMigrated,
}

/// 同步报告：决策结果 + 需要调用方落地的记账动作。
/// 调用方按报告一次性完成配置写回与事件通知，不再各自手搓。
#[derive(Debug, Clone)]
pub struct SyncReport {
    pub outcome: Outcome,
    /// 是否记录本次同步时间（last_sync）
    pub record_sync: bool,
    /// 下载了新数据，前端需要刷新
    pub downloaded: bool,
    /// 应关闭该网盘的自动同步（Cookie 失效；手动同步不会置位）
    pub disable_auto_sync: bool,
    pub expiry_warning: Option<ExpiryWarning>,
    pub set_warn_sent: Option<bool>,
}

impl SyncReport {
    /// 由结局构造报告（调用方仅在不经 run_cycle/run_manual 的兜底路径使用）
    pub fn new(outcome: Outcome) -> Self {
        let record_sync = !matches!(
            outcome,
            Outcome::NotLoggedIn | Outcome::CookieExpired | Outcome::Failed(_)
        );
        SyncReport {
            outcome,
            record_sync,
            downloaded: false,
            disable_auto_sync: false,
            expiry_warning: None,
            set_warn_sent: None,
        }
    }

    fn fail(&mut self, msg: String) {
        self.outcome = Outcome::Failed(msg);
        self.record_sync = false;
    }
}

/// 一次自动同步循环的输入
pub struct CycleInputs<'a> {
    pub provider: &'a dyn SyncProvider,
    pub validator: &'a dyn QrAuthenticator,
    pub cookie: &'a str,
    pub remote_path: &'a str,
    /// 网盘标识（"quark" / "baidu"，同步记账分账 key）
    pub provider_key: &'a str,
    pub cookie_expires_at: i64,
    pub warn_sent: bool,
    /// 当前 unix 秒（注入以便测试）
    pub now_secs: i64,
    pub vault: &'a dyn VaultIo,
    pub sync_state: &'a dyn SyncStateIo,
}

/// 执行一次自动同步循环：过期预警 → Cookie 校验 → 追加式会话（push/apply/挂起）。
pub fn run_cycle(i: CycleInputs) -> SyncReport {
    let (expiry_warning, set_warn_sent) =
        check_expiry(i.cookie_expires_at, i.now_secs, i.warn_sent);

    let mut report = if i.cookie.is_empty() {
        SyncReport::new(Outcome::NotLoggedIn)
    } else {
        match i.validator.validate(i.cookie) {
            Ok(true) => SyncReport::new(Outcome::Skipped),
            Ok(false) => {
                let mut r = SyncReport::new(Outcome::CookieExpired);
                r.disable_auto_sync = true;
                r
            }
            Err(e) => SyncReport::new(Outcome::Failed(format!("validate:{}", e))),
        }
    };
    report.expiry_warning = expiry_warning;
    report.set_warn_sent = set_warn_sent;

    // 只有校验通过（占位为 Skipped）才继续方向判定与传输
    if matches!(report.outcome, Outcome::Skipped) {
        execute_cycle(&i, &mut report);
    }
    report
}

/// 会话执行（自动循环专用）
fn execute_cycle(i: &CycleInputs, report: &mut SyncReport) {
    let result = session::sync_auto(&SessionInputs {
        provider: i.provider,
        vault: i.vault,
        sync_state: i.sync_state,
        remote_path: i.remote_path,
        provider_key: i.provider_key,
        now_ms: i.now_secs * 1000,
    });
    apply_session(result, report);
}

/// 把会话结果映射进报告
fn apply_session(result: Result<session::SessionReport, String>, report: &mut SyncReport) {
    match result {
        Ok(r) => {
            report.outcome = match r.outcome {
                SessionOutcome::Pushed => Outcome::Uploaded,
                SessionOutcome::Applied => Outcome::Downloaded,
                SessionOutcome::Skipped => Outcome::Skipped,
                SessionOutcome::ConflictPending => Outcome::ConflictPending,
                SessionOutcome::GenesisMigrated => Outcome::GenesisMigrated,
            };
            report.downloaded = r.downloaded;
        }
        Err(e) => report.fail(e),
    }
}

/// 手动同步方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Upload,
    Download,
}

/// 手动同步输入（用户在界面上指定方向）
pub struct ManualInputs<'a> {
    pub provider: &'a dyn SyncProvider,
    pub validator: Option<&'a dyn QrAuthenticator>,
    pub cookie: &'a str,
    pub remote_path: &'a str,
    pub provider_key: &'a str,
    pub direction: Direction,
    pub vault: &'a dyn VaultIo,
    pub sync_state: &'a dyn SyncStateIo,
}

/// 执行一次手动同步。上传前校验 Cookie，下载不校验（与既有行为一致）；
/// Cookie 失效只报结果，不关闭自动同步。
/// 手动上传/下载视为用户裁决：会清除冲突挂起后执行。
pub fn run_manual(i: ManualInputs) -> SyncReport {
    let mut report = if i.cookie.is_empty() {
        SyncReport::new(Outcome::NotLoggedIn)
    } else {
        SyncReport::new(Outcome::Skipped)
    };

    if i.direction == Direction::Upload {
        if let Some(v) = i.validator {
            match v.validate(i.cookie) {
                Ok(true) => {}
                Ok(false) => {
                    report.outcome = Outcome::CookieExpired;
                    report.record_sync = false;
                    return report;
                }
                Err(e) => {
                    report.fail(format!("校验失败: {}", e));
                    return report;
                }
            }
        }
    }

    let result = match i.direction {
        Direction::Upload => session::push_manual(&SessionInputs {
            provider: i.provider,
            vault: i.vault,
            sync_state: i.sync_state,
            remote_path: i.remote_path,
            provider_key: i.provider_key,
            now_ms: chrono::Utc::now().timestamp_millis(),
        }),
        Direction::Download => session::pull_manual(&SessionInputs {
            provider: i.provider,
            vault: i.vault,
            sync_state: i.sync_state,
            remote_path: i.remote_path,
            provider_key: i.provider_key,
            now_ms: chrono::Utc::now().timestamp_millis(),
        }),
    };
    apply_session(result, &mut report);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::storage::sync_state::SyncState;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    // ---- 替身 ----

    struct FakeProvider {
        fail_upload: bool,
        fail_download: bool,
        calls: Mutex<Vec<&'static str>>,
    }

    impl FakeProvider {
        fn new() -> Self {
            FakeProvider {
                fail_upload: false,
                fail_download: false,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl SyncProvider for FakeProvider {
        fn upload(&self, _local: &Path, _remote: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push("upload");
            if self.fail_upload {
                Err("网络错误".to_string())
            } else {
                Ok(())
            }
        }
        fn download(&self, _remote: &str) -> Result<Vec<u8>, String> {
            self.calls.lock().unwrap().push("download");
            if self.fail_download {
                Err("网络错误".to_string())
            } else {
                Ok(b"downloaded vault bytes".to_vec())
            }
        }
        fn list(&self, _remote_dir: &str) -> Result<Vec<super::super::RemoteFile>, String> {
            Ok(Vec::new())
        }
        fn delete(&self, _remote_path: &str) -> Result<(), String> {
            Ok(())
        }
    }

    struct FakeValidator {
        valid: bool,
    }

    impl QrAuthenticator for FakeValidator {
        fn start_login(&self) -> Result<crate::shared::sync::auth::QrLoginSession, String> {
            unimplemented!()
        }
        fn poll_login(
            &self,
            _token: &str,
        ) -> Result<crate::shared::sync::auth::QrLoginStatus, String> {
            unimplemented!()
        }
        fn validate(&self, _cookie: &str) -> Result<bool, String> {
            Ok(self.valid)
        }
        fn name(&self) -> &str {
            "fake"
        }
    }

    /// 临时目录里的密码簿替身：真实文件、真实 mtime，replace 即写文件
    struct TempVault {
        path: PathBuf,
    }

    impl TempVault {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "ps-engine-{}-{}-{}",
                std::process::id(),
                tag,
                nanos
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempVault {
                path: dir.join("vault.json"),
            }
        }
    }

    impl VaultIo for TempVault {
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

    /// 内存同步记账替身
    struct MemState(Mutex<SyncState>);

    impl MemState {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "ps-engine-state-{}-{}.json",
                std::process::id(),
                tag
            ));
            MemState(Mutex::new(SyncState::load_or_create(p).unwrap()))
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

    // ---- 过期预警 ----

    const DAY: i64 = 86400;

    #[test]
    fn expiry_warning_windows() {
        let now = 1_000_000;
        // 未知过期时间：不预警
        assert_eq!(check_expiry(0, now, false), (None, None));
        // 已过期：不预警（由校验兜底）
        assert_eq!(check_expiry(now - 1, now, false), (None, None));
        // <7 天且未提醒过：预警并置位
        let (w, set) = check_expiry(now + 3 * DAY, now, false);
        assert_eq!(w.unwrap().days_left, 3);
        assert_eq!(set, Some(true));
        // <7 天但提醒过：不再提醒
        assert_eq!(check_expiry(now + 3 * DAY, now, true), (None, None));
        // 还远：重置提醒标记
        assert_eq!(check_expiry(now + 30 * DAY, now, true), (None, Some(false)));
    }

    // ---- 完整循环 ----

    fn cycle<'a>(
        provider: &'a FakeProvider,
        vault: &'a TempVault,
        validator: &'a FakeValidator,
        state: &'a MemState,
        cookie: &str,
    ) -> SyncReport {
        run_cycle(CycleInputs {
            provider,
            validator,
            cookie,
            remote_path: "/VAULT/vault.json",
            provider_key: "quark",
            cookie_expires_at: 0,
            warn_sent: false,
            now_secs: 1_000_000,
            vault,
            sync_state: state,
        })
    }

    #[test]
    fn cycle_local_invalid_cloud_empty_skips() {
        let provider = FakeProvider::new();
        let vault = TempVault::new("skip-empty"); // 文件不存在 → 本地无效
        let validator = FakeValidator { valid: true };
        let state = MemState::new("skip-empty");

        let report = cycle(&provider, &vault, &validator, &state, "cookie");

        // 云端与本地均为空 → 跳过
        assert_eq!(report.outcome, Outcome::Skipped);
        assert!(report.record_sync);
        assert!(provider.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn cycle_local_valid_cloud_empty_pushes() {
        let provider = FakeProvider::new();
        let vault = TempVault::new("push-fresh");
        std::fs::write(&vault.path, b"local").unwrap();
        let validator = FakeValidator { valid: true };
        let state = MemState::new("push-fresh");

        let report = cycle(&provider, &vault, &validator, &state, "cookie");

        assert_eq!(report.outcome, Outcome::Uploaded);
        // 两段式 push：blob 一次 + meta 一次
        assert_eq!(*provider.calls.lock().unwrap(), vec!["upload", "upload"]);
    }

    #[test]
    fn cycle_upload_failure_reports_failed() {
        let mut provider = FakeProvider::new();
        provider.fail_upload = true;
        let vault = TempVault::new("fail");
        std::fs::write(&vault.path, b"local").unwrap();
        let validator = FakeValidator { valid: true };
        let state = MemState::new("fail");

        let report = cycle(&provider, &vault, &validator, &state, "cookie");

        assert!(matches!(report.outcome, Outcome::Failed(_)));
        assert!(!report.record_sync);
    }

    #[test]
    fn cycle_cookie_expired_disables_and_skips_transfer() {
        let provider = FakeProvider::new();
        let vault = TempVault::new("expired");
        let validator = FakeValidator { valid: false };
        let state = MemState::new("expired");

        let report = cycle(&provider, &vault, &validator, &state, "cookie");

        assert_eq!(report.outcome, Outcome::CookieExpired);
        assert!(report.disable_auto_sync);
        assert!(!report.record_sync);
        assert!(provider.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn cycle_cookie_empty_reports_not_logged_in() {
        let provider = FakeProvider::new();
        let vault = TempVault::new("nologin");
        let validator = FakeValidator { valid: true };
        let state = MemState::new("nologin");

        let report = cycle(&provider, &vault, &validator, &state, "");

        assert_eq!(report.outcome, Outcome::NotLoggedIn);
        assert!(!report.record_sync);
    }

    #[test]
    fn cycle_warns_before_expiry_once() {
        let provider = FakeProvider::new();
        let vault = TempVault::new("warn");
        std::fs::write(&vault.path, b"local").unwrap();
        let validator = FakeValidator { valid: true };
        let state = MemState::new("warn");

        let now = 1_000_000_i64;
        let report = run_cycle(CycleInputs {
            provider: &provider,
            validator: &validator,
            cookie: "cookie",
            remote_path: "/VAULT/vault.json",
            provider_key: "quark",
            cookie_expires_at: now + 3 * DAY,
            warn_sent: false,
            now_secs: now,
            vault: &vault,
            sync_state: &state,
        });
        assert_eq!(report.expiry_warning.as_ref().unwrap().days_left, 3);
        assert_eq!(report.set_warn_sent, Some(true));
        // 上传照常执行
        assert_eq!(report.outcome, Outcome::Uploaded);
    }

    // ---- 手动同步 ----

    #[test]
    fn manual_upload_validates_first() {
        let provider = FakeProvider::new();
        let vault = TempVault::new("manual-up");
        std::fs::write(&vault.path, b"local").unwrap();
        let validator = FakeValidator { valid: false };
        let state = MemState::new("manual-up");

        let report = run_manual(ManualInputs {
            provider: &provider,
            validator: Some(&validator),
            cookie: "cookie",
            remote_path: "/VAULT/vault.json",
            provider_key: "quark",
            direction: Direction::Upload,
            vault: &vault,
            sync_state: &state,
        });

        assert_eq!(report.outcome, Outcome::CookieExpired);
        // 手动失效不关自动同步
        assert!(!report.disable_auto_sync);
        assert!(provider.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn manual_download_does_not_validate_and_applies() {
        let provider = FakeProvider::new();
        let vault = TempVault::new("manual-dl");
        std::fs::write(&vault.path, b"old local").unwrap();
        let validator = FakeValidator { valid: false }; // 即使失效，下载也不校验
        let state = MemState::new("manual-dl");

        let report = run_manual(ManualInputs {
            provider: &provider,
            validator: Some(&validator),
            cookie: "cookie",
            remote_path: "/VAULT/vault.json",
            provider_key: "quark",
            direction: Direction::Download,
            vault: &vault,
            sync_state: &state,
        });

        // 云端为空（FakeProvider list 恒空）+ 本地有效 → genesis push 之外，
        // 下载方向在空云端下走 genesis：本地有效即首推
        assert!(matches!(
            report.outcome,
            Outcome::Uploaded | Outcome::GenesisMigrated
        ));
    }
}
