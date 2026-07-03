# Password Safer - 本地加密密码保管箱

基于 Tauri + Rust 的 Windows 桌面密码管理应用。数据本地 SQLite 加密存储，支持百度网盘/夸克网盘定时云同步，无需用户登录注册。

## 功能特性

- **本地加密存储**：AES-256-GCM 加密，密钥与数据库分离存储
- **模糊搜索**：按名称、用户名、网址、标签、备注不完全匹配查询
- **标签分类**：自定义标签管理，支持按标签筛选
- **密码强度**：自动评估密码强度（1-4 级）
- **收藏标记**：快速访问常用密码
- **密码生成**：内置强密码生成器
- **一键复制**：点击即复制密码到剪贴板
- **百度网盘同步**：官方 API 支持，完整的上传/下载（precreate → upload → create）
- **夸克网盘同步**：Cookie 鉴权方式，支持下载（上传需进一步完善）
- **定时同步**：按配置间隔自动上传数据库文件到网盘
- **数据库导入**：从外部 SQLite 文件反向导入数据到应用
- **数据库导出**：导出当前数据库文件到任意路径
- **无需注册**：无用户登录系统，开箱即用

## 技术栈

| 组件 | 技术 |
|------|------|
| 桌面框架 | Tauri 2.x |
| 后端语言 | Rust (edition 2021) |
| 数据库 | SQLite (rusqlite, bundled) |
| 加密 | AES-256-GCM (aes-gcm 0.10) |
| HTTP 客户端 | reqwest 0.12 (blocking + rustls) |
| 异步运行时 | tokio 1.x |
| 前端 | 原生 HTML/CSS/JS (Vite 构建) |
| UI 主题 | 天青色渐变 (Cyan/Teal) |

## 快速开始

### 1. 安装环境依赖

#### Rust 工具链

```
访问 https://rustup.rs 下载并安装
```

**重要**：Windows 上 Rust 有两种工具链：

| 工具链 | 需要的额外组件 | 说明 |
|--------|---------------|------|
| `stable-x86_64-pc-windows-msvc` | Visual Studio C++ Build Tools | 官方推荐，编译速度快 |
| `stable-x86_64-pc-windows-gnu` | 无需额外组件 | 需要 MinGW，安装更简单 |

如果选择 MSVC 工具链，需安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选「使用 C++ 的桌面开发」。

如果不想安装庞大的 MSVC，可以切换 GNU 工具链：
```
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

#### Node.js

```
访问 https://nodejs.org/ 下载 LTS 版本安装
```

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

### 3. 配置云同步

应用启动后，点击右上角设置图标进入设置页面，配置网盘同步参数。

详细配置说明请参阅 [CONFIG.md](./CONFIG.md)。

## 项目结构

```
password-safer/
├── index.html              # 前端 UI（HTML + CSS + JS 内联）
├── package.json            # npm 依赖配置
├── vite.config.js          # Vite 构建配置
├── build.ps1               # 构建脚本
├── CONFIG.md               # 配置参数说明文档
├── README.md               # 本文档
└── src-tauri/
    ├── Cargo.toml           # Rust 依赖配置
    ├── tauri.conf.json      # Tauri 应用配置
    ├── build.rs             # Tauri 构建脚本
    ├── icons/               # 应用图标
    ├── .cargo/
    │   └── config.toml      # Cargo 镜像配置（国内加速）
    ├── capabilities/
    │   └── default.json     # Tauri 权限配置
    └── src/
        ├── main.rs           # 程序入口
        ├── lib.rs            # 核心逻辑：Tauri 命令 + 同步调度器
        ├── models.rs         # 数据模型：PasswordDto, AppConfig 等
        ├── crypto.rs         # AES-256-GCM 加密/解密
        ├── db.rs             # SQLite 数据库 CRUD + 导入
        ├── config.rs         # 配置管理器
        └── sync/
            ├── mod.rs         # 同步 trait + 辅助函数
            ├── baidu.rs       # 百度网盘同步（官方 API）
            └── quark.rs       # 夸克网盘同步（Cookie 方式）
```

## 数据存储

| 文件 | 位置 | 说明 |
|------|------|------|
| `vault.db` | `%APPDATA%\password-safer\` | SQLite 数据库（密码字段 AES 加密） |
| `master.key` | `%APPDATA%\password-safer\` | 256 位加密密钥 |

> **密钥安全提醒**：`master.key` 是解密密码的唯一凭证。请单独备份此文件。如果密钥丢失，已加密的密码将无法恢复。请勿将 `master.key` 和 `vault.db` 放在同一位置。

## 同步机制

采用**文件级同步**策略：将整个 SQLite 数据库文件上传/下载到网盘。

- **自动同步**：后台定时器按间隔（默认 5 分钟）自动上传
- **手动同步**：设置页面点击「立即同步」
- **多设备**：设备 A 上传 → 设备 B 下载恢复

## 数据库导入/导出

### 导出
将当前 `vault.db` 复制到指定路径，包含全部加密数据。

### 导入（反向转换）
从外部 SQLite 文件读取密码记录，导入到当前数据库：
- 自动验证表结构（需有 `passwords` 表）
- 尝试解密（如果是本应用导出的加密格式）
- 解密失败则当作明文处理
- 重新加密后插入当前数据库

## 配置参数一览

| 参数 | 当前值 | 说明 |
|------|--------|------|
| `baidu_app_key` | `[待补充]` | 百度网盘应用 AppKey |
| `baidu_secret_key` | `[待补充]` | 百度网盘应用 SecretKey |
| `baidu_access_token` | `[待补充]` | OAuth2 访问令牌 |
| `baidu_refresh_token` | `[待补充]` | OAuth2 刷新令牌 |
| `baidu_remote_path` | `/apps/VAULT/vault.db` | 网盘存储路径 |
| `baidu_sync_enabled` | `false` | 是否启用百度同步 |
| `quark_cookie` | `[待补充]` | 夸克网盘登录 Cookie |
| `quark_remote_path` | `/VAULT/vault.db` | 夸克网盘存储路径 |
| `quark_sync_enabled` | `false` | 是否启用夸克同步 |

> 标注 `[待补充]` 的参数请在应用设置页面中填写。详细获取方式见 [CONFIG.md](./CONFIG.md)。

## 已知限制

1. **夸克网盘上传**：夸克无官方 API，上传流程复杂，当前仅实现下载。建议优先使用百度网盘同步
2. **冲突处理**：文件级同步不支持增量合并，多设备同时修改可能冲突。建议同一时间只在一台设备上编辑
3. **编辑功能**：当前版本仅支持新增和删除，编辑已有密码记录功能待开发

## 国内网络配置

项目已配置 rsproxy.cn 镜像加速（`src-tauri/.cargo/config.toml`），无需额外设置。

如果遇到网络问题，可修改镜像配置：
```toml
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

## 许可证

私有项目，未公开发布。
