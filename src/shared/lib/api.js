// ============================================
// Tauri 后端 API 封装
// 所有 window.__TAURI__.core.invoke / event.listen 调用集中于此
// ============================================

const tauriCore = window.__TAURI__ && window.__TAURI__.core;
const tauriWindow = window.__TAURI__ && window.__TAURI__.window;
const tauriEvent = window.__TAURI__ && window.__TAURI__.event;

// 动态加载 dialog 插件（移动端使用 SAF 文件选择器）
let _dialogModule = null;
let _dialogError = false;
async function getDialogModule() {
  if (_dialogError) return null;
  if (_dialogModule) return _dialogModule;
  try {
    _dialogModule = await import('@tauri-apps/plugin-dialog');
    return _dialogModule;
  } catch (e) {
    console.warn('[api] dialog 插件加载失败:', e);
    _dialogError = true;
    return null;
  }
}

// ========== 文件选择器封装（移动端使用 SAF，桌面端走 prompt） ==========
//
// 设计说明：
// - 移动端 Android 没有 prompt 弹窗，且无法手动输入文件路径
// - 改用 tauri-plugin-dialog 的 open/save 调用系统 SAF 文件选择器
// - save() 返回选择的目录 DocumentURI（content://...），传给后端 export_db
// - open() 返回选择的文件 DocumentURI，传给后端 import_db
// - 桌面端保持原有 prompt() 方式不变，返回值仍为相对/绝对路径字符串
// - 二者签名一致：均返回 string|null，由调用方判断
//
// 参考 tauri-plugin-dialog v2 API：
//   open(options): Promise<string | string[] | null>
//   save(options): Promise<string | null>

/**
 * 打开"导出文件"选择器
 * @param {string} defaultName 默认文件名（仅桌面端 prompt 用到，移动端由 SAF 决定）
 * @returns {Promise<string|null>} 用户选择的保存路径/DocumentURI，取消则返回 null
 */
export async function saveFileDialog(defaultName = 'vault_export.json') {
  const dialog = await getDialogModule();
  if (!dialog || typeof dialog.save !== 'function') {
    // 桌面端或插件不可用：fallback 到 prompt
    return prompt('请输入导出文件路径：', defaultName);
  }
  try {
    const result = await dialog.save({
      defaultPath: defaultName,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    // save() 在用户取消时返回 null
    return result || null;
  } catch (e) {
    console.warn('[api] saveFileDialog 失败:', e);
    return null;
  }
}

/**
 * 打开"导入文件"选择器
 * @returns {Promise<string|null>} 用户选择的文件路径/DocumentURI，取消则返回 null
 */
export async function openFileDialog() {
  const dialog = await getDialogModule();
  if (!dialog || typeof dialog.open !== 'function') {
    // 桌面端或插件不可用：fallback 到 prompt
    return prompt('请输入要导入的数据库文件路径：', 'vault_import.json');
  }
  try {
    const result = await dialog.open({
      multiple: false,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    // open() 在用户取消时返回 null
    if (!result) return null;
    // multiple:false 时返回 string，但若用户多选可能返回数组，强制取首项
    if (Array.isArray(result)) return result[0] || null;
    return result;
  } catch (e) {
    console.warn('[api] openFileDialog 失败:', e);
    return null;
  }
}

export function hasTauri() {
  return !!(tauriCore && tauriCore.invoke);
}

export function invoke(cmd, args) {
  if (!tauriCore || !tauriCore.invoke) {
    return Promise.reject(new Error('Tauri invoke 不可用：' + cmd));
  }
  return tauriCore.invoke(cmd, args);
}

// ========== 密码 CRUD ==========

export function getAllPasswords() {
  return invoke('get_all_passwords');
}

export function addPassword(data) {
  return invoke('add_password', { data });
}

export function updatePassword(id, data) {
  return invoke('update_password', { id, data });
}

export function deletePassword(id) {
  return invoke('delete_password', { id });
}

export function toggleFavorite(id) {
  return invoke('toggle_favorite', { id });
}

export function updateLastUsed(id) {
  return invoke('update_last_used', { id }).catch(() => {});
}

export function generatePassword(length = 16) {
  return invoke('generate_password', { length });
}

// ========== 配置 ==========

export function getConfig() {
  return invoke('get_config');
}

export function saveConfig(config) {
  return invoke('save_config', { config });
}

// ========== 存储 ==========

export function getDbInfo() {
  return invoke('get_db_info');
}

export function exportDb(filePath) {
  return invoke('export_db', { filePath });
}

export function importDb(filePath) {
  return invoke('import_db', { filePath });
}

// ========== 云同步 ==========

export function syncNow(provider, direction) {
  return invoke('sync_now', { provider, direction });
}

export function logout(provider) {
  return invoke('logout', { provider });
}

export function openQuarkLogin() {
  return invoke('open_quark_login');
}

export function openBaiduLogin() {
  return invoke('open_baidu_login');
}

// ========== QR 扫码登录 ==========

export function qrStart(provider) {
  return invoke('qr_start', { provider });
}

export function qrPoll(provider, loginToken) {
  return invoke('qr_poll', { provider, loginToken });
}

// ========== 窗口控制 ==========

export function getCurrentWindow() {
  return tauriWindow && tauriWindow.getCurrentWindow
    ? tauriWindow.getCurrentWindow()
    : null;
}

export async function showMainWindow() {
  try {
    const win = getCurrentWindow();
    if (win) await win.show();
  } catch (_) { /* ignore */ }
}

// ========== 事件监听 ==========

export function onSyncExpired(handler) {
  if (!tauriEvent || !tauriEvent.listen) return () => {};
  return tauriEvent.listen('sync://expired', handler);
}

export function onSyncError(handler) {
  if (!tauriEvent || !tauriEvent.listen) return () => {};
  return tauriEvent.listen('sync://error', handler);
}

export function onSyncRestored(handler) {
  if (!tauriEvent || !tauriEvent.listen) return () => {};
  return tauriEvent.listen('sync://restored', handler);
}

export function onBaiduLoginSuccess(handler) {
  if (!tauriEvent || !tauriEvent.listen) return () => {};
  return tauriEvent.listen('baidu-login-success', handler);
}

export function onQuarkLoginSuccess(handler) {
  if (!tauriEvent || !tauriEvent.listen) return () => {};
  return tauriEvent.listen('quark-login-success', handler);
}
