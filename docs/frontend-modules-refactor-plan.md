# 前端三刀合并重构 — 密码簿模块 / 共享界面模板 / 筛选接缝

> 日期：2026-09-28 · 分支 master · 状态：**代码完成 ✅（cargo 20 测试通过、Rust 零警告、vite build 通过），待手工冒烟与提交**
> 前序：云同步策略（卡片 01）、扫码登录（卡片 02）已完成。本文件落地体检报告卡片 03 / 04 / 05。
> 实施顺序：05（最小、独立）→ 03（密码簿服务）→ 04（共享模板），每步后可独立验证。

## 卡片 05：筛选接缝二选一 —— 删掉假接缝（后端）

**决策：方案 A，删掉后端查询命令**，承认「数据量小、本地全量筛选」的现状（PasswordList.js
在浏览器内过滤全量明文列表）。grep 复核：五个命令前端零调用。

- 删除 `lib.rs`：`search_passwords` / `get_passwords_by_tag` / `get_favorites` /
  `get_weak_passwords` / `check_sync_connection`（含桌面、移动两处注册）。
- 删除 `JsonStore`：`search` / `get_by_tag` / `get_favorites` / `get_weak`
  （仅被上述命令使用；`toggle_favorite` 有前端调用，保留）。
- 调整 `json_store.rs` 测试 `crud_roundtrip`（改用 `get_all` 断言收藏位）。

## 卡片 03：密码簿模块 —— 增删改 + 刷新约定收进一处（前端）

**现状复核（比体检更严重）**：`passwordService.js` 只有 `toggleFavorite` 一个函数，
且**没有任何文件 import 它**——LongPressMenu 直接从 api.js 拿同名函数并逐行重抄。
增删改后的刷新序列（改 state → 渲染列表/计数/标签云/存储信息 → toast）手搓于
AddModal、DetailPanel（改/删）、LongPressMenu（删/藏）、SettingsModal（恢复/导入/
从云恢复）等 6+ 处；两份删除拷贝已分叉（DetailPanel 关详情面板，LongPressMenu 不关）。

**设计**：`passwordService.js` 成为密码记录增删改的唯一入口：

```
refreshAll()                 // 统一刷新序列：renderPasswordList + updateCounts
                             //   + renderTagsCloud + updateStorageInfo
createPassword(input)        // api → unshift 进 state → refreshAll → toast「密码已保存」
updatePassword(id, input)    // api → 替换 state → refreshAll → toast「已保存」
deletePassword(id)           // confirm（唯一文案）→ api → 过滤 state → refreshAll
                             //   → toast「密码已删除」→ 返回是否已删
toggleFavorite(id)           // 原有逻辑保留；LongPressMenu 改走此处
```

- **分叉的根因归位**：「关闭详情面板」是视图行为，不进服务——DetailPanel 删除成功后
  自己 `closeDetailCard()`，LongPressMenu 本就没有详情面板。服务只负责数据 + 刷新 + 提示，
  confirm 文案收进服务（两处原本逐字相同）。
- **全量重载路径**：`dataLoader.loadPasswords` 成功/失败分支改调 `refreshAll()`；
  SettingsModal 的从云恢复 / sync://restored / 导入三处手搓序列收敛为一句 `loadPasswords()`。
- 顺带：`DetailPanel.js:233` 明文密码嵌内联 onclick 的隐患改用编辑态已有的按 ID 取值
  模式（复制按钮改查 `#detailPassword` 的 data-* 或专用机制——见实施细节）。

## 卡片 04：共享界面模板 —— 桌面/移动共用一份模态 HTML（前端）

**现状复核**：两份编排器逐行对照，7 组 HTML 中除 2-3 个 CSS 类与标签页集合外完全相同；
两平台各差 2 处文案（存储卡片标题、关于页描述）。`renderers` 三个槽位零使用；
SettingsModal 的 `updateSyncUI` 查询不存在的 `.status-dot`，是纯死代码。

**设计**：新建 `src/shared/components/layout.js`：

- `getToastHTML()` / `getEditConfirmDialogHTML(mobile)` / `getAddModalHTML(mobile)` /
  `getSettingsModalHTML(mobile)` / `getSyncProvidersHTML()` / `getCloudSyncModalHTML()` /
  `getQrModalHTML(mobile)` / `getDetailHTML(mobile)` —— DOM ID 全部保持不变
  （共享组件的 getElementById 契约不动），`mobile` 只影响 `mobile-modal(-overlay)` /
  `mobile-detail(-header)` 类名与标签页集合（移动多「云同步」Tab）。
- `mountSharedOverlays({ cloudSync })`：把弹窗/Toast 挂到 body 的公共序列。
- `registerCoreRenderers()`：原两份相同的 `setRenderers({...})` 7 槽注册。
- `initSharedComponents({ cloudSync, longPress })`：公共组件初始化序列（按依赖顺序）。
- `bindOverlayEscape({ keys, before, cloudSync })`：Escape/返回键关浮层栈。
  桌面 `['Escape']` 含云同步弹窗、无二维码弹窗；移动 `['Escape','Backspace']`
  含二维码弹窗、长按菜单优先——与现状逐行为一致。
- 导出 `countTags(passwords)`（从 `renderTagsCloud` 抽出），移动端标签计数复用。

**两处文案统一（有意为之）**：存储卡片标题统一「加密 JSON 存储」；关于页描述统一
桌面措辞「数据采用加密 JSON 文件，仅存储在本地」。

**死代码清理**：`renderer.js` 删掉零使用的 `updateSyncUI`/`renderDetail`/
`reloadConfigAndUI` 槽位；SettingsModal 删掉查询不存在元素的 `updateSyncUI` 函数
及 init 序列中对它的调用；`reloadConfigAndUI` 调用点改直连 dataLoader 导出。

## 验证（已通过）

- `cargo test`：20 个测试全绿（json_store 测试已按计划调整）；`cargo build` 零警告。
- `npm run build`（vite）：桌面 + 移动双入口构建通过。
- 运行时行为需手工冒烟：桌面 + 移动各过一遍 增/删/改/收藏/搜索/标签/导入导出/同步面板/扫码入口。

## 实施结果

| 文件 | 前 | 后 |
|---|---|---|
| `desktop/index.js` | 555 | 103（只剩平台外壳 + Ctrl+F） |
| `mobile/index.js` | 631 | 185（只剩平台外壳 + app-ready 门控 + 骨架屏） |
| 新 `shared/components/layout.js` | — | 494（7 组模板唯一一份 + 挂载/注册/初始化/关浮层栈） |
| `shared/lib/passwordService.js` | 25（零引用） | 71（create/update/delete/toggleFavorite/refreshAll，全部调用点接入） |

## 验收对照

- [x] 05：后端接口面收窄 5 命令 + 4 个 JsonStore 查询方法（`escapeQuotes` 死工具函数顺带删除）；筛选语义只剩 PasswordList 一处
- [x] 03：刷新约定单点化（`refreshAll`）；两份分叉的删除拷贝合并（confirm 文案进服务）；passwordService 从零引用变成唯一入口；「关闭详情面板」归位为视图行为
- [x] 04：约 700 行重复模板删除；「加一个同步字段改 4 处」降为改 layout.js 一处
- [x] 死代码（updateSyncUI/.status-dot、三个空槽位）清除
- [x] 明文密码内联 onclick 隐患消除（复制按钮改读元素/变量，不进 HTML 属性）
- [x] cargo + vite 构建全绿
- [ ] 手工冒烟 + 提交
