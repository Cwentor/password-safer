// ============================================
// Toast 组件
// showToast(message) + initToast() 初始化 DOM 引用
// ============================================

let toastEl = null;
let toastTextEl = null;
let toastTimer = null;

export function initToast() {
  toastEl = document.getElementById('toast');
  toastTextEl = document.getElementById('toastText');
}

export function showToast(message) {
  if (!toastEl || !toastTextEl) {
    // 兜底：DOM 尚未就绪时回退到 console
    console.log('[toast]', message);
    return;
  }
  toastTextEl.textContent = message;
  toastEl.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.remove('show'), 2000);
}
