// ============================================
// StorageInfo 组件
// updateStorageInfo() - 侧栏存储信息 + 设置页存储条目数
// ============================================

import { getDbInfo } from '../lib/api.js';
import { formatBytes } from '../lib/utils.js';

export async function updateStorageInfo() {
  try {
    const info = await getDbInfo();
    const countEl = document.getElementById('storageCount');
    if (countEl) countEl.textContent = `${info.count} 条`;

    // 侧边栏本地存储显示
    const sizeBytes = info.size || 0;
    const sizeText = formatBytes(sizeBytes);
    const sizeEl = document.getElementById('storageSize');
    if (sizeEl) sizeEl.textContent = sizeText;
    const fillEl = document.getElementById('storageFill');
    if (fillEl) {
      // 以 10 MB 为参考刻度展示占用比例
      const pct = Math.min(100, Math.round((sizeBytes / (10 * 1024 * 1024)) * 100));
      fillEl.style.width = pct + '%';
    }
  } catch (e) { /* 静默 */ }
}
