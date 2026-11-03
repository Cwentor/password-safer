// ============================================
// ListControls 组件（移动端列表筛选/排序面板）
//
// 设计：
// - 列表头「筛选/排序」按钮 → 底部 ActionSheet（复用长按菜单样式体系）
// - 分类：全部 / 收藏 / 最近使用 / 弱密码（与桌面侧栏四类对齐）
// - 排序：名称 / 最近使用 / 创建时间 / 密码强度（与桌面排序栏对齐，
//   写入 state.currentSort，由共享 PasswordList 渲染时消费）
// - 选择即生效并关闭面板；遮罩点击 / 取消 / 安卓返回键关闭
// - 事件用 sheet 容器上的委托绑定，选项每次打开重绘也能响应
// ============================================

import { state } from '../../shared/lib/state.js';
import { renderPasswordList } from '../../shared/components/PasswordList.js';
import { setActiveTab } from './BottomNav.js';

let sheetEl = null;
let backdropEl = null;

// 分类定义：key 与 state.currentFilter 对齐
const FILTERS = [
  { key: 'all', label: '全部密码' },
  { key: 'favorites', label: '收藏夹' },
  { key: 'recent', label: '最近使用' },
  { key: 'weak', label: '弱密码' },
];

// 排序定义：key 与 PasswordList 的 SORTERS 对齐
const SORTS = [
  { key: 'name', label: '按名称' },
  { key: 'recent', label: '按最近使用' },
  { key: 'created', label: '按创建时间' },
  { key: 'strength', label: '按密码强度' },
];

// 各分类条目数（侧栏 #countAll 等 id 移动端不存在，直接由 state 计算）
function filterCounts() {
  const ps = state.passwords;
  return {
    all: ps.length,
    favorites: ps.filter(p => p.favorite).length,
    recent: ps.filter(p => p.last_used).length,
    weak: ps.filter(p => p.strength <= 2).length,
  };
}

const CHECK_SVG = '<svg class="actionsheet-check" viewBox="0 0 24 24"><polyline points="20 6 9 17 4 12"/></svg>';

function getOptionsHTML() {
  const counts = filterCounts();
  const filterItems = FILTERS.map(f => `
    <button class="actionsheet-item${state.currentFilter === f.key ? ' active' : ''}" data-filter="${f.key}">
      ${CHECK_SVG}
      <span>${f.label}</span>
      <span class="actionsheet-count">${counts[f.key]}</span>
    </button>`).join('');
  const sortItems = SORTS.map(s => `
    <button class="actionsheet-item${state.currentSort === s.key ? ' active' : ''}" data-sort="${s.key}">
      ${CHECK_SVG}
      <span>${s.label}</span>
    </button>`).join('');
  return `
    <div class="actionsheet-handle"></div>
    <div class="actionsheet-group-label">分类</div>
    ${filterItems}
    <div class="actionsheet-group-label">排序</div>
    ${sortItems}
    <button class="actionsheet-cancel">取消</button>
  `;
}

// ========== 面板显示/隐藏 ==========

function showPanel() {
  if (!sheetEl) return;
  // 每次打开重绘选项，同步当前分类/排序高亮与计数
  sheetEl.innerHTML = getOptionsHTML();
  backdropEl.classList.add('show');
  sheetEl.classList.add('show');
}

function hidePanel() {
  if (backdropEl) backdropEl.classList.remove('show');
  if (sheetEl) sheetEl.classList.remove('show');
}

// ========== 选择处理 ==========

function applyFilter(key) {
  state.currentFilter = key;
  state.currentTag = null;
  hidePanel();
  // 分类与底部导航联动：all/favorites 有对应 Tab，recent/weak 无
  setActiveTab(key === 'all' || key === 'favorites' ? key : null);
  renderPasswordList();
}

function applySort(key) {
  state.currentSort = key;
  hidePanel();
  renderPasswordList();
}

// ========== 初始化 ==========

export function initListControls() {
  // 注入面板骨架（选项内容每次打开重绘）
  document.body.insertAdjacentHTML('beforeend', `
    <div class="mobile-actionsheet-backdrop" id="listControlsBackdrop"></div>
    <div class="mobile-actionsheet" id="listControlsSheet" role="dialog" aria-modal="true"></div>
  `);
  backdropEl = document.getElementById('listControlsBackdrop');
  sheetEl = document.getElementById('listControlsSheet');

  // 列表头入口按钮（renderPasswordList 重渲染会保留 .list-header 节点，绑定不丢失）
  const btn = document.getElementById('listControlsBtn');
  if (btn) btn.addEventListener('click', showPanel);

  // 事件委托：选项 / 取消（绑定在 sheet 容器上，innerHTML 重绘不影响）
  if (sheetEl) {
    sheetEl.addEventListener('click', (e) => {
      const filterBtn = e.target.closest('[data-filter]');
      if (filterBtn) {
        applyFilter(filterBtn.dataset.filter);
        return;
      }
      const sortBtn = e.target.closest('[data-sort]');
      if (sortBtn) {
        applySort(sortBtn.dataset.sort);
        return;
      }
      if (e.target.closest('.actionsheet-cancel')) hidePanel();
    });
    sheetEl.addEventListener('click', (e) => e.stopPropagation());
  }

  // 点击遮罩关闭
  if (backdropEl) backdropEl.addEventListener('click', hidePanel);
}

// 供 back button 调用
export function tryCloseListControls() {
  if (sheetEl && sheetEl.classList.contains('show')) {
    hidePanel();
    return true;
  }
  return false;
}
