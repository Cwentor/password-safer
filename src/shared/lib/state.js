// ============================================
// 共享应用状态
// 组件间通过此模块读写共享状态，避免全局变量
// ============================================

export const state = {
  // 密码数据
  passwords: [],
  selectedId: null,

  // 列表筛选 / 搜索 / 排序
  currentFilter: 'all',
  currentTag: null,
  searchQuery: '',
  currentSort: 'name',

  // 应用配置
  appConfig: null,

  // 详情内联编辑态
  editMode: false,
  editBuffer: null,           // {name, icon, url, username, password, tags, notes}
  pendingCloseAfterSave: false, // 确认对话框"是"分支使用

  // 设置页：网盘详情展开状态
  providerDetailExpanded: { quark: false, baidu: false },

  // QR 扫码登录轮询状态
  qrPolling: false,
  qrCurrentToken: null,
  qrCurrentProvider: null,
  qrStatus: 'idle',          // idle | waiting | scanned | expired | failed | confirmed
  qrProactiveTimer: null,    // 主动刷新计时器
};

// QR 主动刷新延时（预估 token 约 2 分钟，90 秒时主动刷新）
export const QR_PROACTIVE_MS = 90 * 1000;

// 默认配置（加载失败时回退）
export const DEFAULT_CONFIG = {
  baidu_cookie: '', baidu_remote_path: '/apps/VAULT/vault.json',
  baidu_sync_enabled: false, baidu_sync_interval: 300,
  baidu_cookie_expires_at: 0, baidu_last_sync: '',
  quark_cookie: '', quark_remote_path: '/VAULT/vault.json',
  quark_sync_enabled: false, quark_sync_interval: 300,
  quark_cookie_expires_at: 0, quark_last_sync: '',
  auto_lock_minutes: 30, clipboard_clear_seconds: 30, master_password: ''
};
