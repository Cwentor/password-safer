// ============================================
// 密码数据服务
// 处理需要跨组件协作的 CRUD 操作（更新 state + 调用 API + 触发重渲染）
// ============================================

import { state } from './state.js';
import { renderers } from './renderer.js';
import { showToast } from '../components/Toast.js';
import {
  toggleFavorite as apiToggleFavorite,
} from './api.js';

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
