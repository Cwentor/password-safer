# 追加式云同步 — 远端不可覆盖、仅允许新增的设计与实施计划

> 日期：2026-11-01 · 分支 `feat/append-only-sync` · 状态：**计划待评审，未开工**
> 背景约束：远端网盘（夸克 / 百度）限制为**不能覆盖重写，只能上传全新文件**；
> 目标：任意新接入客户端一键拉到全量最新状态，多端并发可收敛、历史可追溯。
> 前置阅读：现有同步为 vault.json 整文件 mtime 新者覆盖（`engine.rs::decide_direction`），
> 夸克实现是"删旧传新"（`quark.rs:609-623`），百度是 precreate→superfile2→create(rtype=3)。

## 一、现状复核（以代码为准）与新方案的差距

| # | 现状 | 追加式远端下的问题 | 本计划的处置 |
|---|---|---|---|
| 1 | `upload` 语义 = 覆盖远端：夸克先 `delete_files` 再传新（`quark.rs:614-623`）；百度 `rtype=3` 覆盖语义（`baidu.rs:161`） | 直接违反"不可覆盖"约束 | trait 语义改为**仅新增**；文件名内容寻址/时间戳+设备号，天然不撞名 |
| 2 | 方向判定靠 mtime 比较（`engine.rs:25-51`）；百度 `remote_file_mtime` 未实现（trait 默认 Err）→ 百度自动同步每轮必报错 | mtime 不再是可靠的"新"信号 | 判定改为 **head 对比**（manifest 快照链），顺手删掉 `remote_file_mtime`，消灭该 bug |
| 3 | `last_sync` 记账写在 AppConfig 里 = 在 vault.json 内部（`models.rs:42,49`），每次同步后写回（`lib.rs:175-183`）→ 本地 mtime 永远变新 → **每周期重复上传** | 灾难级放大：每周期新增一个 blob + 一个 meta，远端无限膨胀 | 记账搬出 vault.json → 本地 `sync_state.json`（阶段 0，**硬前提**） |
| 4 | 新客户端拉取 = 下载 vault.json，但 master.key 不上云 → 拉到也解不开 | 新方案只解决"拉到最新"，不解决"解得开" | 列为待决问题 §五.1，本期不动 |
| 5 | `JsonStore::open` 解密失败直接拒绝启动，不回退 `.bak` | 与本方案无直接耦合，但新方案依赖本地 vault 可用 | 阶段 0 顺手修 |
| 6 | 测试接缝完善（`SyncProvider`/`VaultIo`/`QrAuthenticator` 三替身，engine 内联测试） | — | 沿用此风格，新增 FakeAppendOnlyProvider |

## 二、设计（对原思考稿的采纳与修正）

### 2.1 采纳的部分

- **blob 内容寻址**：vault 内容本来就是 E2E 密文，`blob 名 = SHA-256(密文)`，同内容只传一次。
  去重无需扫 /blobs 目录——夸克 `file/update/hash`（finish=true）与百度 precreate slice-md5
  都支持哈希秒传，push 时零成本判定远端是否已有该 blob。
- **元数据与实体分离**：远端只存不可变 blob；"最新状态"由 meta 快照维护；客户端先拉 meta 再按需拉 blob。
- **两段式 push**：先传 blob（崩溃留孤儿 blob，无害，下次秒传复用），meta 后传 = 提交点。
- **删除即标记**：meta 里 `deleted: true`，blob 永不删除（单文件密码簿没有"文件删除"，此语义为 files map 泛化预留）。

### 2.2 修正的部分（原稿的坑）

1. **"列出 /meta 取版本号最大"有并发漏洞**：没有服务器分配单调版本号，两个客户端并发 push
   会产生两个同号/乱序 meta。修正：meta 文件名 `meta_<UTC毫秒>_<device_id>.json`（唯一、可排序），
   meta 内带 `supersedes` 字段记录它已知的一切 head。**head 集合 = 未被任何 meta 的 supersedes
   引用的 meta**。fork（多 head）时按 `(updated_at, device_id)` 字典序确定性裁决——所有客户端
   对同一组 head 收敛出同一个胜者，避免同秒冲突时各端各说各话。
2. **冲突败者必须可找回**：胜者直接上位，败者内容先上传为 blob 并以 `kind: "backup"` 的 meta
   钉进命名空间（backup meta **不参与 head 解析**，纯粹是给 blob 一个可寻址的名字）。
   原"策略 2 双存备份"落到实现就是这个机制，且用户可在后续版本做恢复 UI。
3. **增量日志 log_{start}_{end} 不做**：单文件密码簿的 meta 全量快照只有几百字节，
   操作日志是过度设计；blob 本身已天然构成历史。列为泛化后续，不进本期。
4. **定时清理孤立 blob 不做**：网盘 API 实际支持删除（夸克 `delete_files` 在用），
   但历史版本在密码管理器场景是特性（误覆盖找回），量级可忽略（几 KB/次 × 手动频率）。
5. **选择性同步 / 目录过滤 / 按需下载不做**：单文件 vault（几 KB～几 MB）用不上，属多文件泛化后续。
6. **时钟偏差说明**：LWW 裁决按 `(updated_at, device_id)`，时钟严重偏慢的设备永远输——
   与原稿"全局时间戳后操作覆盖前操作"性质相同，文档化为可接受限制。

### 2.3 远端布局（沿用现有 base 目录）

```
/apps/VAULT/ （百度） | /VAULT/ （夸克）
├── meta/meta_<UTCms>_<device>.json    # 提交点，唯一"最新状态"来源
├── blobs/<sha256>.bin                 # 不可变密文 blob，内容寻址
└── vault.json                         # legacy，迁移期读取一次后不再写（保留不删）
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

`files` 用 map 是给未来多文件（master.key 包裹件、附件、导出包）留的扩展位。

### 2.4 本地新增 sidecar：`sync_state.json`（vault.json 同目录，不上云）

```json
{
  "device_id": "a1b2c3d4",
  "last_heads": { "quark": ["meta_....json"], "baidu": [] },
  "local_dirty_at": 1730000000000,
  "last_sync": { "quark": "2026-11-01 12:00", "baidu": "" },
  "conflict_backups": ["<sha256hex>"]
}
```

- `device_id`：首启随机生成，标识本设备（meta 命名与 LWW tie-break 用）。
- `local_dirty_at`：`JsonStore::save`（用户写路径）时刷新；`replace_raw`（同步写路径）不刷新——
  这是区分"用户改过"与"同步落下"的关键，冲突裁决靠它。
- `last_sync` 从 AppConfig 迁来，前端展示改走新 command（见阶段 0）。

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
}
```

- `remote_file_mtime` 删除（连同 `decide_direction` 的 mtime 语义）。
- 夸克：upload 去掉 delete 分支；list 复用 `file/list` 分页（`find_file_fid` 已在用同一接口）。
- 百度：实现 list（`/api/file/list`，web=1 分页）；`rtype=3` 在唯一文件名下永不触发，等效 append-only。

### 2.6 核心流程

**push（本地有变更）**
1. list `/meta` → 解析 head 集合；
2. 若存在本地 `last_heads` 之外的 head → 走冲突分支（下）；
3. 计算 vault 密文 sha256；等于当前 head 引用的 blob → 无需 push，更新 state 收尾；
4. 传 blob（秒传命中即跳过）→ 传 meta（`supersedes` = 已知全部 heads）→ 更新 `last_heads`。

**pull（含首装）**
1. list `/meta` → head 集合；空且 legacy vault.json 存在 → 迁移（§2.7）；空且无 legacy → 空库跳过；
2. head 集合 ⊆ `last_heads` → 无新，跳过；
3. 多 head（fork）→ `(updated_at, device_id)` 裁决出胜者；
4. 下载胜者 blob → sha256 校验 → `replace_raw`（已有验密 + 原子替换 + .bak）→ 更新 `last_heads`。

**冲突（push 时发现远端有未见过的 head，且本地 vault 哈希 ≠ 任何 head 的 blob）**
- `local_dirty_at > 胜者 head.updated_at` → **本地胜**：本地内容作为新 head 正常 push（supersedes 全部）。
- 反之 → **远端胜**：本地内容先传 blob + 写 `kind: "backup"` 的 meta（不抢 head），
  再 pull 胜者，向用户报告"检测到冲突：已应用云端版本，本地版本已备份（可找回）"。

**幂等与崩溃安全**
- blob 传完、meta 未传 → 孤儿 blob，无害（下次同内容秒传命中）；
- meta 传完、state 未更新 → 下轮 list 看到自己的 head，`last_heads` 收敛，跳过；
- 下载完未 apply → 下轮重来；apply 前有 sha 校验 + `replace_raw` 验密双保险。

### 2.7 legacy 迁移

远端 `/meta` 为空且旧 `vault.json` 存在：
读旧文件字节 → 传为 blob（同一份密文，秒传大概率直接命中）→ 写 genesis meta（supersedes 空）。
旧 `vault.json` 保留不删（见待决 §五.2）。本地 vault 有效时本地胜出路径同样适用（正常 push 即 genesis）。

## 三、实施阶段（每阶段独立可验证、独立提交）

### 阶段 0：记账搬出 vault.json（硬前提）+ `.bak` 回退
- 新增 `sync_state.json` 读写模块（`shared/storage/sync_state.rs`）：device_id、last_heads、
  local_dirty_at、last_sync、conflict_backups。
- `lib.rs` 删除所有 `cfg.baidu_last_sync/quark_last_sync` 写回（`lib.rs:175-183`、`lib.rs:427-433`）。
- `JsonStore::save` 刷新 `local_dirty_at`（经注入的 state 句柄或回调，保持 store 不依赖 state 文件布局）。
- `JsonStore::open` 解密失败回退 `.bak`（修审计问题③）。
- 新 command `get_sync_state`；`SettingsModal.js` 的 last_sync 展示改走它（`SettingsModal.js:99-100`）。
- 验证：cargo test 全绿；两轮自动同步不产生第二次上传（现有 FakeProvider 日志断言）。

### 阶段 1：Provider 扩展——list + 仅新增语义
- `SyncProvider` 增加 `RemoteFile` + `list()`；删 `remote_file_mtime`。
- 夸克 / 百度分别实现 list；夸克 upload 去 delete 分支；百度 upload 不动（唯一名下等效 append-only）。
- `FakeProvider` 测试适配；新增"upload 不删远端同名"断言。
- 验证：cargo test；真机各手动列目录一次。

### 阶段 2：manifest 模块（纯函数核心，测试密度最高）
- 新增 `shared/sync/manifest.rs`：meta 结构体、命名生成、解析校验、
  **head 解析**（supersedes 集合减法 + 忽略 `kind:"backup"`）、fork 裁决 `(updated_at, device_id)`。
- 单测：正常单链、并发 fork、同秒 device_id tie-break、backup 不参与 head、坏 meta 文件跳过。
- 验证：cargo test（本模块 ≥10 个用例）。

### 阶段 3：session 编排 + 引擎改造
- 新增 `shared/sync/session.rs`：push / pull / 冲突 / genesis / legacy 迁移 / 幂等，
  依赖 `SyncProvider` + `VaultIo` + `SyncStateIo`（新接缝，替身实现）。
- `engine.rs`：`decide_direction(mtime)` → `decide_action(local_state, remote_heads)`；
  Cookie 校验、过期预警、SyncReport 骨架保留不动；新增 `Outcome::ConflictResolved(String)`。
- FakeAppendOnlyProvider（内存 HashMap，upload 撞名即 Err）覆盖 §2.6 全流程与崩溃序列。
- 验证：cargo test（新增 ≥15 用例）。

### 阶段 4：接线、前端与冒烟
- `lib.rs` 的 `sync_now` / 调度循环 / `apply_sync_report` 换用 session；`sync://conflict` 事件。
- 前端：冲突 Toast / 通知（桌面 Toast、移动 notification），文案含备份位置。
- 手工冒烟清单：单端 push→pull、双端同时改（fork 收敛）、首装无 master.key 提示、
  legacy 远端迁移、断网中断 push 后重试、百度自动同步连续两轮无报错（修问题①的验收）。

## 四、明确不做（本期）

- 增量日志 meta（log_{start}_{end}）→ blob + meta 快照已覆盖需求，量级不需要。
- 孤立 blob 清理、选择性同步、目录过滤、按需下载 → 多文件泛化时再说。
- 记录级合并（按 PasswordRecord id 取并集）→ 有价值但改动面大，作为冲突处理的 v2 演进，接口已留好。

## 五、待决问题（不阻塞开工，请拍板）

1. **master.key 上云**：不做则新设备仍是"拷 key + 拉取"两步。可选方案为用户设置"云解锁口令"、
   用其派生密钥包裹 master.key 后作为第二个 files 条目上云。涉及产品定位（零账号、E2E），
   建议单独立项。
2. **legacy 远端 vault.json 的去留**：建议保留（append-only 精神 + 兜底），不清理。
3. **冲突交互**：v1 静默 LWW + 通知备份位置（本计划）；v2 再做让用户二选一的 UI。
