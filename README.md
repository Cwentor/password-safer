# Password Safer

> 本地优先 · 端到端加密 · 双网盘云同步的 Windows 桌面密码保管箱

![version](https://img.shields.io/badge/version-v2.1.0-cyan?style=flat-square)
![license](https://img.shields.io/badge/license-GPL%20v3-blue?style=flat-square)
![platform](https://img.shields.io/badge/platform-Windows%20%7C%20Android%20(Phase%202)-blueviolet?style=flat-square)
![tauri](https://img.shields.io/badge/Tauri-2.x-orange?style=flat-square)
![rust](https://img.shields.io/badge/Rust-edition%202021-dea584?style=flat-square)

Password Safer 是一款基于 **Tauri 2 + Rust** 构建的 Windows 桌面密码管理应用。所有密码数据以 **AES-256-GCM** 加密后存储在本地 JSON 文件中，密钥与数据库文件分离保存；支持**百度网盘**与**夸克网盘**双向/单向云同步，无需用户登录注册，开箱即用。

**v2.1.0 亮点**：加密 JSON 存储迁移（SQLite → vault.json，整体 AES-256-GCM 加密）、SQLite 依赖彻底移除（桌面与安卓编译体积优化，安卓端不再交叉编译 libsqlite3-sys）、Android 端正式落地（架构重构 + 完整实现 + 网盘云同步支持）。继承 v2.0.0 能力：夸克网盘双向同步、应用内扫码登录、Cookie 过期预警、无边框沉浸式窗口。

---

## 目录

- [功能特性](#功能特性)
- [技术栈](#技术栈)
- [快速开始](#快速开始)
- [项目结构](#项目结构)
- [数据存储](#数据存储)
- [同步机制](#同步机制)
- [数据库导入/导出](#数据库导入导出)
- [配置参数一览](#配置参数一览)
- [已知限制](#已知限制)
- [国内网络配置](#国内网络配置)
- [许可证](#许可证)

---

## 功能特性

### 🔒 安全

- **AES-256-GCM 加密**：认证加密，密码字段密文存储
- **密钥与数据分离**：`master.key` 与 `vault.json` 独立存放，提升物理安全性
- **密码强度评估**：自动评分 1-4 级（长度 + 大小写 + 数字 + 符号）
- **密码生成器**：内置强密码生成，长度可配置

### 🗂 管理

- **模糊搜索**：覆盖名称、用户名、网址、标签、备注，Ctrl+F 快捷键
- **标签分类**：自定义标签云（工作 / 社交 / 金融 / 电商 / 开发 / 娱乐 / 云服务）
- **收藏标记**：快速访问常用密码
- **一键复制**：点击即复制密码到剪贴板
- **多维排序**：按名称 / 最近使用 / 创建时间 / 密码强度

### ☁️ 云同步

| 网盘 | 鉴权 | 上传 | 下载 | 双向同步 | 扫码登录 |
|------|------|------|------|----------|----------|
| **夸克网盘** | HttpOnly Cookie | ✅ 分片 + 断点续传 + 秒传 | ✅ | ✅ mtime 比对 | ✅ WebView 扫码 |
| **百度网盘** | Web Cookie + bdstoken | ✅ precreate→upload→create | ✅ | ❌ 单向上传 | ✅ API 扫码 |

- **定时自动同步**：后台调度器按间隔（默认 5 分钟）轮询
- **Cookie 过期预警**：夸克 Cookie 距过期 <7 天时弹窗提醒，过期自动停用同步
- **手动同步**：设置页面点击「立即上传」/「从云恢复」

### 🖥 体验

- **无边框沉浸式窗口**：自定义系统标题栏与边框隐藏
- **自绘标题栏**：支持拖拽移动、最小化 / 最大化 / 关闭按钮、双击切换最大化
- **天青色渐变主题**：Cyan / Teal 配色，三栏式布局（侧栏 / 主区 / 详情）
- **键盘快捷键**：`Ctrl+F` 聚焦搜索，`Esc` 关闭弹窗
- **本地存储用量可视化**：侧栏实时显示数据库占用

### 💾 数据

- **数据库导出**：将当前 `vault.json` 复制到任意路径
- **数据库导入**：从外部 JSON 文件反向导入（支持加密格式与明文格式）
- **自动图标映射**：按名称识别常见站点（GitHub → 🐙 等）

### ⚠️ 待完善

- **编辑密码**：后端 `update_password` 命令已实现，前端 UI 待接线
- **自动锁定**：配置项已就绪（默认 30 分钟），闲置定时逻辑待实现
- **剪贴板自动清空**：配置项已就绪（默认 30 秒），定时清空逻辑待实现

---

## 技术栈

| 组件 | 技术 | 说明 |
|------|------|------|
| 桌面框架 | Tauri 2.x | 跨平台桌面 / 移动应用框架 |
| 后端语言 | Rust (edition 2021) | 核心业务逻辑（shared / desktop / mobile 三层） |
| 加密 | aes-gcm 0.10 | AES-256-GCM 认证加密 |
| HTTP 客户端 | reqwest 0.12 (blocking + rustls) | 同步 HTTP，避免 OpenSSL 依赖 |
| 异步运行时 | tokio 1.x (full) | 同步调度器与扫码轮询 |
| 二维码 | qrcode 0.14 + image 0.25 | 扫码登录二维码渲染 |
| 哈希 | sha1 0.10 + md-5 0.10 | 夸克上传秒传校验 |
| 前端 | 原生 HTML / CSS / JS (ES Modules) | Vite 5 构建，组件化，无框架依赖 |
| 平台检测 | @tauri-apps/plugin-os | 桌面 / 安卓平台识别与布局分发 |
| 应用唤起 | @tauri-apps/plugin-shell | 安卓端 Deep Link 与外部唤起支持 |
| UI 主题 | 天青色渐变 (Cyan / Teal) | 自定义无边框窗口 |

---

## 快速开始

### 1. 安装环境依赖

#### Rust 工具链

访问 [https://rustup.rs](https://rustup.rs) 下载并安装。

**Windows 上 Rust 有两种工具链可选**：

| 工具链 | 需要的额外组件 | 说明 |
|--------|---------------|------|
| `stable-x86_64-pc-windows-msvc` | Visual Studio C++ Build Tools | 官方推荐，编译速度快 |
| `stable-x86_64-pc-windows-gnu` | 无需额外组件（需 MinGW） | 安装更简单 |

如果选择 MSVC 工具链，需安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选「使用 C++ 的桌面开发」。

如果不想安装庞大的 MSVC，可切换 GNU 工具链：

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

#### Node.js

访问 [https://nodejs.org](https://nodejs.org/) 下载 LTS 版本安装。

### 2. 构建运行

```powershell
# 进入项目目录
cd d:\Program\password-safer

# 方式一：使用构建脚本（推荐）
.\build.ps1              # 开发模式（热重载）
.\build.ps1 -Build       # 生产构建（生成安装包）
.\build.ps1 -Check       # 仅检查环境

# 方式二：手动命令
npm install              # 安装前端依赖
npm run tauri dev        # 开发模式
npm run tauri build      # 生产构建
```

构建产物位于 `src-tauri/target/release/bundle/`：
- `nsis/` — NSIS 安装程序
- `msi/` — MSI 安装包

### 2.1 Android 构建（Phase 2，需 Android SDK）

Android 端完整功能将在 Phase 2 实现，当前已就绪多端架构骨架：
- 后端 `src-tauri/src/mobile/` 模块骨架（auth / notification 占位）
- 前端 `src/mobile/` 布局骨架
- Tauri Android 工程配置（`capabilities/mobile.json`、`tauri-plugin-os` / `tauri-plugin-shell`）

**前置条件**：
1. 安装 [Android Studio](https://developer.android.com/studio) + SDK + NDK
2. 设置环境变量 `ANDROID_HOME`（指向 SDK 根目录），可选 `NDK_HOME`
3. 首次运行需初始化 Android 工程：`npx tauri android init`

```powershell
# 使用构建脚本（推荐）
.\build.ps1 -Android              # 安卓开发模式（需连接设备 / 模拟器）
.\build.ps1 -Android -Build       # 安卓生产构建（生成 APK / AAB）
.\build.ps1 -Android -Check       # 仅检查 Android 环境

# 手动命令
npm run tauri:android:dev         # 开发模式
npm run tauri:android:build       # 生产构建
```

构建产物位于 `src-tauri/gen/android/app/build/outputs/`。

> ✅ Android 工程已生成并就绪：`src-tauri/gen/android/` 目录已存在，可直接执行上述构建命令。首次构建前请确保已安装 Android SDK/NDK 并完成环境配置。

### 3. 配置云同步

应用启动后，点击右上角设置图标进入设置页面 → 「云同步」标签，选择网盘并点击「扫码登录」（夸克）或「扫码登录」（百度）即可自动获取 Cookie。详细配置说明请参阅 [CONFIG.md](./CONFIG.md)。

---

## 项目结构

项目采用 **前后端三层架构**（`shared/` + `desktop/` + `mobile/`），共享代码集中管理，平台专属代码独立隔离，便于桌面端与安卓端并行演进。

```
password-safer/
├── index.html                    # Vite 入口模板
├── package.json                  # npm 依赖配置
├── vite.config.js                # Vite 构建配置
├── build.ps1                     # 构建脚本（支持桌面 / 安卓）
├── CONFIG.md                     # 配置参数说明文档
├── LICENSE                       # GNU General Public License v3
├── README.md                     # 本文档
├── src/                          # 前端源码（组件化 ES Modules）
│   ├── main.js                   # 入口：平台检测 + 动态加载 desktop/mobile
│   ├── shared/                   # 跨平台共享
│   │   ├── components/           # 共享组件（PasswordList / DetailPanel / AddModal 等）
│   │   ├── lib/                  # 工具库（api / platform / utils / state 等）
│   │   └── styles/               # 设计令牌（tokens.css）+ 基础样式（base.css）
│   ├── desktop/                  # 桌面端专属
│   │   ├── components/           # TitleBar / Sidebar
│   │   ├── styles/               # 桌面布局样式（desktop.css）
│   │   └── index.js              # 桌面三栏布局组装
│   └── mobile/                   # 安卓端专属（Phase 2 完整实现）
│       ├── components/           # 底部导航等（Phase 2）
│       ├── styles/               # 移动布局样式（mobile.css）
│       └── index.js              # 移动布局骨架
└── src-tauri/
    ├── Cargo.toml                # Rust 依赖配置
    ├── tauri.conf.json           # Tauri 应用配置
    ├── build.rs                  # Tauri 构建脚本
    ├── icons/                    # 应用图标
    ├── .cargo/
    │   └── config.toml           # Cargo 镜像配置（国内加速）
    ├── capabilities/             # Tauri 权限配置
    │   ├── default.json          # 桌面端权限
    │   ├── quark-login.json      # 夸克登录 WebView 远程页面权限
    │   └── mobile.json           # 安卓端权限（os / shell 等）
    └── src/
        ├── main.rs               # 程序入口
        ├── lib.rs                # 平台分发入口 + 共享命令注册
        ├── shared/               # 跨平台共享代码
        │   ├── models.rs         # 数据模型（PasswordDto / AppConfig）
        │   ├── crypto.rs         # AES-256-GCM 加密
        │   ├── sync/             # 云同步（夸克 / 百度）
        │   └── storage/          # 加密 JSON 存储模块（json_store.rs / mod.rs）
        ├── desktop/              # 桌面专属代码
        │   ├── tray.rs           # 系统托盘
        │   ├── window.rs         # 窗口控制（CloseRequested 拦截等）
        │   └── auth/             # WebView 扫码登录（夸克 / 百度）
        └── mobile/               # 安卓专属代码（Phase 2）
            ├── auth/             # Deep Link 登录
            └── notification.rs   # 前台服务通知
```

> 安卓端 `src-tauri/gen/android/` 工程由 `tauri android init` 生成，已加入 `.gitignore`。

---

## 数据存储

| 文件 | 位置 | 说明 |
|------|------|------|
| `vault.json` | `%APPDATA%\password-safer\` | 加密 JSON 数据文件 |
| `master.key` | `%APPDATA%\password-safer\` | 256 位加密密钥 |

> **⚠️ 密钥安全提醒**
>
> `master.key` 是解密密码的**唯一凭证**。请单独备份此文件。
> - 如果密钥丢失，已加密的密码将**无法恢复**。
> - 请勿将 `master.key` 和 `vault.json` 放在同一位置。
> - 网盘同步仅上传 `vault.json`，**不会上传** `master.key`。

### 加密范围

- 加密算法：**AES-256-GCM**（认证加密）
- 加密字段：仅 `password_encrypted` 字段加密
- 明文字段：名称、用户名、网址、标签、备注等（用于搜索筛选）
- 密钥生成：首次运行时随机生成 256 位密钥

---

## 同步机制

采用**文件级同步**策略：将整个加密 JSON 文件上传/下载到网盘，而非同步单条记录。

### 百度网盘（单向上传）

- 鉴权：Web Cookie + bdstoken（自动从 cookie 获取）
- 上传流程：`precreate` → 分片 `upload` → `create`
- 下载：通过 `d.pcs.baidu.com` 直接下载
- **不支持双向同步**（未实现 `remote_file_mtime`）

### 夸克网盘（双向同步）

- 鉴权：HttpOnly Cookie（通过 `cookies_for_url()` 获取完整 cookie）
- 上传流程：`file/upload/pre` → `file/update/hash`（秒传判断）→ 分片 PUT → `commit` → `finish`
- 断点续传：`.uploadmeta` 元文件记录进度
- 冲突处理：23008 doloading 状态自动复用现有目录 fid
- **双向同步逻辑**：
  1. 本地无效 → 下载云端恢复
  2. 本地有效 → 比较本地 mtime 与云端 mtime，新者覆盖旧者
  3. mtime 相等 → 跳过

### 自动同步调度器

- 后台 `tauri::async_runtime::spawn` 死循环，按 `*_sync_interval`（默认 300 秒）轮询
- 同步前校验 Cookie 有效性（`QuarkAuth::validate` / `BaiduAuth::validate`）
- Cookie 距过期 <7 天 → emit `sync://cookie_expiring` 事件
- Cookie 失效 → 自动关闭同步 + emit `sync://expired` 事件
- 同步异常 → emit `sync://error` 事件

### 多设备使用流程

1. 设备 A 创建/修改密码 → 自动上传到网盘
2. 设备 B 打开应用 → 手动「从云恢复」下载 → 覆盖本地数据

> **⚠️ 冲突提醒**
>
> 文件级同步不支持增量合并，多设备同时修改可能冲突。建议同一时间只在一台设备上编辑。

---

## 数据库导入/导出

### 导出

将当前 `vault.json` 复制到指定路径，包含全部加密数据。

### 导入（反向转换）

从外部 JSON 文件读取密码记录，导入到当前数据文件：

- 自动验证数据结构（需含 `passwords` 字段）
- 尝试解密（如果是本应用导出的加密格式）
- 解密失败则当作明文处理
- 重新加密后写入当前数据文件

---

## 配置参数一览

> 完整配置说明请参阅 [CONFIG.md](./CONFIG.md)。下表仅列出关键字段。

### 百度网盘

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `baidu_cookie` | 空 | 百度网盘登录 Cookie（应用内扫码登录自动获取） |
| `baidu_remote_path` | `/apps/VAULT/vault.json` | 网盘存储路径 |
| `baidu_sync_enabled` | `false` | 是否启用百度同步 |
| `baidu_sync_interval` | `300` | 自动同步间隔（秒） |
| `baidu_cookie_expires_at` | `0` | Cookie 过期时间戳 |

### 夸克网盘

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `quark_cookie` | 空 | 夸克网盘登录 Cookie（含 HttpOnly） |
| `quark_remote_path` | `/VAULT/vault.json` | 网盘存储路径 |
| `quark_sync_enabled` | `false` | 是否启用夸克同步 |
| `quark_sync_interval` | `300` | 自动同步间隔（秒） |
| `quark_cookie_expires_at` | `0` | Cookie 过期时间戳 |
| `quark_last_remote_mtime` | `0` | 上次同步时云端文件 mtime（双向同步用） |

### 通用设置

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `auto_lock_minutes` | `30` | 闲置自动锁定时间（配置占位，逻辑待实现） |
| `clipboard_clear_seconds` | `30` | 复制后剪贴板清空时间（配置占位，逻辑待实现） |
| `master_password` | 空 | 主密码（可选额外保护） |

---

## 已知限制

1. **编辑密码**：后端 `update_password` 命令已实现并注册，前端 UI 仍显示「编辑功能开发中」，待接线
2. **冲突处理**：文件级同步不支持增量合并，多设备同时修改可能冲突
3. **百度双向同步**：当前百度仅支持单向上传，未实现 `remote_file_mtime`，不支持下载恢复（夸克已支持）
4. **自动锁定 / 剪贴板自动清空**：配置项已就绪，定时触发逻辑待实现
5. **夸克网盘非官方接口**：依赖 Web 端 Cookie，可能随夸克版本更新失效
6. **安卓端完整功能待 Phase 2 实现**：当前 Phase 1 已完成多端架构骨架（后端 `mobile/` 模块、前端 `mobile/` 布局、Tauri Android 工程配置与权限），完整移动端 UI、Deep Link 登录、前台服务通知等原生功能将在 Phase 2 实现

---

## 国内网络配置

项目已配置 [rsproxy.cn](https://rsproxy.cn) 镜像加速（`src-tauri/.cargo/config.toml`），无需额外设置。

```toml
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

如遇网络问题，可修改上述配置切换其他镜像。

---

## 许可证

本项目采用 **GNU General Public License v3 (GPLv3)** —— 强 copyleft 开源协议，允许自由使用、研究、修改与分发（含商业用途），但**任何衍生作品必须以 GPLv3 协议开源**，且必须提供完整对应源代码。

完整协议文本见 [LICENSE](./LICENSE)。

### GPLv3 核心义务

- ✅ **可自由使用**：包括商业用途
- ✅ **可修改**：但修改后的版本必须同样以 GPLv3 开源
- ✅ **可分发**：无论免费或付费
- ❗ **衍生作品传染**：任何基于本软件的衍生作品必须采用 GPLv3 或兼容协议
- ❗ **源代码公开**：分发软件时必须向接收者提供完整对应源代码
- ❗ **保留版权声明**：不得移除原作者版权声明与协议声明

> 商业使用无需另行授权，但需严格遵守上述义务。如需将本软件整合入闭源商业产品，请联系作者协商商业许可。

---

<p align="center">
  <sub>Built with Tauri 2 · Rust · Vite · Made for Windows · Android (Phase 2)</sub>
</p>
