// ============================================
// 剪贴板工具
// ============================================

import { showToast } from '../components/Toast.js';

export function copyToClipboard(text, message = '已复制到剪贴板') {
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(() => {
      showToast(message);
    }).catch(() => {
      fallbackCopy(text, message);
    });
  } else {
    fallbackCopy(text, message);
  }
}

function fallbackCopy(text, message) {
  const textarea = document.createElement('textarea');
  textarea.value = text;
  document.body.appendChild(textarea);
  textarea.select();
  try { document.execCommand('copy'); } catch (_) {}
  document.body.removeChild(textarea);
  showToast(message);
}
