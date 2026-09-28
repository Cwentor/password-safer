# 云同步策略深化重构 — 进度与完成计划

> 日期：2026-09-27 · 分支 master · 状态：**代码完成（第 1–5 步 ✅），待手工冒烟与提交（第 6 步）**
> 架构体检报告：`C:\Users\cwt15\AppData\Local\Temp\architecture-review-20260927-004259.html`
>
> ✅ `cargo test`：20 个测试全部通过；`cargo build` 零警告。
> 实施中的两处小偏差：`AppState.database` 由 `Mutex<JsonStore>` 改为 `Arc<Mutex<JsonStore>>`
>（VaultAccess 需要克隆引用走单一写入口，所有 `.lock()` 调用点不受影响）；
> `SyncProvider::name()` 一并删除（消息改走 `provider_display`，删除测试判定为死接口）。

## 一、目标与已拍板的决策

把云同步的方向判定、mtime 比较、last_sync / cookie 过期记账从 `lib.rs` 的调度循环
（原 440–668 行）收进同步接缝后面；vault.json 写入收口为单一入口。

| 决策点 | 结论 |
|---|---|
| 双向同步语义 | **保持「新者覆盖」**（本地 mtime 对比云端 mtime），只搬位置不改行为 |
| Cookie 过期预警（<7 天） | **泛化到两个网盘**（原为夸克专属；`quark_cookie_warn_sent` 不对称特例随之消除） |
| vault.json 单一写入口范围 | **含 import/export**（同步下载与导入共用 `JsonStore::replace_raw`） |
| 配置 JSON 形状 | 不动，保持前端 `get_config`/`save_config` 与旧 vault.json 兼容 |
| `quark_last_remote_mtime` | 删除（写了 5 处、零读取的死状态；旧数据文件多此字段不影响反序列化） |

术语已落档 `CONTEXT.md`（密码簿 / 云同步 / 同步方向判定 / 扫码登录等）。

## 二、已完成（第 1–3 步）

1. **`CONTEXT.md`**（新建）：领域术语表。
2. **`src-tauri/src/shared/models.rs`**：删除 `quark_last_remote_mtime` 字段及默认值。
3. **`src-tauri/src/shared/storage/json_store.rs`**：
   - 新增 `write_atomic()`：临时文件 + rename 原子替换，替换前备份 `.bak`；`save()` 改走它。
   - 新增 `replace_raw(bytes)`：**先验证可解密、可解析，再原子替换**——坏文件不会再毁掉本地密码簿。
   - 新增 `#[cfg(test)]` 测试 7 个：crypto 往返 / 错钥失败 / CRUD / replace_raw 拒绝坏数据且不动原文件 / replace_raw 接受有效导出 / 配置往返。
4. **`src-tauri/src/shared/sync/mod.rs`**（重写）：
   - `SyncProvider::download` 改为 `fn download(&self, remote_path) -> Result<Vec<u8>, String>`（返回字节，不落盘）；`upload` 参数 `&PathBuf` → `&Path`。
   - 新增 `build_provider(name, cookie)` 工厂——按网盘分派的**唯一**位置。
   - `local_vault_valid` 从 quark.rs 移入此处；删除 `sync_upload`/`sync_download` 包装函数。
5. **`src-tauri/src/shared/sync/engine.rs`**（新建，核心）：纯决策层，不依赖 Tauri。
   - `decide_direction()` 纯函数：本地无效→云端有则下载/无则跳过；本地有效→比较 mtime 三分支。
   - `check_expiry()` 纯函数：<7 天预警一次（`warn_sent` 标记由报告带回，两个网盘通用）。
   - `VaultIo` 接缝：`path / local_valid / local_mtime / replace`，生产实现走 `Mutex<JsonStore>`，测试用临时目录。
   - `run_cycle()`（自动调度）：校验 → 方向判定 → 传输 → 产出 `SyncReport`。
   - `run_manual()`（手动同步）：用户指定方向；上传前校验、下载不校验（与既有行为一致）；手动 Cookie 失效**不**关闭自动同步。
   - `SyncReport { outcome, record_sync, downloaded, disable_auto_sync, expiry_warning, set_warn_sent }`：所有记账决策都在报告里，副作用归调用方。
   - `#[cfg(test)]` 测试 10 个：方向判定 3、过期窗口 1、完整循环 5（下载/跳过/上传/失效/未登录/预警/失败）、手动 2；用 FakeProvider/FakeValidator/TempVault 替身，零网络。
6. **`src-tauri/src/shared/sync/baidu.rs`**：`download` 返回字节（原直写 vault.json 的绕锁写入已消除）；`upload` 改 `&Path`。
7. **`src-tauri/src/shared/sync/quark.rs`**：同上；删除 `local_vault_valid`（移至 mod.rs）；下载的 `.bak` 回滚职责移入 `JsonStore::write_atomic`；403 重试逻辑原样保留。

## 三、第 4–5 步（已完成）

**第 4 步：`lib.rs` 薄壳重写 ✅**

1. ✅ **AppState**：新增 `baidu_cookie_warn_sent`（过期预警泛化到两个网盘）；`database` 改为 `Arc<Mutex<JsonStore>>`。
2. ✅ **`VaultAccess`**（实现 `engine::VaultIo`）：path / local_mtime / replace（锁内 `replace_raw`），锁只护替换、不跨网络调用。
3. ✅ **`sync_now`**：`build_provider` + `engine::run_manual`；报告映射回 `SyncResult`（消息文案与旧版一致）；成功按 `record_sync` 写 last_sync，`downloaded` 时 emit `sync://restored`。内联 validate 块与字符串分派已删除。
4. ✅ **`handle_sync_result` 删除**，新写 **`apply_sync_report`**——所有同步副作用的唯一出口：warn 标记、`sync://cookie_expiring`、一次 load→改→save 的 last_sync 记账、`sync://expired`（关自动同步）、`sync://error`、移动端通知，全部按报告分发。
5. ✅ **`start_sync_scheduler`**：循环体缩为——load config → 对 `["baidu","quark"]` 逐个 `build_provider` → `run_cycle` → `apply_sync_report`。原 78 行双向状态机、5 处配置写回、7 处字符串分派全部移除。
6. ✅ **`import_db`**：`fs::read` → `replace_raw`（先验证再原子替换，不再盲目 `fs::copy` 覆盖）。
7. ✅ **`export_db`**：`fs::copy` 移入锁内。
8. ✅ **未动**：`qr_start`/`qr_poll`/`logout`/`check_sync_connection` 与 desktop/mobile auth（留给候选 02）；命令注册表不变；前端零改动。

**第 5 步：验证 ✅**

- `cargo test`：20 个测试全部通过（engine 11 + json_store 6 + crypto 2 等），0.07s。
- `cargo build`：零警告零错误。

## 六、收尾（第 6 步，待办）

- `npm run tauri dev` 手工冒烟：设置→云同步→立即上传/下载、自动同步开关、退出登录、扫码登录（auth 未动，应不受影响）。
- 提交（建议单 commit：`refactor(sync): 云同步策略收进接缝，vault.json 单一写入入口`）。

## 四、验收对照（体检报告卡片 01 的承诺）

- [x] 方向判定 / mtime 比较 / 过期预警 / 记账决策集中到一处（engine.rs）
- [x] vault.json 绕锁双写消除（download 返回字节 + replace_raw 单一入口）
- [x] 双向同步状态机可测（20 个离线单测，替身注入，无需网络/WebView）
- [x] 按网盘分派收敛到 `build_provider` 一处
- [x] lib.rs 调度循环瘦身（864 行 → 调度器仅 ~80 行薄壳）
- [x] `quark_last_remote_mtime` 死字段删除
- [x] import 路径不再盲目覆盖（先验证再替换）
- [ ] 手工冒烟 + 提交（第 6 步，待办）
