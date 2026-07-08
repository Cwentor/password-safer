// ============================================
// Sidebar 组件（桌面专属）
// 侧栏骨架：分类导航 + 标签云容器 + 存储信息
// 标签云数据渲染调用 shared/components/PasswordList.renderTagsCloud()
// ============================================

export function getSidebarHTML() {
  return `
  <aside class="sidebar">
    <div class="sidebar-section">
      <div class="sidebar-label">分类</div>
      <ul class="nav-list">
        <li class="nav-item active" data-filter="all">
          <svg class="nav-icon" viewBox="0 0 24 24"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>
          全部密码
          <span class="nav-count" id="countAll">0</span>
        </li>
        <li class="nav-item" data-filter="favorites">
          <svg class="nav-icon" viewBox="0 0 24 24"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
          收藏夹
          <span class="nav-count" id="countFav">0</span>
        </li>
        <li class="nav-item" data-filter="recent">
          <svg class="nav-icon" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
          最近使用
          <span class="nav-count" id="countRecent">0</span>
        </li>
        <li class="nav-item" data-filter="weak">
          <svg class="nav-icon" viewBox="0 0 24 24"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          弱密码
          <span class="nav-count" id="countWeak" style="color:#ff4444">0</span>
        </li>
      </ul>
    </div>

    <div class="sidebar-section">
      <div class="sidebar-label">标签</div>
      <div class="tags-cloud" id="tagsCloud"></div>
    </div>

    <div class="storage-info">
      <div class="sidebar-label" style="margin-bottom:12px">本地存储</div>
      <div class="storage-bar">
        <div class="storage-fill" id="storageFill" style="width: 0%"></div>
      </div>
      <div class="storage-text">
        <span>已使用</span>
        <span id="storageSize">—</span>
      </div>
    </div>
  </aside>
  `;
}

export function initSidebar() {
  // 侧栏导航与标签云的事件监听由 PasswordList.initPasswordList() 统一绑定
  // 此处仅作为骨架渲染入口，无独立事件
}
