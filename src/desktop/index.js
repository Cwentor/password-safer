// ============================================
// 桌面端入口
// 平台外壳：TitleBar + Sidebar + 主区（搜索/排序/列表）。
// 模态、浮层、渲染注册、组件初始化、关浮层栈共用 layout.js。
// ============================================

import '../shared/styles/tokens.css';
import '../shared/styles/base.css';
import './styles/desktop.css';

import { loadConfig, loadPasswords } from '../shared/lib/dataLoader.js';
import { showMainWindow } from '../shared/lib/api.js';
import { showToast } from '../shared/components/Toast.js';
import { populateSettingsForm, listenSyncEvents } from '../shared/components/SettingsModal.js';
import {
  mountSharedOverlays, registerCoreRenderers, initSharedComponents,
  bindOverlayEscape, getDetailHTML,
} from '../shared/components/layout.js';
import { getTitleBarHTML, initTitleBar } from './components/TitleBar.js';
import { getSidebarHTML, initSidebar } from './components/Sidebar.js';

// 主区域（桌面专属布局）
function getMainHTML() {
  return `
  <main class="main">
    <div class="search-container">
      <div class="search-wrapper">
        <div class="search-box">
          <svg class="search-icon" viewBox="0 0 24 24"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
          <input type="text" class="search-input" id="searchInput" placeholder="搜索密码、标签、备注...">
          <span class="search-shortcut">Ctrl+F</span>
        </div>
        <button class="add-btn" id="addBtn">
          <svg viewBox="0 0 24 24"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
          新建密码
        </button>
      </div>
    </div>

    <div class="filter-bar">
      <span class="filter-label">排序：</span>
      <div class="filter-tabs">
        <button class="filter-tab active" data-sort="name">名称</button>
        <button class="filter-tab" data-sort="recent">最近使用</button>
        <button class="filter-tab" data-sort="created">创建时间</button>
        <button class="filter-tab" data-sort="strength">密码强度</button>
      </div>
    </div>

    <div class="password-list" id="passwordList">
      <div class="list-header">
        <span class="list-title" id="listTitle">全部密码</span>
        <span class="list-count" id="listCount">0 个条目</span>
      </div>
    </div>
  </main>
  `;
}

// ========== 初始化序列 ==========

async function init() {
  try {
    await loadConfig();
    await loadPasswords();
    populateSettingsForm();
    listenSyncEvents();
  } catch (e) {
    showToast('初始化失败: ' + e);
  } finally {
    // 渲染完成（含失败）后显示窗口，避免启动时白闪
    await showMainWindow();
  }
}

// 挂载到 #app 元素
export function mount(appElement) {
  // 构建桌面布局
  const appDiv = document.createElement('div');
  appDiv.className = 'app';
  appDiv.innerHTML = getTitleBarHTML() + getSidebarHTML() + getMainHTML() + getDetailHTML(false);
  appElement.appendChild(appDiv);

  // 弹窗与 Toast 挂到 body；注册渲染函数；初始化共享组件（含独立云同步弹窗）
  mountSharedOverlays({ cloudSync: true });
  registerCoreRenderers();
  initTitleBar();
  initSidebar();
  initSharedComponents({ cloudSync: true });

  // 快捷键：Ctrl+F 聚焦搜索；Escape 按栈关闭浮层（含云同步弹窗）
  document.addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.key === 'f') {
      e.preventDefault();
      const searchInput = document.getElementById('searchInput');
      if (searchInput) searchInput.focus();
    }
  });
  bindOverlayEscape({ keys: ['Escape'], cloudSync: true });

  // 启动初始化序列
  init();
}
