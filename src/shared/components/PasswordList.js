// ============================================
// PasswordList 组件
// renderPasswordList() + renderTagsCloud() + updateCounts() + initPasswordList()
// ============================================

import { state } from '../lib/state.js';
import { renderers } from '../lib/renderer.js';
import { escapeAttr, escapeHtml } from '../lib/utils.js';
import { renderPasswordCard } from './PasswordCard.js';
import { selectPassword } from './DetailPanel.js';

let passwordListEl = null;
let searchInputEl = null;
let listTitleEl = null;
let listCountEl = null;
let tagsCloudEl = null;

export function initPasswordList() {
  passwordListEl = document.getElementById('passwordList');
  searchInputEl = document.getElementById('searchInput');
  listTitleEl = document.getElementById('listTitle');
  listCountEl = document.getElementById('listCount');
  tagsCloudEl = document.getElementById('tagsCloud');

  // 搜索
  if (searchInputEl) {
    searchInputEl.addEventListener('input', (e) => {
      state.searchQuery = e.target.value.trim();
      renderPasswordList();
    });
  }

  // 导航分类
  document.querySelectorAll('.nav-item').forEach(item => {
    item.addEventListener('click', () => {
      document.querySelectorAll('.nav-item').forEach(i => i.classList.remove('active'));
      item.classList.add('active');
      state.currentFilter = item.dataset.filter;
      state.currentTag = null;
      document.querySelectorAll('.tag-chip').forEach(t => t.classList.remove('active'));
      renderPasswordList();
    });
  });

  // 标签云点击
  if (tagsCloudEl) {
    tagsCloudEl.addEventListener('click', (e) => {
      const chip = e.target.closest('.tag-chip');
      if (!chip) return;
      const tag = chip.dataset.tag;
      if (!tag) return;
      if (state.currentTag === tag) {
        state.currentTag = null;
        chip.classList.remove('active');
      } else {
        state.currentTag = tag;
        document.querySelectorAll('.tags-cloud .tag-chip').forEach(c => c.classList.remove('active'));
        chip.classList.add('active');
        document.querySelectorAll('.nav-item').forEach(i => i.classList.remove('active'));
        const allNav = document.querySelector('[data-filter="all"]');
        if (allNav) allNav.classList.add('active');
        state.currentFilter = 'all';
      }
      renderPasswordList();
    });
  }

  // 排序
  document.querySelectorAll('.filter-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.filter-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      state.currentSort = tab.dataset.sort;
    });
  });

  // 卡片选择事件（由 PasswordCard 派发）
  if (passwordListEl) {
    passwordListEl.addEventListener('card-select', (e) => {
      selectPassword(e.detail);
    });
  }
}

export function renderPasswordList() {
  if (!passwordListEl) return;
  let filtered = [...state.passwords];

  if (state.currentFilter === 'favorites') {
    filtered = filtered.filter(p => p.favorite);
  } else if (state.currentFilter === 'weak') {
    filtered = filtered.filter(p => p.strength <= 2);
  } else if (state.currentFilter === 'recent') {
    filtered.sort((a, b) => (b.last_used || '').localeCompare(a.last_used || ''));
  }

  if (state.currentTag) {
    filtered = filtered.filter(p => p.tags && p.tags.includes(state.currentTag));
  }

  if (state.searchQuery) {
    const query = state.searchQuery.toLowerCase();
    filtered = filtered.filter(p =>
      p.name.toLowerCase().includes(query) ||
      (p.username || '').toLowerCase().includes(query) ||
      (p.url || '').toLowerCase().includes(query) ||
      (p.tags || []).some(t => t.toLowerCase().includes(query)) ||
      (p.notes || '').toLowerCase().includes(query)
    );
  }

  if (state.currentFilter !== 'recent') {
    filtered.sort((a, b) => a.name.localeCompare(b.name, 'zh'));
  }

  if (listCountEl) listCountEl.textContent = `${filtered.length} 个条目`;
  if (listTitleEl) {
    if (state.currentTag) {
      listTitleEl.textContent = `标签：${state.currentTag}`;
    } else if (state.currentFilter === 'favorites') {
      listTitleEl.textContent = '收藏夹';
    } else if (state.currentFilter === 'weak') {
      listTitleEl.textContent = '弱密码';
    } else if (state.currentFilter === 'recent') {
      listTitleEl.textContent = '最近使用';
    } else {
      listTitleEl.textContent = '全部密码';
    }
  }

  const header = passwordListEl.querySelector('.list-header');
  passwordListEl.innerHTML = '';
  if (header) passwordListEl.appendChild(header);

  if (filtered.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'empty-state';
    empty.innerHTML = `
      <svg class="empty-icon" viewBox="0 0 24 24"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
      <div class="empty-title">未找到匹配的密码</div>
      <div class="empty-desc">试试其他关键词，或检查标签筛选</div>
    `;
    passwordListEl.appendChild(empty);
    return;
  }

  filtered.forEach(p => {
    passwordListEl.appendChild(renderPasswordCard(p));
  });
}

export function renderTagsCloud() {
  if (!tagsCloudEl) return;
  const counter = {};
  state.passwords.forEach(p => {
    (p.tags || []).forEach(t => {
      const key = String(t).trim();
      if (!key) return;
      counter[key] = (counter[key] || 0) + 1;
    });
  });
  const tags = Object.keys(counter).sort((a, b) => counter[b] - counter[a] || a.localeCompare(b, 'zh'));
  if (tags.length === 0) {
    tagsCloudEl.innerHTML = '<span style="font-size:12px;color:var(--text-dim)">暂无标签</span>';
    return;
  }
  tagsCloudEl.innerHTML = tags.map(t => {
    const active = state.currentTag === t ? ' active' : '';
    return `<span class="tag-chip${active}" data-tag="${escapeAttr(t)}"><span class="tag-dot"></span>${escapeHtml(t)}</span>`;
  }).join('');
}

export function updateCounts() {
  const countAll = document.getElementById('countAll');
  const countFav = document.getElementById('countFav');
  const countRecent = document.getElementById('countRecent');
  const countWeak = document.getElementById('countWeak');
  if (countAll) countAll.textContent = state.passwords.length;
  if (countFav) countFav.textContent = state.passwords.filter(p => p.favorite).length;
  if (countRecent) countRecent.textContent = state.passwords.filter(p => p.last_used).length;
  if (countWeak) countWeak.textContent = state.passwords.filter(p => p.strength <= 2).length;
}
