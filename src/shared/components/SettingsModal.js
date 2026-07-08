// ============================================
// SettingsModal 组件
// 设置弹窗（4 Tab：云同步/存储/安全/关于）+ 同步配置 + 事件监听
// ============================================

import { state } from '../lib/state.js';
import { renderers } from '../lib/renderer.js';
import { showToast } from './Toast.js';
import { formatExpires } from '../lib/utils.js';
import { loadConfig, reloadConfigAndUI, loadPasswords } from '../lib/dataLoader.js';
import { saveConfig, syncNow, logout } from '../lib/api.js';
import { startQrLogin } from './QrLoginModal.js';
import { exportDb, importDb, saveFileDialog, openFileDialog } from '../lib/api.js';
import {
  onSyncExpired, onSyncError, onSyncRestored,
  onBaiduLoginSuccess, onQuarkLoginSuccess
} from '../lib/api.js';
import { isMobile } from '../lib/platform.js';

let settingsModalEl = null;

export function initSettingsModal() {
  settingsModalEl = document.getElementById('settingsModal');
  const settingsBtn = document.getElementById('settingsBtn');
  const settingsClose = document.getElementById('settingsClose');

  if (settingsBtn) {
    settingsBtn.addEventListener('click', async () => {
      await loadConfig();
      populateSettingsForm();
      if (settingsModalEl) settingsModalEl.classList.add('show');
    });
  }

  if (settingsClose) settingsClose.addEventListener('click', () => settingsModalEl.classList.remove('show'));
  if (settingsModalEl) {
    settingsModalEl.addEventListener('click', (e) => {
      if (e.target === settingsModalEl) settingsModalEl.classList.remove('show');
    });
  }

  // Tab 切换
  document.querySelectorAll('.settings-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      const target = tab.dataset.tab;
      document.querySelectorAll('.settings-tab').forEach(t => t.classList.remove('active'));
      document.querySelectorAll('.settings-panel').forEach(p => p.style.display = 'none');
      tab.classList.add('active');
      const panel = document.getElementById('tab-' + target);
      if (panel) panel.style.display = 'block';
    });
  });

  initSyncFields();
  initImportExport();
  registerSyncWindowGlobals();
  // listenSyncEvents() 由 desktop/index.js 的 init() 序列统一调用，避免重复注册
}

export function openSettingsModal() {
  if (settingsModalEl) settingsModalEl.classList.add('show');
}

// ========== 独立云同步弹窗（桌面端） ==========

export function initCloudSyncModal() {
  const cloudSyncModalEl = document.getElementById('cloudSyncModal');
  const cloudSyncBtn = document.getElementById('cloudSyncBtn');
  const cloudSyncClose = document.getElementById('cloudSyncClose');

  if (cloudSyncBtn) {
    cloudSyncBtn.addEventListener('click', async () => {
      await loadConfig();
      populateSettingsForm();
      if (cloudSyncModalEl) cloudSyncModalEl.classList.add('show');
    });
  }

  if (cloudSyncClose) cloudSyncClose.addEventListener('click', () => cloudSyncModalEl.classList.remove('show'));
  if (cloudSyncModalEl) {
    cloudSyncModalEl.addEventListener('click', (e) => {
      if (e.target === cloudSyncModalEl) cloudSyncModalEl.classList.remove('show');
    });
  }
}

// ========== 表单填充 ==========

export function populateSettingsForm() {
  if (!state.appConfig) return;
  updateProviderUI('quark');
  updateProviderUI('baidu');

  setVal('quarkRemotePath', state.appConfig.quark_remote_path);
  setVal('quarkInterval', String(state.appConfig.quark_sync_interval || 300));
  setVal('baiduRemotePath', state.appConfig.baidu_remote_path);
  setVal('baiduInterval', String(state.appConfig.baidu_sync_interval || 300));

  setText('quarkLastSync', state.appConfig.quark_last_sync || '—');
  setText('baiduLastSync', state.appConfig.baidu_last_sync || '—');
  setText('quarkExpires', formatExpires(state.appConfig.quark_cookie_expires_at));
  setText('baiduExpires', formatExpires(state.appConfig.baidu_cookie_expires_at));

  renderers.updateStorageInfo();
}

function setVal(id, v) { const el = document.getElementById(id); if (el) el.value = v; }
function setText(id, v) { const el = document.getElementById(id); if (el) el.textContent = v; }

export function updateProviderUI(provider) {
  const cookie = provider === 'quark' ? state.appConfig.quark_cookie : state.appConfig.baidu_cookie;
  const enabled = provider === 'quark' ? state.appConfig.quark_sync_enabled : state.appConfig.baidu_sync_enabled;
  const statusEl = document.getElementById(provider + 'Status');
  const toggleEl = document.getElementById(provider + 'Toggle');
  const detailEl = document.getElementById(provider + 'Detail');
  const chevronEl = document.getElementById(provider + 'Chevron');
  // 箭头始终显示，仅当已绑定且有详情时才联动详情显隐
  const showDetail = cookie && state.providerDetailExpanded[provider];
  if (detailEl) detailEl.style.display = showDetail ? 'block' : 'none';
  if (chevronEl) chevronEl.classList.toggle('expanded', state.providerDetailExpanded[provider]);
  if (!cookie) {
    if (statusEl) { statusEl.textContent = '未绑定 · 点击扫码'; statusEl.classList.remove('connected'); }
    if (toggleEl) toggleEl.classList.remove('active');
  } else {
    if (enabled) {
      if (statusEl) { statusEl.textContent = '已绑定 · 自动同步中'; statusEl.classList.add('connected'); }
      if (toggleEl) toggleEl.classList.add('active');
    } else {
      if (statusEl) { statusEl.textContent = '已绑定 · 未开启自动同步'; statusEl.classList.add('connected'); }
      if (toggleEl) toggleEl.classList.remove('active');
    }
  }
}

export function updateSyncUI() {
  if (!state.appConfig) return;
  const anySync = (state.appConfig.baidu_sync_enabled && state.appConfig.baidu_cookie) ||
                  (state.appConfig.quark_sync_enabled && state.appConfig.quark_cookie);
  const dot = document.querySelector('.status-dot');
  if (dot) dot.classList.toggle('syncing', anySync);
}

// ========== 网盘详情展开 ==========

function toggleProviderDetail(provider) {
  state.providerDetailExpanded[provider] = !state.providerDetailExpanded[provider];
  const cookie = provider === 'quark' ? state.appConfig.quark_cookie : state.appConfig.baidu_cookie;
  const detailEl = document.getElementById(provider + 'Detail');
  const chevronEl = document.getElementById(provider + 'Chevron');
  if (chevronEl) chevronEl.classList.toggle('expanded', state.providerDetailExpanded[provider]);
  if (detailEl) detailEl.style.display = (cookie && state.providerDetailExpanded[provider]) ? 'block' : 'none';
}

// ========== 同步开关 ==========

async function toggleSync(provider) {
  if (!state.appConfig) return;
  const cookie = provider === 'quark' ? state.appConfig.quark_cookie : state.appConfig.baidu_cookie;
  const providerName = provider === 'quark' ? '夸克网盘' : '百度网盘';

  // 未绑定 Cookie 时直接唤起登录（夸克、百度均走 WebView 方案）
  if (!cookie) {
    if (provider === 'quark') {
      window.openQuarkLogin();
    } else if (provider === 'baidu') {
      window.openBaiduLogin();
    } else {
      startQrLogin(provider);
    }
    return;
  }

  // 已绑定：切换自动同步开关
  if (provider === 'baidu') {
    state.appConfig.baidu_sync_enabled = !state.appConfig.baidu_sync_enabled;
  } else {
    state.appConfig.quark_sync_enabled = !state.appConfig.quark_sync_enabled;
  }

  try {
    await saveConfig(state.appConfig);
    const enabled = provider === 'quark' ? state.appConfig.quark_sync_enabled : state.appConfig.baidu_sync_enabled;
    showToast(providerName + '自动同步已' + (enabled ? '开启' : '关闭'));
    updateProviderUI(provider);
    updateSyncUI();
  } catch (e) {
    showToast('保存配置失败: ' + e);
  }
}

async function manualSync(provider) {
  const providerName = provider === 'quark' ? '夸克网盘' : '百度网盘';
  showToast('正在同步到' + providerName + '...');
  try {
    const result = await syncNow(provider, 'upload');
    showToast(result.message);
    if (result.success) reloadConfigAndUI();
  } catch (e) {
    showToast('同步失败: ' + e);
  }
}

async function manualDownload(provider) {
  const providerName = provider === 'quark' ? '夸克网盘' : '百度网盘';
  if (!confirm('确定要从云端覆盖本地数据库吗？此操作不可撤销。')) return;
  showToast('正在从' + providerName + '恢复...');
  try {
    const result = await syncNow(provider, 'download');
    showToast(result.message);
    if (result.success) {
      await loadPasswords();
      renderers.updateCounts();
      renderers.renderTagsCloud();
      renderers.updateStorageInfo();
    }
  } catch (e) {
    showToast('恢复失败: ' + e);
  }
}

async function logoutProvider(provider) {
  if (!confirm('确定要退出登录吗？将清空本地 Cookie 并关闭自动同步。')) return;
  try {
    await logout(provider);
    await reloadConfigAndUI();
    showToast('已退出登录');
  } catch (e) {
    showToast('退出失败: ' + e);
  }
}

// ========== 备份路径 / 同步间隔自动保存 ==========

function initSyncFields() {
  const fields = [
    ['quarkRemotePath', 'quark_remote_path', false],
    ['quarkInterval', 'quark_sync_interval', true],
    ['baiduRemotePath', 'baidu_remote_path', false],
    ['baiduInterval', 'baidu_sync_interval', true],
  ];
  fields.forEach(([id, key, isInt]) => {
    const el = document.getElementById(id);
    if (!el) return;
    el.addEventListener('change', async () => {
      if (!state.appConfig) return;
      let val = el.value;
      if (isInt) val = parseInt(val, 10) || 300;
      state.appConfig[key] = val;
      try {
        await saveConfig(state.appConfig);
        showToast('设置已保存');
      } catch (e) {
        showToast('保存失败: ' + e);
      }
    });
  });
}

// ========== 导入导出 ==========
//
// 平台分支策略（SubTask 5.2）：
// - 桌面端：保持原有 prompt() 方式，用户手动输入路径
// - 移动端：调用 saveFileDialog/openFileDialog 走 SAF 文件选择器
//   （Android WebView 无 window.prompt，必须改用原生对话框）

function initImportExport() {
  document.addEventListener('click', async (e) => {
    const btn = e.target.closest('.storage-card-footer .secondary-btn');
    if (!btn) return;
    const text = btn.textContent.trim();
    if (text === '导出数据库') {
      let path;
      if (isMobile()) {
        path = await saveFileDialog('vault_export.json');
      } else {
        path = prompt('请输入导出文件路径：', 'vault_export.json');
      }
      if (path) {
        try {
          await exportDb(path);
          showToast('数据库已导出到: ' + path);
        } catch (err) {
          showToast('导出失败: ' + err);
        }
      }
    } else if (text === '导入数据') {
      let path;
      if (isMobile()) {
        path = await openFileDialog();
      } else {
        path = prompt('请输入要导入的数据库文件路径：', 'vault_import.json');
      }
      if (path) {
        try {
          const result = await importDb(path);
          if (result.success) {
            showToast(result.message);
            await loadPasswords();
            renderers.updateCounts();
            renderers.renderTagsCloud();
            renderers.updateStorageInfo();
          } else {
            showToast('导入失败: ' + result.message);
          }
        } catch (err) {
          showToast('导入失败: ' + err);
        }
      }
    }
  });
}

// ========== window 全局函数（供设置页内联 onclick 调用） ==========

function registerSyncWindowGlobals() {
  window.toggleSync = toggleSync;
  window.manualSync = manualSync;
  window.manualDownload = manualDownload;
  window.logoutProvider = logoutProvider;
  window.toggleProviderDetail = toggleProviderDetail;
}

// ========== 监听后端同步事件 ==========

export function listenSyncEvents() {
  onSyncExpired((e) => {
    const p = (e.payload && e.payload.provider) || '';
    const providerName = p === 'quark' ? '夸克网盘' : '百度网盘';
    showToast(providerName + ' 登录已失效，请重新扫码');
    reloadConfigAndUI();
  });
  onSyncError((e) => {
    const p = (e.payload && e.payload.provider) || '';
    const providerName = p === 'quark' ? '夸克网盘' : '百度网盘';
    const msg = (e.payload && e.payload.message) || '未知错误';
    showToast(providerName + ' 同步出错: ' + msg);
  });
  onSyncRestored(async () => {
    // 从云恢复完成，数据库已重新打开，刷新前端列表
    try {
      await loadPasswords();
      renderers.updateCounts();
      renderers.renderTagsCloud();
      renderers.updateStorageInfo();
    } catch (e) {
      console.error('restored refresh failed:', e);
    }
  });
  onBaiduLoginSuccess(() => {
    reloadConfigAndUI();
    showToast('百度网盘登录成功');
  });
  onQuarkLoginSuccess(async (e) => {
    const cookie = (typeof e.payload === 'string') ? e.payload : (e.payload && e.payload.cookie) || '';
    if (!cookie) return;
    state.appConfig.quark_cookie = cookie;
    state.appConfig.quark_cookie_expires_at = Math.floor(Date.now() / 1000) + 45 * 86400;
    try {
      await saveConfig(state.appConfig);
      showToast('夸克网盘登录成功');
      reloadConfigAndUI();
    } catch (err) {
      showToast('保存夸克登录态失败: ' + err);
    }
  });
}
