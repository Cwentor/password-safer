// ============================================
// 数据加载 / 配置重载
// 集中管理 loadPasswords / loadConfig / reloadConfigAndUI
// ============================================

import { state, DEFAULT_CONFIG } from './state.js';
import { renderers } from './renderer.js';
import { showToast } from '../components/Toast.js';
import { getAllPasswords, getConfig } from './api.js';

export async function loadPasswords() {
  try {
    state.passwords = await getAllPasswords();
    renderers.renderPasswordList();
    renderers.renderTagsCloud();
  } catch (e) {
    showToast('加载密码失败: ' + e);
    state.passwords = [];
    renderers.renderPasswordList();
    renderers.renderTagsCloud();
  }
}

export async function loadConfig() {
  try {
    state.appConfig = await getConfig();
  } catch (e) {
    state.appConfig = { ...DEFAULT_CONFIG };
  }
}

export async function reloadConfigAndUI() {
  await loadConfig();
  renderers.populateSettingsForm();
  renderers.updateSyncUI();
}
