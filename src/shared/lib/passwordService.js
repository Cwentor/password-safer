// ============================================
// 密码簿模块：密码记录增删改的唯一入口
// 职责：调 API → 同步 state 缓存 → 统一刷新序列 → 统一提示。
// 视图行为（关闭详情面板等）不在此处，由调用方自理。
// ============================================

import { state } from './state.js';
import { renderers } from './renderer.js';
import { showToast } from '../components/Toast.js';
import {
  addPassword as apiAdd,
  updatePassword as apiUpdate,
  deletePassword as apiDelete,
  toggleFavorite as apiToggleFavorite,
} from './api.js';

// 增删改后的统一刷新序列（列表 / 计数 / 标签云 / 存储信息）
export function refreshAll() {
  renderers.renderPasswordList();
  renderers.updateCounts();
  renderers.renderTagsCloud();
  renderers.updateStorageInfo();
}

// 新建密码记录
export async function createPassword(input) {
  const record = await apiAdd(input);
  state.passwords.unshift(record);
  refreshAll();
  showToast('密码已保存');
  return record;
}

// 更新密码记录
export async function updatePassword(id, input) {
  const record = await apiUpdate(id, input);
  const idx = state.passwords.findIndex(x => x.id === id);
  if (idx >= 0) state.passwords[idx] = record;
  refreshAll();
  showToast('已保存');
  return record;
}

// 删除密码记录（内含确认对话框，取消返回 false）
export async function deletePassword(id) {
  if (!confirm('确定要删除这个密码吗？此操作不可撤销。')) return false;
  try {
    await apiDelete(id);
    state.passwords = state.passwords.filter(p => p.id !== id);
    refreshAll();
    showToast('密码已删除');
    return true;
  } catch (e) {
    showToast('删除失败: ' + e);
    return false;
  }
}

// 切换收藏
export async function toggleFavorite(id) {
  try {
    await apiToggleFavorite(id);
    const p = state.passwords.find(x => x.id === id);
    if (p) p.favorite = !p.favorite;
    renderers.renderPasswordList();
    renderers.updateCounts();
    showToast(p.favorite ? '已添加到收藏夹' : '已从收藏夹移除');
  } catch (e) {
    showToast('操作失败: ' + e);
  }
}
