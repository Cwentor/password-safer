// ============================================
// LongPressMenu 组件（移动端长按上下文菜单）
//
// 设计：
// - 监听密码卡片的长按事件（touchstart 起 500ms 计时）
// - 弹出底部 ActionSheet，提供：复制密码 / 收藏 / 删除 三个操作
// - 点击遮罩或安卓返回键关闭
// - 长按触发时阻止默认行为（避免系统文本选择菜单）
// - 与桌面端右键菜单等价
// ============================================

import { state } from '../../shared/lib/state.js';
import { showToast } from '../../shared/components/Toast.js';
import { copyToClipboard } from '../../shared/lib/clipboard.js';
import { toggleFavorite as apiToggleFavorite, deletePassword, updateLastUsed } from '../../shared/lib/api.js';
import { renderers } from '../../shared/lib/renderer.js';

let sheetEl = null;
let backdropEl = null;
let currentPasswordId = null;
let longPressTimer = null;
let longPressTriggered = false;

const LONG_PRESS_DURATION = 500; // ms

// ========== ActionSheet HTML ==========

function getActionSheetHTML() {
  return `
  <div class="mobile-actionsheet-backdrop" id="longPressBackdrop"></div>
  <div class="mobile-actionsheet" id="longPressSheet" role="dialog" aria-modal="true">
    <div class="actionsheet-handle"></div>
    <div class="actionsheet-title" id="actionsheetTitle">密码操作</div>
    <button class="actionsheet-item" data-action="copy">
      <svg viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
      <span>复制密码</span>
    </button>
    <button class="actionsheet-item" data-action="favorite">
      <svg viewBox="0 0 24 24"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
      <span id="favoriteLabel">收藏</span>
    </button>
    <button class="actionsheet-item actionsheet-danger" data-action="delete">
      <svg viewBox="0 0 24 24"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
      <span>删除</span>
    </button>
    <button class="actionsheet-cancel">取消</button>
  </div>
  `;
}

// ========== 长按事件监听 ==========

function attachLongPressListener() {
  const list = document.getElementById('passwordList');
  if (!list) return;

  // 使用 touchstart/touchend 实现长按检测
  list.addEventListener('touchstart', (e) => {
    const card = e.target.closest('.password-card');
    if (!card) return;
    // 排除点击 card-action-btn 的情况
    if (e.target.closest('.card-action-btn')) return;

    const id = parseInt(card.dataset.id, 10);
    if (!id) return;

    longPressTriggered = false;
    longPressTimer = setTimeout(() => {
      longPressTriggered = true;
      // 触发振动反馈（Android 支持）
      if (navigator.vibrate) navigator.vibrate(30);
      showActionSheet(id);
    }, LONG_PRESS_DURATION);
  }, { passive: true });

  // 触摸结束/移动/取消时清除计时器
  ['touchend', 'touchmove', 'touchcancel'].forEach(evt => {
    list.addEventListener(evt, () => {
      if (longPressTimer) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
    }, { passive: true });
  });

  // 阻止长按后的系统上下文菜单
  list.addEventListener('contextmenu', (e) => {
    if (longPressTriggered) {
      e.preventDefault();
    }
  });
}

// ========== ActionSheet 显示/隐藏 ==========

function showActionSheet(id) {
  currentPasswordId = id;
  const p = state.passwords.find(x => x.id === id);
  if (!p) return;

  if (sheetEl) {
    // 更新收藏按钮文本
    const favLabel = sheetEl.querySelector('#favoriteLabel');
    if (favLabel) favLabel.textContent = p.favorite ? '取消收藏' : '收藏';
    const titleEl = sheetEl.querySelector('#actionsheetTitle');
    if (titleEl) titleEl.textContent = p.name || '密码操作';

    backdropEl.classList.add('show');
    sheetEl.classList.add('show');
  }
}

function hideActionSheet() {
  if (backdropEl) backdropEl.classList.remove('show');
  if (sheetEl) sheetEl.classList.remove('show');
  currentPasswordId = null;
}

// ========== 操作处理 ==========

async function handleAction(action) {
  const id = currentPasswordId;
  if (!id) return;
  const p = state.passwords.find(x => x.id === id);
  if (!p) return;

  hideActionSheet();

  switch (action) {
    case 'copy': {
      copyToClipboard(p.password, '密码已复制');
      updateLastUsed(p.id);
      break;
    }
    case 'favorite': {
      try {
        await apiToggleFavorite(id);
        p.favorite = !p.favorite;
        renderers.renderPasswordList();
        renderers.updateCounts();
        showToast(p.favorite ? '已添加到收藏夹' : '已从收藏夹移除');
      } catch (e) {
        showToast('操作失败: ' + e);
      }
      break;
    }
    case 'delete': {
      if (!confirm('确定要删除这个密码吗？此操作不可撤销。')) return;
      try {
        await deletePassword(id);
        state.passwords = state.passwords.filter(x => x.id !== id);
        renderers.renderPasswordList();
        renderers.updateCounts();
        renderers.renderTagsCloud();
        renderers.updateStorageInfo();
        showToast('密码已删除');
      } catch (e) {
        showToast('删除失败: ' + e);
      }
      break;
    }
  }
}

// ========== 初始化 ==========

export function initLongPressMenu() {
  // 注入 HTML（挂到 body）
  document.body.insertAdjacentHTML('beforeend', getActionSheetHTML());
  sheetEl = document.getElementById('longPressSheet');
  backdropEl = document.getElementById('longPressBackdrop');

  // 绑定操作按钮
  if (sheetEl) {
    sheetEl.querySelectorAll('[data-action]').forEach(btn => {
      btn.addEventListener('click', () => {
        handleAction(btn.dataset.action);
      });
    });
    const cancelBtn = sheetEl.querySelector('.actionsheet-cancel');
    if (cancelBtn) cancelBtn.addEventListener('click', hideActionSheet);
  }

  // 点击遮罩关闭
  if (backdropEl) backdropEl.addEventListener('click', hideActionSheet);

  // 阻止 sheet 自身点击事件冒泡到 backdrop
  if (sheetEl) {
    sheetEl.addEventListener('click', (e) => e.stopPropagation());
  }

  // 绑定长按监听
  attachLongPressListener();
}

// 供 back button 调用
export function tryCloseLongPressMenu() {
  if (sheetEl && sheetEl.classList.contains('show')) {
    hideActionSheet();
    return true;
  }
  return false;
}
