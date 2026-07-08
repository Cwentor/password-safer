// ============================================
// TitleBar 组件（桌面专属）
// 自定义标题栏：品牌 / Slogan / 设置按钮 / 锁定按钮 / 窗口控制
// 包含拖拽、双击切换最大化、最小化/最大化/关闭
// ============================================

import { getCurrentWindow } from '../../shared/lib/api.js';

// 标题栏 HTML 结构
export function getTitleBarHTML() {
  return `
  <header class="topbar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <div class="brand-logo">
        <svg viewBox="0 0 24 24"><rect x="4" y="10" width="16" height="11" rx="1"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/><circle cx="12" cy="15" r="1.5"/></svg>
      </div>
      <div class="brand-text">VAULT<span>.</span></div>
    </div>

    <div class="topbar-slogan" data-tauri-drag-region>
      <span class="slogan-text">本地加密存储 · 云端安全同步</span>
    </div>

    <div class="topbar-actions">
      <button class="icon-btn" id="settingsBtn" title="设置">
        <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
      </button>
      <button class="icon-btn" id="lockBtn" title="锁定">
        <svg viewBox="0 0 24 24"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
      </button>

      <div class="window-controls">
        <button class="win-btn" id="winMinimize" title="最小化">
          <svg viewBox="0 0 12 12"><line x1="2" y1="6" x2="10" y2="6"/></svg>
        </button>
        <button class="win-btn" id="winMaximize" title="最大化">
          <svg viewBox="0 0 12 12"><rect x="2.5" y="2.5" width="7" height="7" rx="0.5"/></svg>
        </button>
        <button class="win-btn close" id="winClose" title="关闭">
          <svg viewBox="0 0 12 12"><line x1="3" y1="3" x2="9" y2="9"/><line x1="9" y1="3" x2="3" y2="9"/></svg>
        </button>
      </div>
    </div>
  </header>
  `;
}

export function initTitleBar() {
  const win = getCurrentWindow();

  const minBtn = document.getElementById('winMinimize');
  const maxBtn = document.getElementById('winMaximize');
  const closeBtn = document.getElementById('winClose');

  if (minBtn && win) minBtn.addEventListener('click', () => win.minimize());
  if (closeBtn && win) closeBtn.addEventListener('click', () => win.close());

  if (maxBtn && win) {
    // 双击 topbar 切换最大化（Windows 习惯）
    const topbar = document.querySelector('.topbar');
    if (topbar) {
      topbar.addEventListener('dblclick', (e) => {
        // 排除按钮区域的点击
        if (e.target.closest('.icon-btn') || e.target.closest('.win-btn')) return;
        win.toggleMaximize();
      });
    }
    maxBtn.addEventListener('click', () => win.toggleMaximize());

    // 监听最大化状态变化，切换图标
    const maxIcon = maxBtn.querySelector('svg');
    const restoreIcon = '<svg viewBox="0 0 12 12"><rect x="2.5" y="4" width="5.5" height="5.5" rx="0.5"/><path d="M4.5 4V2.5H10V8H8.5" fill="none"/></svg>';
    const normalIcon = '<svg viewBox="0 0 12 12"><rect x="2.5" y="2.5" width="7" height="7" rx="0.5"/></svg>';
    if (win.onResized) {
      win.onResized(() => {
        win.isMaximized().then(isMax => {
          if (maxIcon) maxBtn.innerHTML = isMax ? restoreIcon : normalIcon;
        }).catch(() => {});
      });
    }
  }

  // 锁定按钮（Phase 1 暂无后端实现，保留按钮但无操作）
  const lockBtn = document.getElementById('lockBtn');
  if (lockBtn) {
    lockBtn.addEventListener('click', () => {
      // 锁定功能 Phase 1 暂未实现，保留按钮以维持视觉一致性
    });
  }
}
