# 追加式云同步 — 远端不可覆盖、仅允许新增的设计与实施计划

> 日期：2026-11-01 · 分支 `feat/append-only-sync` · 状态：**代码完成（阶段 0–5 全部落地，cargo 63 测试通过、零警告、vite build 通过），待手工冒烟（阶段 6 清单）**
> 背景约束：远端网盘（夸克 / 百度）限制为**不能覆盖重写，只能上传全新文件**；
> 目标：任意新接入客户端一键拉到全量最新状态，多端并发可收敛、历史可追溯。
> 前置阅读：现有同步为 vault.json 整文件 mtime 新者覆盖（`engine.rs::decide_direction`），
> 夸克实现是"删旧传新"（`quark.rs:609-623`），百度是 precreate→superfile2→create(rtype=3)。

## 一、现状复核（以代码为准）与新方案的差距

| # | 现状 | 追加式远端下的问题 | 本计划的处置 |
|---|---|---|---|
| 1 | `upload` 语义 = 覆盖远端：夸克先 `delete_files` 再传新（`quark.rs:614-623`）；百度 `rtype=3` 覆盖语义（`baidu.rs:161`） | 直接违反"不可覆盖"约束 | trait 语义改为**仅新增**；文件名内容寻址/时间戳+设备号，天然不撞名 |
| 2 | 方向判定靠 mtime 比较（`engine.rs:25-51`）；百度 `remote_file_mtime` 未实现（trait 默认 Err）→ 百度自动同步每轮必报错 | mtime 不再是可靠的"新"信号 | 判定改为 **head 对比**（manifest 快照链），删掉 `remote_file_mtime`，消灭该 bug |
| 3 | `last_sync` 记账写在 AppConfig 里 = 在 vault.json 内部（`models.rs:42,49`），每次同步后写回（`lib.rs:175-183`）→ 本地 mtime 永远变新 → **每周期重复上传** | 灾难级放大：每周期新增一个 blob + 一个 meta，远端无限膨胀 | 记账搬出 vault.json → 本地 `sync_state.json`（阶段 0，**硬前提**） |
| 4 | 新客户端拉取 = 下载 vault.json，但 master.key 不上云 → 拉到也解不开 | "拉到最新"只完成一半 | **拍板取消 master.key**：库密码派生密钥，凭密码跨设备解密（阶段 4，§2.8） |
| 5 | `JsonStore::open` 解密失败直接拒绝启动，不回退 `.bak` | 与本方案无直接耦合，但新方案依赖本地 vault 可用 | 阶段 0 顺手修 |
| 6 | 测试接缝完善（`SyncProvider`/`VaultIo`/`QrAuthenticator` 三替身，engine 内联测试） | — | 沿用此风格，新增 FakeAppendOnlyProvider |

## 二、设计（对原思考稿的采纳与修正）

### 2.1 采纳的部分

- **blob 内容寻址**：vault 内容本来就是 E2E 密文，`blob 名 = SHA-256(密文)`，同内容只传一次。
  去重无需扫 /blobs 目录——夸克 `file/update/hash`（finish=true）与百度 precreate slice-md5
  都支持哈希秒传，push 时零成本判定远端是否已有该 blob。
- **元数据与实体分离**：远端只存不可变 blob；"最新状态"由 meta 快照维护；客户端先拉 meta 再按需拉 blob。
- **两段式 push**：先传 blob（崩溃留孤儿 blob，无害，下次秒传复用），meta 后传 = 提交点。
- **删除即标记**：meta 里 `deleted: true`，实体 blob 永不因"逻辑删除"而消失。

### 2.2 修正的部分（原思考稿的坑）

1. **"列出 /meta 取版本号最大"有并发漏洞**：没有服务器分配单调版本号，两个客户端并发 push
   会产生两个同号/乱序 meta。修正：meta 文件名 `meta_<UTC毫秒>_<device_id>.json`（唯一、可排序），
   meta 内带 `supersedes` 字段记录它已知的一切 head。**head 集合 = 未被任何 meta 的 supersedes
   引用的 meta**。fork（多 head）时按 `(updated_at, device_id)` 字典序确定性裁决——所有客户端
   对同一组 head 收敛出同一个胜者。
2. **冲突不静默裁决（已拍板 v2）**：冲突 = **挂起待用户裁决**，不在引擎里自动 LWW 定胜负。
   挂起时本地内容先上传为 blob 并写 `kind: "backup"` 的 meta（不参与 head 解析、不抢位，
   让本地侧在任何设备上都可寻址），远端侧本来就由 head 引用——两侧都不会丢。
   裁决 UI 与记录级合并见 §2.9，后台自动同步遇冲突一律挂起（无法阻塞等输入）。
3. **增量日志 log_{start}_{end} 不做**：单文件密码簿的 meta 全量快照只有几百字节，
   操作日志是过度设计；blob 本身已天然构成历史。
4. **定期清理孤立 blob 不做**：历史版本在密码管理器场景是特性（误覆盖找回），量级可忽略
   （几 KB/次 × 手动频率）。唯一例外是 legacy 迁移（§2.7，拍板删除）。
5. **选择性同步 / 目录过滤 / 按需下载不做**：单文件 vault（几 KB～几 MB）用不上。
6. **时钟偏差说明**：head 显示与"较新者胜"的默认合并按 `updated_at` 排序，时钟严重偏慢的
   设备在合并建议里永远显旧——与原稿"全局时间戳后操作覆盖前操作"性质相同，文档化为可接受限制。

### 2.3 远端布局（沿用现有 base 目录）

```
/apps/VAULT/ （百度） | /VAULT/ （夸克）
├── meta/meta_<UTCms>_<device>.json    # 提交点，唯一"最新状态"来源
├── blobs/<sha256>.bin                 # 不可变密文 blob，内容寻址
└── vault.json                         # legacy：迁移成功后删除（已拍板）
```

meta 为明文 JSON，**不含任何明文数据**（blob 名是密文哈希）：

```json
{
  "kind": "head",
  "updated_at": 1730000000000,
  "device_id": "a1b2c3d4",
  "supersedes": ["meta_1729999999000_e5f6.json"],
  "files": {
    "vault.json": { "blob": "<sha256hex>", "size": 12345, "updated_at": 1730000000000, "deleted": false }
  }
}
```

`files` 用语义路径作 key，未来可扩展附件、导出包（master.key 已拍板不上云，见 §2.8）。
`kind` 取值 `"head"`（默认）| `"backup"`（冲突挂起备份，不参与 head 解析）。

### 2.4 本地新增 sidecar：`sync_state.json`（vault.json 同目录，不上云）

```json
{
  "device_id": "a1b2c3d4",
  "last_heads": { "quark": ["meta_....json"], "baidu": [] },
  "local_dirty_at": 1730000000000,
  "last_sync": { "quark": "2026-11-01 12:00", "baidu": "" },
  "conflict_pending": null,
  "legacy_pending_delete": false
}
```

- `device_id`：首启随机生成，标识本设备（meta 命名与显示排序用）。
- `local_dirty_at`：`JsonStore::save`（用户写路径）时刷新；`replace_raw`（同步写路径）不刷新——
  区分"用户改过"与"同步落下"；冲突 UI 用它标注哪侧较新。
- `last_sync` 从 AppConfig 迁来，前端展示改走新 command（阶段 0）。
- `conflict_pending`：冲突挂起状态（§2.9），非 null 时自动同步周期直接 Skip。
- `legacy_pending_delete`：legacy 删除失败的补删标记（§2.7）。

### 2.5 接口演进

```rust
pub trait SyncProvider: Send + Sync {
    /// 上传新文件。语义收严：仅新增，绝不删除/覆盖远端已有文件。
    /// 调用方保证文件名全局唯一（内容哈希 / 时间戳+设备号）。
    fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), String>;
    /// 下载远端文件字节。
    fn download(&self, remote_path: &str) -> Result<Vec<u8>, String>;
    /// 列出远端目录（分页内部消化），返回 name/size/mtime。
    fn list(&self, remote_dir: &str) -> Result<Vec<RemoteFile>, String>;
    /// 删除远端单个文件（仅用于 legacy 迁移补删；同步主流程不得调用）。
    fn delete(&self, remote_path: &str) -> Result<(), String>;
}
```

- `remote_file_mtime` 删除（连同 `decide_direction` 的 mtime 语义）。
- 夸克：upload 去掉 delete 分支；list 复用 `file/list` 分页（`find_file_fid` 已在用同一接口）；
  delete 复用 `file/delete`（`delete_files` 现成）。
- 百度：实现 list（`/api/file/list`，web=1 分页）与 delete（`/api/filemanager`）；`rtype=3`
  在唯一文件名下永不触发，等效 append-only。

### 2.6 核心流程

**push（本地有变更）**
1. list `/meta` → 解析 head 集合；
2. 若存在本地 `last_heads` 之外的 head 且本地 vault 哈希 ≠ 其 blob → 走冲突挂起（§2.9）；
3. 计算 vault 密文 sha256；等于当前 head 引用的 blob → 无需 push，更新 state 收尾；
4. 传 blob（秒传命中即跳过）→ 传 meta（`supersedes` = 已知全部 heads）→ 更新 `last_heads`。

**pull（含首装）**
1. list `/meta` → head 集合；空且 legacy vault.json 存在 → 迁移（§2.7）；空且无 legacy → 空库跳过；
2. head 集合 ⊆ `last_heads`，或仅多出自己 push 过的 head → 无新，跳过；
3. 多 head（fork）→ 提示冲突待处理（§2.9），不自动择一；
4. 单一新 head → 下载 blob → sha256 校验 → `replace_raw`（已有验密 + 原子替换 + .bak）→ 更新 `last_heads`。

**幂等与崩溃安全**
- blob 传完、meta 未传 → 孤儿 blob，无害（下次同内容秒传命中）；
- meta 传完、state 未更新 → 下轮 list 看到自己的 head，`last_heads` 收敛，跳过；
- 下载完未 apply → 下轮重来；apply 前有 sha 校验 + `replace_raw` 验密双保险。

### 2.7 legacy 迁移（拍板：迁移成功后删除旧文件）

远端 `/meta` 为空且旧 `vault.json` 存在：
读旧文件字节 → 传为 blob（同一份密文，秒传大概率直接命中）→ 写 genesis meta（supersedes 空）→
**list `/meta` 确认 genesis head 在列后，`delete` 旧 vault.json**。

- 删除前必须先确认 genesis meta 已可被 list 到（提交点先行，破坏性动作在后）；
- 删除失败不阻塞同步：置 `legacy_pending_delete: true`，后续周期重试补删；
- **纪律约束：全设备一起升级**。legacy 删除后，未升级的旧客户端会因"远端无 vault.json"重新上传，
  而新客户端只看 meta，两端静默分叉。个人项目可控，写进冒烟清单。

### 2.8 库密码派生密钥（拍板定稿：取消 master.key 与包裹件，短密码即钥匙）

**核心模型**：不再存在 master.key 文件与包裹件。**库密码**（用户自设短密码）经 Argon2id
派生 32 字节长钥，直接作为整库与字段加密的密钥。用户只需记住库密码，
任何设备凭密码即可解密云端密码簿——零保存物、零导入环节。

**加密信封（vault v2 格式，本地 vault.json 与远端 blob 同字节）**
- vault 改为自描述信封：`{ "v":2, "kdf":{algo:"argon2id", salt_b64, m, t, p}, "nonce_b64", "ct_b64" }`，
  ct 内层仍是现有结构（字段级密文 + StoreData JSON）。
- **盐与 KDF 参数随库走**（存在信封头里）：新设备拿到 blob 即拥有派生所需一切，只需用户输密码；
  参数取 m=256MiB / t=4 / p=4（解锁约 0.5～1s，显著抬高离线爆破成本），建库时随机 16 字节盐。
- 本地文件与 blob 同字节 → 内容寻址去重、秒传逻辑不变。

**安全性账本（拍板时已知悉，落地缓解）**
- 云端 blob = KDF(库密码) 的产物：**云端泄露时库密码可被离线爆破**——用强 KDF 参数、
  强制 ≥8 位 + 强度建议（≥10 位或四词组）缓解；这是相对包裹件方案让渡的安全性。
- **忘记库密码 = 数据永久丢失**（本地、云端均不可解，无任何兜底）——
  所有设置/修改密码入口强制警示文案。
- 改密码 = 全库重加密（字段 + 整文件）→ 新 blob → push 新 head；**历史 blob 仍由旧密码可解**，
  改密码流程提供可选「清理云端历史 blob」步骤（provider delete，防旧密码泄露导致历史回读）。
- 并发改密码形成"跨密码 fork"：两侧信封密码不同，冲突解决 UI 无法同时解密两侧——
  v1 简化为选择一侧（输入其密码）应用，另一侧 blob 留存可找回。

**本机解锁与启动体验**
- 派生钥经系统凭据库缓存（`keyring` crate：Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service），
  **本机启动免输密码**，与现状体验一致；缓存随改密码失效。
  跨设备永不用缓存——新设备无缓存，自然落到输密码路径。
- （备选不做：每次启动询问密码。）

**首次运行**：无库无密码 → 直接进入「设置库密码」（强制 ≥8 位 + 强度提示 +
"忘记密码无法找回"警示 + 勾选确认）→ 建库。原首启生成 master.key 的路径删除。

**新设备接入（极致简化）**
1. 扫码绑盘 → 同步拉 blob → 解析信封头（拿到盐与参数）→ 弹「输入库密码」；
2. 派生 → 解密成功 → 入库使用；错密码原地重试。无任何导入/粘贴/文件环节。
- 「以本机为准强制上云」入口保留（云端有数据但用户想以本机重开）：force push genesis，
  云端旧 head 仍可从 blobs 找回。
- **边界情形**：新设备先建库后绑盘、本地已有输入——先解密暂存本地库，转入冲突解决 UI（§2.9）。

**存量用户迁移（v1 master.key 模式 → v2 库密码模式）**
- 启动时检测本地 vault 为 v1 格式：提示「升级为库密码模式」（设库密码 → 旧钥解密全库 →
  新钥重加密 → 原子写 → push 新 head → master.key 归档删除），或暂不升级（v1 继续可用）；
- `replace_raw` 双格式兼容期：按信封 `v` 字段分派验证逻辑。

**术语与产品整理**：master.key 从产品与 CONTEXT.md 术语表退役；原「主密码」（仅前端的可选锁屏口令）
与库密码合并为唯一口令——解锁密码簿即解锁应用，旧主密码设置在迁移时废弃。

**残余风险（写明，属拍板取舍）**：云端泄露 + 弱密码 = 可离线爆破；忘记密码 = 永久丢失。
换来的是"只记一个密码、零保存物、新设备零导入"。安全性边界由用户密码强度决定，
与"零账号、数据主权"定位一致。

### 2.9 冲突解决 UI 与记录级合并（拍板：v2 用户裁决）

**挂起（引擎侧，同步周期内完成，不阻塞用户）**
- push/pull 发现 fork 且本地有未推送修改 → 本地内容上传为 blob + 写 `kind:"backup"` meta
  （不抢 head，任意设备可寻址）→ `conflict_pending = { remote_heads, backup_blob, detected_at }`
  → `Outcome::ConflictPending`，通知用户"同步冲突待处理"；
- 挂起期间 vault 照常可编辑；后续同步周期遇 pending 直接 Skip，不再重复挂起；
- 若期间远端又出现更新 head → 重新挂起并对齐到最新 head 集合（备份 meta 链自然累积，均可找回）。

**裁决（用户操作时）**
- 新 command `get_conflict_details`：解密两侧（本地当前 vault + 远端 head blob，同一 master.key 均可解），
  按 `PasswordRecord.id` 对齐，返回 `only_local / only_remote / both_differ` 三组记录
  （复用现有 DTO 约定），并标注每条较新侧（`last_used` + `local_dirty_at` 辅助）；
- 新 command `resolve_conflict(choices)`：纯函数 `merge_vaults(local, remote, choices) -> StoreData`
  → `JsonStore::save` → push 新 head（supersedes = 挂起的 remote_heads + 本地已知 heads）→ 清 pending；
- 前端新增 `ResolveConflictModal`：三个快捷按钮（全部用本地 / 全部用云端 / 智能合并）+
  逐条可改选；智能合并默认逐条取较新者。
- `merge_vaults` 合并语义：passwords 按 id 取并集；config（Cookie、设置等）不逐字段合并，
  整体取较新侧；**"仅一侧存在"无法区分新建与删除**（StoreData 无墓碑），默认保留（并集）——
  误复活好过误删除，墓碑列为 v3 演进。

## 三、实施阶段（每阶段独立可验证、独立提交）

### 阶段 0：记账搬出 vault.json（硬前提）+ `.bak` 回退
- 新增 `shared/storage/sync_state.rs`：`sync_state.json` 读写（device_id、last_heads、
  local_dirty_at、last_sync、conflict_pending、legacy_pending_delete）。
- `lib.rs` 删除所有 `cfg.baidu_last_sync/quark_last_sync` 写回（`lib.rs:175-183`、`lib.rs:427-433`）。
- `JsonStore::save` 刷新 `local_dirty_at`（注入回调，store 不依赖 state 文件布局）。
- `JsonStore::open` 解密失败回退 `.bak`（修审计问题③）。
- 新 command `get_sync_state`；`SettingsModal.js` 的 last_sync 展示改走它（`SettingsModal.js:99-100`）。
- 验证：cargo test 全绿；两轮自动同步不产生第二次上传。

### 阶段 1：Provider 扩展——list / delete + 仅新增语义
- `SyncProvider` 增加 `RemoteFile` + `list()` + `delete()`；删 `remote_file_mtime`。
- 夸克 / 百度分别实现 list 与 delete；夸克 upload 去 delete 分支；百度 upload 不动。
- `FakeProvider` 适配；新增"upload 不删远端同名"断言。
- 验证：cargo test；真机各手动列目录、删测试文件一次。

### 阶段 2：manifest 模块（纯函数核心，测试密度最高）
- 新增 `shared/sync/manifest.rs`：meta 结构体、命名生成、解析校验、
  **head 解析**（supersedes 集合减法 + 忽略 `kind:"backup"`）、fork 检出。
- 单测：正常单链、并发 fork、backup 不参与 head、坏 meta 文件跳过、孤立 supersedes 引用容错。
- 验证：cargo test（本模块 ≥10 个用例）。

### 阶段 3：session 编排 + 引擎改造
- 新增 `shared/sync/session.rs`：push / pull / genesis / **legacy 迁移（含确认后删除 + 补删重试）** /
  冲突挂起 / 幂等，依赖 `SyncProvider` + `VaultIo` + `SyncStateIo`（新接缝，替身实现）。
- `engine.rs`：`decide_direction(mtime)` → `decide_action(local_state, remote_heads)`；
  Cookie 校验、过期预警、SyncReport 骨架保留；新增 `Outcome::ConflictPending` / `Migrated`。
- FakeAppendOnlyProvider（内存 HashMap，upload 撞名即 Err、delete 可用）覆盖 §2.6/2.7/2.9 挂起路径与崩溃序列。
- 验证：cargo test（新增 ≥15 用例）。

### 阶段 4：库密码派生密钥（KDF 改造，取代 master.key 体系）
- `crypto.rs`：Argon2id `derive_key(password, salt, params)`；vault 信封 v2（自描述 JSON，盐与参数随库走）；
  字段加密沿用同一派生钥，`Crypto` 对外接口不变。
- `JsonStore`：`open_with_password` / `re_encrypt(new_password)`；`replace_raw` 按信封 `v` 分派 v1/v2 双格式兼容；
  `AppState` 的 `Arc<Mutex<JsonStore>>` 支持重开。
- 本机解锁：`keyring` crate 缓存派生钥（改密码即失效；跨设备不用缓存）。
- 首启流程改造：设库密码 → 建库，删除 master.key 生成路径。
- 新设备：拉 blob → 解析信封 → 输密码 → derive → 解密入库。
- 存量迁移：v1 检测 + 升级提示（一次性重加密）+ master.key 归档删除；改密码流程含可选云端历史清理。
- 命令与 UI：`set_vault_password` / `unlock_vault` / `change_vault_password`；
  `VaultUnlockModal`（首启设密 / 新设备输密 / 改密码三态）、强度提示、忘记密码警示。
- 术语：CONTEXT.md 的 master.key 退役、「库密码」入库；前端旧「主密码」并入库密码。
- 验证：cargo test（KDF 确定性、信封往返、错密码拒绝、改密码往返、v1→v2 迁移往返、缓存失效）；
  手工冒烟"新设备扫码 → 输库密码 → 全量数据可用"、"忘记密码提示不可找回"。

### 阶段 5：冲突解决 UI + 记录级合并
- `merge_vaults` 纯函数（含测试：并集、双 id 差异按选择、config 取新侧、删除复活语义）。
- 命令 `get_conflict_details` / `resolve_conflict`；引擎挂起状态落地与清除。
- 前端 `ResolveConflictModal`（快捷三选 + 逐条改选）+ 冲突横幅/通知。
- 验证：cargo test（merge ≥8 用例）；手工双端冲突全路径。

### 阶段 6：接线、前端与总冒烟
- `lib.rs` 的 `sync_now` / 调度循环 / `apply_sync_report` 换用 session；`sync://conflict` 事件。
- 总冒烟清单：单端 push→pull、双端同时改（挂起 → 裁决 → 再同步收敛）、
  首启设库密码建库、新设备扫码后输库密码直接可用、错误密码被拒可重试、
  改密码后另一设备输新密码可解、v1 存量库升级迁移、legacy 迁移且旧文件已删、
  断网中断 push 后重试、**全设备同版本升级纪律**、百度自动同步连续两轮无报错（修问题①的验收）。

## 四、明确不做（本期）

- 增量日志 meta（log_{start}_{end}）→ blob + meta 快照已覆盖需求，量级不需要。
- 孤立 blob 清理、选择性同步、目录过滤、按需下载 → 多文件泛化时再说。
- 墓碑机制（区分"删除"与"另一侧新建"）→ v3 演进，冲突合并先按"存在即保留"。
- 冲突"自动 LWW"模式开关 → 拍板为用户裁决，不加配置项；有需求再说。

## 五、已拍板决策（2026-11-01）

| 待决项 | 决定 | 落地 |
|---|---|---|
| master.key / 包裹件 | **全部取消：库密码派生密钥**（当日四次拍板定稿） | §2.8：Argon2id(库密码) 直接加密整库，信封自带盐与参数随库走；本机 keyring 缓存免输密码，新设备凭密码即用、零导入；忘记密码不可找回；云端泄露可离线爆破（强 KDF 参数 + 强制 ≥8 位缓解，改密码可选清理云端历史） |
| legacy 远端 vault.json | **删除** | §2.7：genesis meta 确认可 list 后删除；失败补删重试；全设备一起升级 |
| 冲突交互 | **v2 用户裁决** | §2.9：引擎只挂起不裁决，ResolveConflictModal 三选 + 逐条改选，含记录级合并 |
