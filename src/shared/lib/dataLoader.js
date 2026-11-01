// ============================================
// 数据加载 / 配置重载
// 集中管理 loadPasswords / loadConfig / reloadConfigAndUI
// ============================================

import { state, DEFAULT_CONFIG, DEFAULT_SYNC_STATE } from './state.js';
import { renderers } from './renderer.js';
import { showToast } from '../components/Toast.js';
import { getAllPasswords, getConfig, getSyncState } from './api.js';
import { refreshAll } from './passwordService.js';

export async function loadPasswords() {
  try {
    state.passwords = await getAllPasswords();
    refreshAll();
  } catch (e) {
    showToast('加载密码失败: ' + e);
    state.passwords = [];
    refreshAll();
  }
}

export async function loadConfig() {
  try {
    state.appConfig = await getConfig();
  } catch (e) {
    state.appConfig = { ...DEFAULT_CONFIG };
  }
  // 同步记账（last_sync 等）存于本地 sync_state.json，与密码簿配置分开加载
  try {
    state.syncState = await getSyncState();
  } catch (e) {
    state.syncState = { ...DEFAULT_SYNC_STATE };
  }
}

export async function reloadConfigAndUI() {
  await loadConfig();
  renderers.populateSettingsForm();
}
