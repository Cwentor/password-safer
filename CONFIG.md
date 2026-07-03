# Password Safer 配置说明

本文档列出了应用运行所需的全部配置参数。标注 `[待补充]` 的参数需要你提供具体值后才能启用对应功能。

---

## 一、百度网盘云同步

百度网盘提供官方开放平台 API，需要先注册应用获取凭证。

### 1.1 申请步骤

1. 访问 [百度网盘开放平台](https://pan.baidu.com/union)
2. 注册开发者账号，创建应用
3. 获取 `AppKey`（client_id）和 `SecretKey`（client_secret）
4. 设置回调地址（可填 `oob`）

### 1.2 配置参数

| 参数 | 说明 | 当前值 |
|------|------|--------|
| `baidu_app_key` | 百度网盘应用 AppKey | `[待补充]` |
| `baidu_secret_key` | 百度网盘应用 SecretKey | `[待补充]` |
| `baidu_access_token` | OAuth2 访问令牌（有效期 30 天） | `[待补充]` - 通过授权流程获取 |
| `baidu_refresh_token` | OAuth2 刷新令牌（有效期 10 年） | `[待补充]` - 通过授权流程获取 |
| `baidu_remote_path` | 网盘中的数据库文件路径 | `/apps/VAULT/vault.db` |
| `baidu_sync_enabled` | 是否启用百度网盘同步 | `false` |
| `baidu_sync_interval` | 自动同步间隔（秒） | `300`（5 分钟） |

### 1.3 获取 access_token

在浏览器中打开以下 URL 进行授权（替换 `YOUR_APP_KEY`）：

```
https://openapi.baidu.com/oauth/2.0/authorize?response_type=code&client_id=YOUR_APP_KEY&redirect_uri=oob&scope=basic+netdisk&display=popup
```

授权后获取 `code`，然后用以下 URL 换取 token（替换 `YOUR_CODE`、`YOUR_APP_KEY`、`YOUR_SECRET_KEY`）：

```
https://openapi.baidu.com/oauth/2.0/token?grant_type=authorization_code&code=YOUR_CODE&client_id=YOUR_APP_KEY&client_secret=YOUR_SECRET_KEY&redirect_uri=oob
```

返回的 JSON 中包含 `access_token` 和 `refresh_token`，填入应用设置即可。

---

## 二、夸克网盘云同步

夸克网盘没有官方开放 API，采用 Web 端 Cookie 鉴权方式。

### 2.1 获取 Cookie

1. 在浏览器中登录 [夸克网盘](https://pan.quark.cn)
2. 按 `F12` 打开开发者工具
3. 切换到 `Network` 标签页
4. 刷新页面，点击任意请求
5. 在请求头中找到 `Cookie` 字段，复制完整值

### 2.2 配置参数

| 参数 | 说明 | 当前值 |
|------|------|--------|
| `quark_cookie` | 夸克网盘登录 Cookie | `[待补充]` |
| `quark_remote_path` | 网盘中的数据库文件路径 | `/VAULT/vault.db` |
| `quark_sync_enabled` | 是否启用夸克网盘同步 | `false` |
| `quark_sync_interval` | 自动同步间隔（秒） | `300`（5 分钟） |

### 2.3 注意事项

- Cookie 有时效性，过期后需要重新获取
- 非官方接口可能随夸克网盘版本更新而失效
- 上传功能需要进一步完善（下载功能已实现）
- 建议优先使用百度网盘同步（官方 API 更稳定）

---

## 三、通用设置

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `auto_lock_minutes` | 闲置自动锁定时间（分钟） | `30` |
| `clipboard_clear_seconds` | 复制后剪贴板自动清空时间（秒） | `30` |
| `master_password` | 主密码（可选，用于额外保护） | 空（不启用） |

---

## 四、数据库文件

### 4.1 文件位置

- 数据库文件：`%APPDATA%\password-safer\vault.db`
- 加密密钥：`%APPDATA%\password-safer\master.key`

### 4.2 数据库结构

```sql
-- 密码表
CREATE TABLE passwords (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,              -- 名称
    icon TEXT DEFAULT '🔑',          -- 图标
    url TEXT DEFAULT '',             -- 网址
    username TEXT DEFAULT '',        -- 用户名
    password_encrypted TEXT NOT NULL,-- AES-256-GCM 加密后的密码
    tags TEXT DEFAULT '[]',          -- 标签（JSON 数组）
    notes TEXT DEFAULT '',           -- 备注
    favorite INTEGER DEFAULT 0,      -- 是否收藏
    strength INTEGER DEFAULT 0,      -- 密码强度（1-4）
    created TEXT DEFAULT '',         -- 创建日期
    last_used TEXT DEFAULT ''        -- 最近使用日期
);

-- 配置表
CREATE TABLE app_config (
    key TEXT PRIMARY KEY,
    value TEXT
);
```

### 4.3 导入/导出

- **导出**：将当前 `vault.db` 文件复制到指定路径
- **导入**：从外部 SQLite 文件读取密码记录，解密后重新加密导入当前数据库
  - 支持本应用导出的加密格式
  - 也支持明文存储的外部数据库
  - 自动验证表结构

---

## 五、加密说明

- 加密算法：AES-256-GCM（认证加密）
- 密钥：首次运行时随机生成 256 位密钥，存储在 `master.key` 文件中
- 加密范围：仅 `password_encrypted` 字段加密，其他字段（名称、用户名等）明文存储
- 密钥文件与数据库文件分离存储，增强安全性

> **重要提醒**：`master.key` 文件是解密密码的唯一凭证。请妥善备份此文件。如果丢失，已加密的密码将无法恢复。

---

## 六、同步机制说明

### 同步策略

本应用采用**文件级同步**策略：将整个 SQLite 数据库文件上传到网盘，而非同步单条记录。

- **上传同步**：将本地 `vault.db` 上传到网盘，覆盖远程文件
- **下载同步**：从网盘下载 `vault.db`，覆盖本地文件
- **自动同步**：按配置间隔自动上传（默认 5 分钟）
- **手动同步**：在设置页面点击「立即同步」

### 多设备使用

1. 设备 A 创建密码 → 自动/手动上传到网盘
2. 设备 B 打开应用 → 手动下载网盘文件 → 恢复全部数据
3. 注意：下载会覆盖本地数据，请确保不会丢失新增记录

---

## 七、构建与运行

### 环境要求

- Rust（stable 工具链）
- Node.js 18+
- Windows: 需要 MSVC 构建工具或 Visual Studio C++ 工具

### 开发运行

```bash
# 安装前端依赖
npm install

# 开发模式运行
npm run tauri dev

# 构建生产版本
npm run tauri build
```

### 构建产物

构建完成后，安装包位于 `src-tauri/target/release/bundle/` 目录下：
- `nsis/` - NSIS 安装程序
- `msi/` - MSI 安装包
