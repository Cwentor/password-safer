// ============================================
// MobileSearchBar 组件（移动专属）
// 常驻搜索栏：替代桌面端 Ctrl+F 快捷键
// 与桌面端共享 #searchInput ID，复用 PasswordList.initPasswordList() 的搜索监听
// ============================================

export function getMobileSearchBarHTML() {
  return `
  <div class="mobile-search-container">
    <div class="mobile-search-box">
      <svg class="mobile-search-icon" viewBox="0 0 24 24"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
      <input type="text" class="mobile-search-input" id="searchInput" placeholder="搜索密码、标签、备注...">
    </div>
  </div>
  `;
}
