// ============================================
// StorageInfo 组件
// updateStorageInfo() - 侧栏存储信息 + 设置页存储条目数/占用条
// ============================================

import { getDbInfo } from '../lib/api.js';
import { formatBytes } from '../lib/utils.js';

export async function updateStorageInfo() {
  try {
    const info = await getDbInfo();
    const countEl = document.getElementById('storageCount');
    if (countEl) countEl.textContent = `${info.count} 条`;

    const sizeBytes = info.size || 0;
    const sizeText = formatBytes(sizeBytes);

    // 侧边栏本地存储显示（桌面）
    const sizeEl = document.getElementById('storageSize');
    if (sizeEl) sizeEl.textContent = sizeText;
    const fillEl = document.getElementById('storageFill');
    if (fillEl) {
      // 以 10 MB 为参考刻度展示占用比例
      const pct = Math.min(100, Math.round((sizeBytes / (10 * 1024 * 1024)) * 100));
      fillEl.style.width = pct + '%';
    }

    // 设置-存储页占用条（两端共用；id 与侧栏不同避免重复）
    const settingsSizeEl = document.getElementById('storageSizeSettings');
    if (settingsSizeEl) settingsSizeEl.textContent = sizeText;
    const settingsFillEl = document.getElementById('storageFillSettings');
    if (settingsFillEl) {
      const pct = Math.min(100, Math.round((sizeBytes / (10 * 1024 * 1024)) * 100));
      settingsFillEl.style.width = pct + '%';
    }
  } catch (e) { /* 静默 */ }
}
