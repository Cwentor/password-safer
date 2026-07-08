// ============================================
// BottomNav 组件（移动专属）
// 底部 5 项 TabBar：全部 / 收藏 / 添加 / 标签 / 设置
// 点击切换高亮，添加按钮触发 AddModal 而非切换 Tab
// ============================================

// Tab 定义：key 与 state.currentFilter 对齐（除 add/settings 外）
const TABS = [
  {
    key: 'all',
    label: '全部',
    icon: '<svg viewBox="0 0 24 24"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>',
  },
  {
    key: 'favorites',
    label: '收藏',
    icon: '<svg viewBox="0 0 24 24"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>',
  },
  {
    key: 'add',
    label: '添加',
    icon: '<svg viewBox="0 0 24 24"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>',
    isAction: true, // 不切换 Tab，触发打开 AddModal
  },
  {
    key: 'tags',
    label: '标签',
    icon: '<svg viewBox="0 0 24 24"><path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z"/><line x1="7" y1="7" x2="7.01" y2="7"/></svg>',
  },
  {
    key: 'settings',
    label: '设置',
    icon: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>',
  },
];

export function getBottomNavHTML() {
  const items = TABS.map(t => {
    const actionClass = t.isAction ? ' tab-action' : '';
    return `<button class="tab-item${actionClass}" data-tab="${t.key}" aria-label="${t.label}">
      <span class="tab-icon">${t.icon}</span>
      <span class="tab-label">${t.label}</span>
    </button>`;
  }).join('');
  return `<nav class="bottom-nav" id="bottomNav">${items}</nav>`;
}

let onTabChange = null;
let onAddClick = null;
let onSettingsClick = null;
let onTagsClick = null;

export function initBottomNav({ onTabChange: tc, onAddClick: ac, onSettingsClick: sc, onTagsClick: tg } = {}) {
  onTabChange = tc;
  onAddClick = ac;
  onSettingsClick = sc;
  onTagsClick = tg;

  const nav = document.getElementById('bottomNav');
  if (!nav) return;

  nav.querySelectorAll('.tab-item').forEach(btn => {
    btn.addEventListener('click', () => {
      const key = btn.dataset.tab;
      if (key === 'add') {
        if (onAddClick) onAddClick();
        return;
      }
      if (key === 'settings') {
        if (onSettingsClick) onSettingsClick();
        return;
      }
      if (key === 'tags') {
        if (onTagsClick) onTagsClick();
        return;
      }
      // 普通切换：all / favorites
      nav.querySelectorAll('.tab-item').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      if (onTabChange) onTabChange(key);
    });
  });

  // 默认激活第一项
  const first = nav.querySelector('.tab-item[data-tab="all"]');
  if (first) first.classList.add('active');
}

export function setActiveTab(key) {
  const nav = document.getElementById('bottomNav');
  if (!nav) return;
  nav.querySelectorAll('.tab-item').forEach(b => {
    b.classList.toggle('active', b.dataset.tab === key);
  });
}
