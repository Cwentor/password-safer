# 扫码登录合并重构 — 计划与进度

> 日期：2026-09-28 · 分支 master · 状态：**代码完成 ✅（20 测试通过、构建零警告），待手工冒烟与提交**
> 前序：云同步策略深化已完成（见 `sync-engine-refactor-plan.md`）；本文件是体检报告卡片 02 的落地。
>
> 实施结果：四个平台文件 952 行 → 45 行薄壳；新增 `shared/login.rs` 464 行（一份实现 + 两个 spec 工厂）。

## 一、问题与目标

「开 WebView → 注入反检测脚本 → 轮询 Cookie → 校验 → 写配置 → 发事件 → 关窗」这条链在
4 个文件里逐行复制（952 行中约 900 行是拷贝）：

| 文件 | 行数 | 真实差异 |
|---|---|---|
| `desktop/auth/quark.rs` | 397 | + JS 轮询脚本、`save_quark_cookie` 命令、`quark-cb` URI 回调（保存块写了 3 遍） |
| `desktop/auth/baidu.rs` | 192 | 无 |
| `mobile/auth/quark.rs` | 186 | UA 字符串、窗口尺寸 |
| `mobile/auth/baidu.rs` | 177 | UA 字符串、窗口尺寸 |

目标：一份参数化的**扫码登录模块**（`shared/login.rs`）+ 四份配置，四个平台文件删成薄壳命令。

## 二、设计

### 模块形状（`src-tauri/src/shared/login.rs`）

```
WebviewLoginSpec（四份配置的唯一形态）
  provider / window_label / login_url / cookie_marker / cookie_domain
  expiry_days / window_title / success_event / log_tag
  js_poll_command: Option<&str>      ← 仅桌面夸克 Some("save_quark_cookie")，其余 None
  login_path_regex: &'static str     ← 仅 JS 轮询用（夸克 /\/account\/login/）

open_login_window(app, spec) -> Result<(), String>      // async；建窗 + 启动 probe 线程
save_login_cookie(app, spec, cookie) -> Result<(), String>
  // 校验 marker → 写对应配置字段 → 重置该网盘过期提醒标记 → emit 事件 → 关窗
  // 探询线程保存块、save_quark_cookie 命令、quark-cb 回调 三处共用这一个入口
register_quark_callback_scheme(builder)                  // #[cfg(desktop)]，桌面 mod.rs 调用
```

- **平台差异收进模块内的 `#[cfg]` 常量**：UA（桌面 Windows Chrome 138 / 移动 Android 13 Chrome 138 Mobile）、
  窗口尺寸（桌面 420×640 min 360×540 + center + decorations；移动 420×720 min 360×600）。
  `center/decorations/open_devtools` 仅在 `#[cfg(desktop)]` 分支调用，移动路径与现状逐行等价。
- **probe 线程**：30 次 × 2s 轮询 `cookies_for_url()` → 拼 Cookie 串 → 含 marker 且长度 >50 → `save_login_cookie`。
  桌面夸克 probe 里的 eval 调试输出（纯 console.log，无行为）删除。
- **保存块三合一**：`save_login_cookie` 成为唯一保存入口，消灭 desktop quark 的 3 份拷贝；
  过期提醒标记重置按 provider 对号（百度登录从此也重置 `baidu_cookie_warn_sent`——候选 01 已泛化该标记）。

### 实施中拍板的四个行为微调（记录在案）

1. 窗口标题统一为「XX网盘 · 扫码登录」（移动端原为「· 登录」，标题在移动端几乎不可见）。
2. `quark-cb` 回调仅在 Cookie 有效时 emit `quark-login-success`（旧行为对无效 Cookie 也发事件，前端会把垃圾 Cookie 当登录成功）。
3. `save_login_cookie` 的配置加载统一 `unwrap_or_default()`（原 save_quark_cookie 用 `?`，失败即整个命令报错；统一后更稳）。
4. 移动端日志前缀去掉 `/mobile` 后缀（纯日志差异）。

### 顺带收益

- 「Cookie 是否已登录」判断从 8 处收敛到 2 处（`save_login_cookie` 校验 + probe 线程 marker 检查）。
- `+45/+60 天`过期魔数收敛到 spec 的 `expiry_days`。
- `quark_cookie_warn_sent` 的 4 处重置收敛到 `save_login_cookie` 一处。

## 三、步骤（全部完成）

1. [x] 新建 `shared/login.rs`（spec、开窗、probe、保存、JS 脚本生成、URI 回调）。
2. [x] `shared/mod.rs` 注册模块。
3. [x] `desktop/auth/quark.rs`：397 行 → 20 行薄壳（`open_quark_login`/`save_quark_cookie`/`register_uri_scheme`）。
4. [x] `desktop/auth/baidu.rs`：192 行 → 9 行薄壳。
5. [x] `mobile/auth/quark.rs`、`mobile/auth/baidu.rs`：186/177 行 → 各 8 行薄壳。
6. [x] `cargo test`（20 个测试不受影响）+ `cargo build` 零警告（一次通过）。

实施备注：`save_quark_cookie` 命令签名去掉 `State` 注入参数（内部经 `app.state()` 获取），
前端 `invoke('save_quark_cookie', { cookie })` 调用方式不变。

## 四、验收对照（体检报告卡片 02 的承诺）

- [x] 约 900 行复制代码删除（995 → 509 总行数；实现只剩一份）
- [x] 改一处反爬脚本 / 探询节奏，四个入口同时生效（`INIT_SCRIPT_TEMPLATE` + `POLL_SECTION` 参数化）
- [x] Cookie 有效性判断与过期魔数收敛（marker 校验 2 处、`expiry_days` 进 spec）
- [x] `quark_cookie_warn_sent` 散布重置归位（`save_login_cookie` 一处，且百度登录现在也重置自己的标记）
- [x] 命令注册表、事件名、前端零改动
- [ ] 手工冒烟（桌面扫码登录两个网盘 + 手动粘贴 Cookie 路径）+ 提交
