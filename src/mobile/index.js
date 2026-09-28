// ============================================
// 移动端入口
// 平台外壳：MobileHeader + 搜索栏 + 内容区 + BottomNav。
// 模态、浮层、渲染注册、组件初始化、关浮层栈共用 layout.js。
// Tab 切换：全部/收藏（列表）/添加（弹窗）/标签（标签云）/设置（弹窗）
// ============================================

import '../shared/styles/tokens.css';
import '../shared/styles/base.css';
import './styles/mobile.css';

import { loadConfig, loadPasswords } from '../shared/lib/dataLoader.js';
import { showMainWindow } from '../shared/lib/api.js';
import { state } from '../shared/lib/state.js';
import { showToast } from '../shared/components/Toast.js';
import { renderPasswordList, renderTagsCloud, countTags } from '../shared/components/PasswordList.js';
import { openAddModal } from '../shared/components/AddModal.js';
import { openSettingsModal, populateSettingsForm, listenSyncEvents } from '../shared/components/SettingsModal.js';
import {
  mountSharedOverlays, registerCoreRenderers, initSharedComponents,
  bindOverlayEscape, getDetailHTML,
} from '../shared/components/layout.js';

import { getMobileHeaderHTML, initMobileHeader } from './components/MobileHeader.js';
import { getMobileSearchBarHTML } from './components/MobileSearchBar.js';
import { getBottomNavHTML, initBottomNav, setActiveTab } from './components/BottomNav.js';
import { initLongPressMenu, tryCloseLongPressMenu } from './components/LongPressMenu.js';

// 主区域 HTML（含内容容器，根据 Tab 切换）
function getMainHTML() {
  return `
  <main class="mobile-main">
    <div class="mobile-content" id="mobileContent">
      <!-- 默认显示密码列表 -->
      <div class="password-list mobile-password-list" id="passwordList">
        <div class="list-header">
          <span class="list-title" id="listTitle">全部密码</span>
          <span class="list-count" id="listCount">0 个条目</span>
        </div>
      </div>
    </div>
    <!-- 标签云容器（默认隐藏，点击标签 Tab 时显示） -->
    <div class="mobile-tags-view" id="mobileTagsView" style="display:none">
      <div class="list-header">
        <span class="list-title">标签云</span>
        <span class="list-count" id="mobileTagsCount">0 个标签</span>
      </div>
      <div class="tags-cloud" id="tagsCloud"></div>
    </div>
  </main>
  `;
}

// ========== 加载骨架屏（app-ready 触发前占据内容区） ==========

function showLoadingSkeleton() {
  const content = document.getElementById('mobileContent');
  if (!content) return;
  const skeleton = document.createElement('div');
  skeleton.id = 'appLoadingSkeleton';
  skeleton.style.cssText = 'display:flex;align-items:center;justify-content:center;flex-direction:column;height:60vh;color:var(--text-secondary,#888);font-size:15px;';
  skeleton.innerHTML = '<div style="margin-bottom:10px;">加载中...</div><div style="font-size:12px;opacity:.7;">正在解密数据，请稍候</div>';
  content.appendChild(skeleton);
}

function hideLoadingSkeleton() {
  const skeleton = document.getElementById('appLoadingSkeleton');
  if (skeleton) skeleton.remove();
}

// ========== 初始化序列 ==========

// app-ready 触发后的实际初始化序列
async function doInit() {
  hideLoadingSkeleton();
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

async function init() {
  const tauriEvent = window.__TAURI__ && window.__TAURI__.event;

  // 监听 app-error：后端初始化失败时提示
  if (tauriEvent && tauriEvent.listen) {
    tauriEvent.listen('app-error', (event) => {
      hideLoadingSkeleton();
      showToast('应用初始化失败: ' + (event.payload || '未知错误'));
      showMainWindow();
    });
  }

  // 监听 app-ready：后端异步初始化完成后开始调用 invoke
  let ready = false;
  if (tauriEvent && tauriEvent.listen) {
    tauriEvent.listen('app-ready', () => {
      ready = true;
      doInit();
    });
  } else {
    // 无 Tauri 事件能力时直接初始化（兜底）
    ready = true;
    doInit();
  }

  // 超时保护：30 秒未收到 app-ready 则提示
  setTimeout(() => {
    if (!ready) {
      showToast('初始化超时，请重启应用');
      showMainWindow();
    }
  }, 30000);
}

// 挂载到 #app 元素
export function mount(appElement) {
  // 构建移动布局
  const appDiv = document.createElement('div');
  appDiv.className = 'mobile-app';
  appDiv.innerHTML = getMobileHeaderHTML() + getMobileSearchBarHTML() + getMainHTML() + getDetailHTML(true) + getBottomNavHTML();
  appElement.appendChild(appDiv);

  // 弹窗与 Toast 挂到 body；注册渲染函数；初始化共享组件（含长按菜单）
  mountSharedOverlays({ mobile: true });
  registerCoreRenderers();
  initSharedComponents({ longPress: initLongPressMenu });
  initMobileHeader({ onSettingsClick: () => openSettingsModal() });

  // 底部导航：Tab 切换逻辑
  initBottomNav({
    onTabChange: (key) => {
      state.currentFilter = key;
      state.currentTag = null;
      // 切换视图：显示密码列表，隐藏标签云
      const listView = document.getElementById('mobileContent');
      const tagsView = document.getElementById('mobileTagsView');
      if (listView) listView.style.display = '';
      if (tagsView) tagsView.style.display = 'none';
      renderPasswordList();
    },
    onAddClick: () => openAddModal(),
    onSettingsClick: () => openSettingsModal(),
    onTagsClick: () => {
      // 切换到标签云视图
      const listView = document.getElementById('mobileContent');
      const tagsView = document.getElementById('mobileTagsView');
      if (listView) listView.style.display = 'none';
      if (tagsView) tagsView.style.display = '';
      // 更新标签计数（计数逻辑与标签云共用）
      const counter = countTags(state.passwords);
      const countEl = document.getElementById('mobileTagsCount');
      if (countEl) countEl.textContent = `${Object.keys(counter).length} 个标签`;
      renderTagsCloud();
      // 高亮标签 Tab（取消其他高亮）
      setActiveTab('tags');
    },
  });

  // 安卓返回键 / Escape 关浮层（长按菜单最优先，含二维码弹窗）
  bindOverlayEscape({
    keys: ['Escape', 'Backspace'],
    before: (e) => {
      if (tryCloseLongPressMenu()) {
        e.preventDefault();
        return true;
      }
      return false;
    },
    qrModal: true,
  });

  // 显示加载骨架屏（app-ready 触发前占据内容区）
  showLoadingSkeleton();

  // 启动初始化序列（等待 app-ready 事件后再调用 invoke）
  init();
}
