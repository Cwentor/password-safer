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

  // 云同步本地记账（last_sync 等，来自 get_sync_state）
  syncState: null,

  // 密码簿状态（{format: none|v1|v2, unlocked}，来自 get_vault_status）
  vaultStatus: null,

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
  baidu_cookie_expires_at: 0,
  quark_cookie: '', quark_remote_path: '/VAULT/vault.json',
  quark_sync_enabled: false, quark_sync_interval: 300,
  quark_cookie_expires_at: 0,
  auto_lock_minutes: 30, clipboard_clear_seconds: 30, master_password: ''
};

// 同步记账默认值（get_sync_state 不可用时回退）
export const DEFAULT_SYNC_STATE = {
  device_id: '',
  local_dirty_at: 0,
  quark: { last_heads: [], last_sync: '' },
  baidu: { last_heads: [], last_sync: '' },
  conflict_pairs: [],
  key_mismatch: null,
  legacy_pending_delete: false
};
