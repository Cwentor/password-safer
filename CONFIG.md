# Password Safer 配置说明

本文档列出了应用运行所需的全部配置参数。除手动填入路径与同步间隔外，Cookie 类凭证均可在应用内通过扫码登录自动获取。

---

## 一、百度网盘云同步

百度网盘采用 **Web Cookie + bdstoken** 鉴权方式（非 OAuth 开放平台 API）。bdstoken 由应用在每次上传前自动通过 `pan.baidu.com/api/gettemplatevariable` 接口从 Cookie 中提取，无需用户手动填写。

### 1.1 获取 Cookie（应用内扫码登录 · 推荐）

1. 打开应用 → 设置 → 「云同步」标签 → 百度网盘区域
2. 点击「扫码登录」按钮，弹出二维码
3. 使用百度网盘手机 App 扫码并确认授权
4. 应用自动轮询登录状态，成功后自动写入 `baidu_cookie` 与 `baidu_cookie_expires_at`
5. 开启「自动同步」开关即可

### 1.2 配置参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `baidu_cookie` | 百度网盘登录 Cookie（应用内扫码自动获取） | 空 |
| `baidu_remote_path` | 网盘中的数据文件路径 | `/apps/VAULT/vault.json` |
| `baidu_sync_enabled` | 是否启用百度网盘同步 | `false` |
| `baidu_sync_interval` | 自动同步间隔（秒） | `300`（5 分钟） |
| `baidu_cookie_expires_at` | Cookie 过期时间戳（秒，0 表示未知） | `0` |
| `baidu_last_sync` | 上次同步时间字符串（自动维护） | 空 |

### 1.3 同步能力说明

- **上传**：`precreate` → 分片 `upload` → `create`
- **下载**：通过 `d.pcs.baidu.com` 直接下载
- **单向上传**：当前未实现 `remote_file_mtime`，调度器仅执行上传，不支持双向同步

> Cookie 有时效性，过期后需重新扫码登录。应用会在 Cookie 失效时自动停用同步并提示。

---

## 二、夸克网盘云同步

夸克网盘无官方开放 API，采用 Web 端 **HttpOnly Cookie** 鉴权方式。应用通过 Tauri 2 的 `cookies_for_url()` API 获取含 HttpOnly 标记的完整 Cookie，确保上传鉴权所需字段齐全。

### 2.1 获取 Cookie

#### 方式一：应用内 WebView 扫码登录（推荐）

1. 打开应用 → 设置 → 「云同步」标签 → 夸克网盘区域
2. 点击「扫码登录」按钮，弹出 WebView 窗口加载 `pan.quark.cn/account/login`
3. 使用夸克 App 扫码并确认登录
4. 应用后台轮询 Cookie，成功后自动写入 `quark_cookie` 与 `quark_cookie_expires_at`，并关闭窗口
5. 开启「自动同步」开关即可

> WebView 已注入反反爬虫脚本：移除 `window.__TAURI__` 全局对象、伪装 Chrome 138 用户代理。

#### 方式二：浏览器手动抓取（备选）

1. 在浏览器中登录 [夸克网盘](https://pan.quark.cn)
2. 按 `F12` 打开开发者工具
3. 切换到 `Network` 标签页
4. 刷新页面，点击任意请求
5. 在请求头中找到 `Cookie` 字段，复制完整值粘贴到设置页

> 注意：浏览器 `document.cookie` 无法读取 HttpOnly Cookie，手动方式可能遗漏鉴权字段，建议优先使用应用内扫码。

### 2.2 配置参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `quark_cookie` | 夸克网盘登录 Cookie（含 HttpOnly） | 空 |
| `quark_remote_path` | 网盘中的数据文件路径 | `/VAULT/vault.json` |
| `quark_sync_enabled` | 是否启用夸克网盘同步 | `false` |
| `quark_sync_interval` | 自动同步间隔（秒） | `300`（5 分钟） |
| `quark_cookie_expires_at` | Cookie 过期时间戳（秒，0 表示未知） | `0` |
| `quark_last_sync` | 上次同步时间字符串（自动维护） | 空 |
| `quark_last_remote_mtime` | 上次同步时云端文件 mtime（双向同步用，自动维护） | `0` |

### 2.3 同步能力说明

- **上传**：`file/upload/pre` → `file/update/hash`（秒传判断）→ 分片 PUT → `commit` → `finish`
- **断点续传**：`.uploadmeta` 元文件记录已上传分片
- **冲突处理**：23008 doloading 状态自动复用现有目录 fid
- **双向同步**：比较本地 mtime 与云端 mtime，新者覆盖旧者，相等则跳过

### 2.4 注意事项

- Cookie 有时效性，过期后需重新登录
- 非官方接口可能随夸克网盘版本更新而失效
- Cookie 距过期 <7 天时应用会弹窗预警，过期自动停用同步

---

## 三、通用设置

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `auto_lock_minutes` | 闲置自动锁定时间（分钟） | `30` |
| `clipboard_clear_seconds` | 复制后剪贴板自动清空时间（秒） | `30` |
| `master_password` | 主密码（可选，用于额外保护） | 空（不启用） |

---

## 四、数据文件

### 4.1 文件位置

- 数据文件：`%APPDATA%\password-safer\vault.json`
- 加密密钥：`%APPDATA%\password-safer\master.key`

### 4.2 数据结构

数据文件 `vault.json` 存储加密后的 JSON 内容。解密后的结构为 `StoreData`，包含两个字段：

- `passwords`：密码记录数组，每条记录包含 `id` / `name` / `icon` / `url` / `username` / `password_encrypted` / `tags` / `notes` / `favorite` / `strength` / `created` / `last_used`
- `config`：应用配置对象

整个 JSON 内容使用 AES-256-GCM 加密。

### 4.3 导入/导出

- **导出**：将当前 `vault.json` 文件复制到指定路径
- **导入**：从外部 JSON 文件读取密码记录，解密后重新加密导入当前数据文件
  - 支持本应用导出的加密格式
  - 也支持明文存储的外部 JSON 文件
  - 自动验证数据结构

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

本应用采用**文件级同步**策略：将整个加密 JSON 文件上传到网盘，而非同步单条记录。

- **上传同步**：将本地 `vault.json` 上传到网盘，覆盖远程文件
- **下载同步**：从网盘下载 `vault.json`，覆盖本地文件
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
