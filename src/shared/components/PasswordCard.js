// ============================================
// PasswordCard 组件
// 渲染单个密码卡片，返回 DOM 元素
// ============================================

import { highlightText } from '../lib/utils.js';
import { updateLastUsed } from '../lib/api.js';
import { state } from '../lib/state.js';
import { copyToClipboard } from '../lib/clipboard.js';
import { toggleFavorite } from '../lib/passwordService.js';

export function renderPasswordCard(p) {
  const card = document.createElement('div');
  card.className = 'password-card' + (state.selectedId === p.id ? ' selected' : '');
  card.dataset.id = p.id;

  const nameHtml = highlightText(p.name, state.searchQuery);
  const userHtml = highlightText(p.username || '', state.searchQuery);

  card.innerHTML = `
    <div class="card-icon">${p.icon || '🔑'}</div>
    <div class="card-info">
      <div class="card-name">${nameHtml}</div>
      <div class="card-username">${userHtml}</div>
      <div class="card-tags">
        ${(p.tags || []).map(t => `<span class="card-tag">${t}</span>`).join('')}
      </div>
    </div>
    <div class="card-actions">
      <button class="card-action-btn card-favorite ${p.favorite ? 'active' : ''}" data-action="favorite" title="收藏">
        <svg viewBox="0 0 24 24"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
      </button>
      <button class="card-action-btn" data-action="copy" title="复制密码">
        <svg viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
      </button>
    </div>
  `;

  card.addEventListener('click', (e) => {
    if (e.target.closest('.card-action-btn')) return;
    // 由父级 PasswordList 绑定 select 逻辑（避免循环依赖）
    card.dispatchEvent(new CustomEvent('card-select', { detail: p.id, bubbles: true }));
    // 阻止冒泡到 document：selectPassword 内会 renderPasswordList() 重建卡片，
    // 导致原 e.target 脱离 DOM，document 的外部点击监听会误判为外部点击而关闭卡片
    e.stopPropagation();
  });

  card.querySelector('[data-action="copy"]').addEventListener('click', (e) => {
    e.stopPropagation();
    copyToClipboard(p.password, '密码已复制');
    updateLastUsed(p.id);
  });

  card.querySelector('[data-action="favorite"]').addEventListener('click', (e) => {
    e.stopPropagation();
    toggleFavorite(p.id);
  });

  return card;
}
